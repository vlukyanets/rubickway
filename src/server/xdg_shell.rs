//! `xdg_wm_base`, `xdg_surface`, `xdg_toplevel`, `xdg_popup`, `xdg_positioner` (research R9).
//!
//! A toplevel maps when its surface commits a buffer after acking the first configure; the
//! server then emits `WindowEvent::Mapped` and, on later changes, `Updated` / `Unmapped`.
//! Window-management requests (move and resize with a checked serial, maximize, fullscreen)
//! become `WindowEvent`s for the shell, which answers with `WindowCommand`s: configures carry
//! the size and states it wants, with one new size outstanding at a time (window-management
//! research W1-W5, W13, W14). Minimize and the window menu are ignored; popups are dismissed
//! at once.

use std::sync::Mutex;

use wayland_protocols::xdg::shell::server::xdg_popup::{self, XdgPopup};
use wayland_protocols::xdg::shell::server::xdg_positioner::{self, XdgPositioner};
use wayland_protocols::xdg::shell::server::xdg_surface::{self, XdgSurface};
use wayland_protocols::xdg::shell::server::xdg_toplevel::{self, XdgToplevel};
use wayland_protocols::xdg::shell::server::xdg_wm_base::{self, XdgWmBase};
use wayland_server::backend::{ClientId, ObjectId};
use wayland_server::{
    Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource, WEnum,
};

use super::compositor::{self, Role};
use super::{ServerState, output};
use crate::canvas::{Point, Rect, Size, Vector};
use crate::shell::{Edges, SizeLimits, States, WindowCommand, WindowEvent, WindowId};

pub(crate) fn create_global(dh: &DisplayHandle) {
    dh.create_global::<ServerState, XdgWmBase, ()>(6, ());
}

/// What an `xdg_surface` became.
pub(crate) enum XdgRole {
    None,
    Toplevel(XdgToplevel),
    Popup,
}

pub(crate) struct XdgSurfaceData {
    pub resource: XdgSurface,
    wm_base: XdgWmBase,
    /// The `wl_surface`.
    pub surface: ObjectId,
    pub role: XdgRole,
    /// Configure serials sent and not acked yet.
    serials: Vec<u32>,
    configure_sent: bool,
    acked: bool,
    pending_geometry: Option<Rect>,
    geometry: Option<Rect>,
    pub window: Option<WindowId>,
    /// Size, input shape and size limits last reported to the shell.
    reported: Option<(Size, Vec<Rect>, SizeLimits)>,
    /// `set_min_size` / `set_max_size`, applied on the next root commit (research W6).
    pending_limits: SizeLimits,
    limits: SizeLimits,
    title: String,
    app_id: String,
    pub activated: bool,
    cfg: ConfigureState,
    /// The application asked to be maximized / fullscreen (kept from before the initial
    /// commit for `Created`, research W13).
    wants_maximized: bool,
    wants_fullscreen: bool,
    /// The id given at the initial commit for `Created`; `map` reuses it.
    early_id: Option<WindowId>,
    /// The initial configure waits for the shell's answer to `Created`.
    awaiting_shell: bool,
    /// The parent toplevel's `xdg_surface` (`set_parent`, research W14).
    parent: Option<ObjectId>,
}

/// What the shell asked a toplevel for and where its configures stand (research W4, W5).
#[derive(Debug, Default)]
struct ConfigureState {
    /// The size the shell wants (`None`: the application chooses).
    wanted_size: Option<(i32, i32)>,
    wanted: States,
    /// The size in the last configure sent.
    sent_size: Option<(i32, i32)>,
    /// Serial of an unacked configure that carried a new size.
    size_serial: Option<u32>,
    /// A newer wanted size waits for `size_serial` to be acked; `deferred_from_shell` tells
    /// whether that configure answers a shell command.
    deferred: bool,
    deferred_from_shell: bool,
    /// Serial of the newest configure sent for a shell command.
    shell_serial: Option<u32>,
    /// The newest serial the application acked.
    acked_serial: Option<u32>,
    /// The `configured` value last reported to the shell.
    reported_configured: bool,
}

/// Whether serial `a` is `b` or newer (serials wrap).
fn serial_at_least(a: u32, b: u32) -> bool {
    (a.wrapping_sub(b) as i32) >= 0
}

/// A size side for the protocol: rounded, at least 1, saturating at `i32::MAX`
/// (Principle II: no unchecked conversion).
fn protocol_side(v: f64) -> i32 {
    if v.is_finite() {
        // A saturating float-to-int conversion of a value already clamped to the i32 range.
        v.round().clamp(1.0, f64::from(i32::MAX)) as i32
    } else {
        1
    }
}

fn protocol_size(size: Size) -> (i32, i32) {
    (protocol_side(size.w), protocol_side(size.h))
}

fn push_state(states: &mut Vec<u8>, state: xdg_toplevel::State) {
    states.extend_from_slice(&(state as u32).to_ne_bytes());
}

/// `set_size` and `set_anchor_rect` seen, needed for a complete positioner.
#[derive(Default)]
pub(crate) struct PositionerState {
    size: bool,
    anchor_rect: bool,
}

fn xdg_of_surface<'a>(state: &'a ServerState, surface: &ObjectId) -> Option<&'a XdgSurfaceData> {
    state.xdg_surfaces.values().find(|x| x.surface == *surface)
}

fn xdg_id_of_surface(state: &ServerState, surface: &ObjectId) -> Option<ObjectId> {
    state
        .xdg_surfaces
        .iter()
        .find(|(_, x)| x.surface == *surface)
        .map(|(id, _)| id.clone())
}

/// The window geometry of the window whose root surface is `root`, relative to that surface:
/// `set_window_geometry` clipped to the content, else the content's bounding box.
pub(crate) fn geometry_of(state: &ServerState, root: &ObjectId) -> Rect {
    let bounds = compositor::tree_bounds(state, root).unwrap_or_default();
    match xdg_of_surface(state, root).and_then(|x| x.geometry) {
        Some(g) => g.intersection(&bounds).unwrap_or(g),
        None => bounds,
    }
}

/// Size and input shape (window coordinates) of the window at `root`.
fn window_shape(state: &ServerState, root: &ObjectId) -> (Size, Vec<Rect>) {
    let geometry = geometry_of(state, root);
    let shift = Point::new(0.0, 0.0) - geometry.origin;
    let input = compositor::input_shape(state, root)
        .into_iter()
        .map(|r| r.translate(Vector::new(shift.x, shift.y)))
        .collect();
    (geometry.size, input)
}

/// Sends a configure with the wanted size and states plus `activated`. While a configure with
/// a new size is unacked, the size stays the one sent and the newer one is deferred until the
/// ack (coalescing, research W5). `from_shell`: it answers a shell command (for `configured`).
fn send_configure(state: &mut ServerState, xdg_id: &ObjectId, from_shell: bool) {
    let Some(x) = state.xdg_surfaces.get_mut(xdg_id) else {
        return;
    };
    let cfg = &mut x.cfg;
    if cfg.size_serial.is_some() && cfg.wanted_size != cfg.sent_size {
        // A new size while another is unacked: wait for the ack, then send the newest.
        cfg.deferred = true;
        cfg.deferred_from_shell |= from_shell;
        return;
    }
    let serial = state.next_serial();
    let Some(x) = state.xdg_surfaces.get_mut(xdg_id) else {
        return;
    };
    let XdgRole::Toplevel(toplevel) = &x.role else {
        return;
    };
    let cfg = &mut x.cfg;
    let size = cfg.wanted_size;
    let mut states = Vec::new();
    if x.activated {
        push_state(&mut states, xdg_toplevel::State::Activated);
    }
    if cfg.wanted.maximized {
        push_state(&mut states, xdg_toplevel::State::Maximized);
    }
    if cfg.wanted.fullscreen {
        push_state(&mut states, xdg_toplevel::State::Fullscreen);
    }
    if cfg.wanted.resizing {
        push_state(&mut states, xdg_toplevel::State::Resizing);
    }
    let (width, height) = size.unwrap_or((0, 0));
    toplevel.configure(width, height, states);
    x.resource.configure(serial);
    x.serials.push(serial);
    x.configure_sent = true;
    if size != cfg.sent_size {
        cfg.size_serial = Some(serial);
    }
    cfg.sent_size = size;
    if from_shell || std::mem::take(&mut cfg.deferred_from_shell) {
        cfg.shell_serial = Some(serial);
        cfg.reported_configured = false;
    }
    cfg.deferred = false;
}

/// After an ack: sends a configure deferred behind the acked size configure.
fn acked(state: &mut ServerState, xdg_id: &ObjectId, serial: u32) {
    let Some(x) = state.xdg_surfaces.get_mut(xdg_id) else {
        return;
    };
    let cfg = &mut x.cfg;
    cfg.acked_serial = Some(serial);
    let Some(size_serial) = cfg.size_serial else {
        return;
    };
    if !serial_at_least(serial, size_serial) {
        return;
    }
    cfg.size_serial = None;
    if cfg.deferred {
        send_configure(state, xdg_id, false);
    }
}

/// Whether the application has acked the newest configure sent for a shell command.
fn configured(cfg: &ConfigureState) -> bool {
    !cfg.deferred_from_shell
        && cfg
            .shell_serial
            .zip(cfg.acked_serial)
            .is_some_and(|(sent, acked)| serial_at_least(acked, sent))
}

/// Applies the shell's window commands (research W1).
pub(crate) fn command(state: &mut ServerState, commands: &[WindowCommand]) {
    for c in commands {
        match c {
            WindowCommand::Configure {
                window,
                size,
                states,
            } => {
                let Some(xdg_id) = xdg_id_of_window(state, *window) else {
                    continue;
                };
                if let Some(x) = state.xdg_surfaces.get_mut(&xdg_id) {
                    x.cfg.wanted_size = size.map(protocol_size);
                    x.cfg.wanted = *states;
                    // The shell's answer to `Created` is the initial configure.
                    x.awaiting_shell = false;
                }
                send_configure(state, &xdg_id, true);
            }
            WindowCommand::Close { window } => {
                if let Some(xdg_id) = xdg_id_of_window(state, *window)
                    && let Some(x) = state.xdg_surfaces.get(&xdg_id)
                    && let XdgRole::Toplevel(toplevel) = &x.role
                {
                    toplevel.close();
                }
            }
        }
    }
}

/// The `xdg_surface` of a mapped window, or of a window waiting for its initial configure.
fn xdg_id_of_window(state: &ServerState, window: WindowId) -> Option<ObjectId> {
    match state.windows.get(&window) {
        Some(root) => xdg_id_of_surface(state, root),
        None => state
            .xdg_surfaces
            .iter()
            .find(|(_, x)| x.window.is_none() && x.early_id == Some(window))
            .map(|(id, _)| id.clone()),
    }
}

/// The window id of `xdg_id`'s parent, if it has a mapped one.
fn parent_window(state: &ServerState, xdg_id: &ObjectId) -> Option<WindowId> {
    let parent = state.xdg_surfaces.get(xdg_id)?.parent.as_ref()?;
    state.xdg_surfaces.get(parent)?.window
}

/// `set_parent`: the protocol's rules (an unmapped parent counts as none; the toplevel itself
/// or a descendant is `invalid_parent`).
fn set_parent(
    state: &mut ServerState,
    resource: &XdgToplevel,
    xdg_id: &ObjectId,
    parent: Option<XdgToplevel>,
) {
    let parent = parent.and_then(|p| p.data::<ObjectId>().cloned());
    if let Some(p) = &parent {
        // Walk up from the new parent; reaching this toplevel means a loop. The walk is
        // bounded by the number of surfaces, as the links can only form a loop through us.
        let mut cur = Some(p.clone());
        for _ in 0..=state.xdg_surfaces.len() {
            let Some(c) = cur else {
                break;
            };
            if c == *xdg_id {
                resource.post_error(
                    xdg_toplevel::Error::InvalidParent,
                    "the parent is this toplevel or one of its descendants",
                );
                return;
            }
            cur = state.xdg_surfaces.get(&c).and_then(|x| x.parent.clone());
        }
    }
    let mapped = parent.filter(|p| {
        state
            .xdg_surfaces
            .get(p)
            .is_some_and(|x| x.window.is_some())
    });
    if let Some(x) = state.xdg_surfaces.get_mut(xdg_id) {
        x.parent = mapped;
    }
}

/// Changes the `activated` state of a window's toplevel (keyboard focus) and re-configures.
pub(crate) fn set_activated(state: &mut ServerState, window: WindowId, activated: bool) {
    let Some(root) = state.windows.get(&window).cloned() else {
        return;
    };
    let Some(xdg_id) = xdg_id_of_surface(state, &root) else {
        return;
    };
    let changed = match state.xdg_surfaces.get_mut(&xdg_id) {
        Some(x) if x.activated != activated => {
            x.activated = activated;
            true
        }
        _ => false,
    };
    if changed {
        send_configure(state, &xdg_id, false);
    }
}

/// Called after a surface of the tree rooted at `root` committed (`committed`).
pub(crate) fn tree_committed(state: &mut ServerState, root: &ObjectId, committed: &ObjectId) {
    let Some(xdg_id) = xdg_id_of_surface(state, root) else {
        return;
    };
    let has_buffer = state
        .surfaces
        .get(root)
        .is_some_and(|s| s.buffer_size.is_some());
    let Some(x) = state.xdg_surfaces.get_mut(&xdg_id) else {
        return;
    };
    if committed == root {
        x.geometry = x.pending_geometry;
        x.limits = x.pending_limits;
    }
    if !matches!(x.role, XdgRole::Toplevel(_)) {
        return;
    }
    if committed == root && (!x.configure_sent || !x.acked) {
        if has_buffer {
            x.resource.post_error(
                xdg_surface::Error::UnconfiguredBuffer,
                "a buffer was committed before the first configure was acked",
            );
            return;
        }
        if !x.configure_sent && !x.awaiting_shell {
            if x.resource.version() >= 5
                && let XdgRole::Toplevel(t) = &x.role
            {
                let mut caps = Vec::new();
                for cap in [
                    xdg_toplevel::WmCapabilities::Maximize,
                    xdg_toplevel::WmCapabilities::Fullscreen,
                ] {
                    caps.extend_from_slice(&(cap as u32).to_ne_bytes());
                }
                t.wm_capabilities(caps);
            }
            x.activated = true;
            if x.wants_maximized || x.wants_fullscreen {
                // The shell answers with the initial configure (research W13).
                let (maximized, fullscreen, limits) =
                    (x.wants_maximized, x.wants_fullscreen, x.pending_limits);
                x.awaiting_shell = true;
                let id = state.new_window_id();
                let parent = parent_window(state, &xdg_id);
                if let Some(x) = state.xdg_surfaces.get_mut(&xdg_id) {
                    x.early_id = Some(id);
                }
                state.window_events.push(WindowEvent::Created {
                    id,
                    parent,
                    limits,
                    maximized,
                    fullscreen,
                });
            } else {
                send_configure(state, &xdg_id, false);
            }
        }
        return;
    }
    match (x.window, has_buffer) {
        (None, true) => map(state, &xdg_id, root),
        (Some(_), false) => unmap(state, &xdg_id),
        (Some(id), true) => {
            let (size, input) = window_shape(state, root);
            if let Some(x) = state.xdg_surfaces.get_mut(&xdg_id) {
                let shape = (size, input, x.limits);
                let configured = configured(&x.cfg);
                let newly_configured = configured && !x.cfg.reported_configured;
                if x.reported.as_ref() != Some(&shape) || newly_configured {
                    x.reported = Some(shape.clone());
                    x.cfg.reported_configured = configured;
                    state.window_events.push(WindowEvent::Updated {
                        id,
                        size: shape.0,
                        input: shape.1,
                        limits: shape.2,
                        configured,
                    });
                }
            }
        }
        (None, false) => {}
    }
}

fn map(state: &mut ServerState, xdg_id: &ObjectId, root: &ObjectId) {
    let early = state
        .xdg_surfaces
        .get_mut(xdg_id)
        .and_then(|x| x.early_id.take());
    let id = early.unwrap_or_else(|| state.new_window_id());
    let (size, input) = window_shape(state, root);
    let parent = parent_window(state, xdg_id);
    let Some(x) = state.xdg_surfaces.get_mut(xdg_id) else {
        return;
    };
    x.window = Some(id);
    let limits = x.limits;
    x.reported = Some((size, input.clone(), limits));
    tracing::info!("window {} opened: {} \"{}\"", id.0, x.app_id, x.title);
    state.windows.insert(id, root.clone());
    state.window_events.push(WindowEvent::Mapped {
        id,
        size,
        input,
        limits,
        parent,
    });
    output::enter(state, root);
    compositor::announce_scale(state, root);
    state.dirty = true;
}

fn unmap(state: &mut ServerState, xdg_id: &ObjectId) {
    let Some(x) = state.xdg_surfaces.get_mut(xdg_id) else {
        return;
    };
    let Some(id) = x.window.take() else {
        // Gone before it mapped: forget a `Created` the shell may still hold.
        if let Some(id) = x.early_id.take() {
            x.awaiting_shell = false;
            state.window_events.push(WindowEvent::Unmapped { id });
        }
        return;
    };
    // The application has to start over with an initial commit (xdg_surface).
    x.reported = None;
    x.configure_sent = false;
    x.acked = false;
    x.serials.clear();
    x.cfg = ConfigureState::default();
    x.wants_maximized = false;
    x.wants_fullscreen = false;
    let parent = x.parent.take();
    tracing::info!("window {} closed", id.0);
    state.windows.remove(&id);
    // Its children get its own parent (xdg_toplevel.set_parent).
    for child in state.xdg_surfaces.values_mut() {
        if child.parent.as_ref() == Some(xdg_id) {
            child.parent.clone_from(&parent);
        }
    }
    state.window_events.push(WindowEvent::Unmapped { id });
    state.dirty = true;
}

/// A `wl_surface` was destroyed: unmap its window, if any.
pub(crate) fn surface_destroyed(state: &mut ServerState, surface: &ObjectId) {
    if let Some(xdg_id) = xdg_id_of_surface(state, surface) {
        unmap(state, &xdg_id);
    }
}

impl GlobalDispatch<XdgWmBase, ()> for ServerState {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<XdgWmBase>,
        _: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<XdgWmBase, ()> for ServerState {
    fn request(
        state: &mut Self,
        client: &Client,
        wm_base: &XdgWmBase,
        request: xdg_wm_base::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            xdg_wm_base::Request::CreatePositioner { id } => {
                data_init.init(id, Mutex::new(PositionerState::default()));
            }
            xdg_wm_base::Request::GetXdgSurface { id, surface } => {
                let resource = data_init.init(id, ());
                let sid = surface.id();
                let Some(s) = state.surfaces.get(&sid) else {
                    return;
                };
                if !matches!(s.role, Role::None | Role::Toplevel | Role::Popup)
                    || xdg_of_surface(state, &sid).is_some()
                {
                    wm_base.post_error(xdg_wm_base::Error::Role, "the surface has another role");
                    return;
                }
                if s.buffer_size.is_some() || matches!(s.pending.attach, Some(Some(_))) {
                    wm_base.post_error(
                        xdg_wm_base::Error::InvalidSurfaceState,
                        "the surface already has a buffer",
                    );
                    return;
                }
                state.xdg_surfaces.insert(
                    resource.id(),
                    XdgSurfaceData {
                        resource,
                        wm_base: wm_base.clone(),
                        surface: sid,
                        role: XdgRole::None,
                        serials: Vec::new(),
                        configure_sent: false,
                        acked: false,
                        pending_geometry: None,
                        geometry: None,
                        window: None,
                        reported: None,
                        pending_limits: SizeLimits::default(),
                        limits: SizeLimits::default(),
                        title: String::new(),
                        app_id: String::new(),
                        activated: false,
                        cfg: ConfigureState::default(),
                        wants_maximized: false,
                        wants_fullscreen: false,
                        early_id: None,
                        awaiting_shell: false,
                        parent: None,
                    },
                );
            }
            xdg_wm_base::Request::Destroy => {
                let client_id = client.id();
                let alive = state
                    .xdg_surfaces
                    .values()
                    .any(|x| x.resource.client().map(|c| c.id()) == Some(client_id.clone()));
                if alive {
                    wm_base.post_error(
                        xdg_wm_base::Error::DefunctSurfaces,
                        "xdg_wm_base destroyed before its surfaces",
                    );
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<XdgPositioner, Mutex<PositionerState>> for ServerState {
    fn request(
        _: &mut Self,
        _: &Client,
        resource: &XdgPositioner,
        request: xdg_positioner::Request,
        data: &Mutex<PositionerState>,
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        let mut p = data.lock().unwrap_or_else(|e| e.into_inner());
        match request {
            xdg_positioner::Request::SetSize { width, height } => {
                if width < 1 || height < 1 {
                    resource.post_error(xdg_positioner::Error::InvalidInput, "size below 1");
                } else {
                    p.size = true;
                }
            }
            xdg_positioner::Request::SetAnchorRect { width, height, .. } => {
                if width < 0 || height < 0 {
                    resource.post_error(xdg_positioner::Error::InvalidInput, "negative anchor");
                } else {
                    p.anchor_rect = true;
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<XdgSurface, ()> for ServerState {
    fn request(
        state: &mut Self,
        _: &Client,
        resource: &XdgSurface,
        request: xdg_surface::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        let xdg_id = resource.id();
        match request {
            xdg_surface::Request::GetToplevel { id } => {
                let toplevel = data_init.init(id, xdg_id.clone());
                let Some(x) = state.xdg_surfaces.get(&xdg_id) else {
                    return;
                };
                if !matches!(x.role, XdgRole::None) {
                    resource.post_error(
                        xdg_surface::Error::AlreadyConstructed,
                        "the xdg_surface already has a role",
                    );
                    return;
                }
                let (surface, wm_base) = (x.surface.clone(), x.wm_base.clone());
                if !compositor::set_role(state, &surface, Role::Toplevel) {
                    wm_base.post_error(xdg_wm_base::Error::Role, "the surface has another role");
                    return;
                }
                if let Some(x) = state.xdg_surfaces.get_mut(&xdg_id) {
                    x.role = XdgRole::Toplevel(toplevel);
                }
            }
            xdg_surface::Request::GetPopup { id, positioner, .. } => {
                let popup = data_init.init(id, ());
                let Some(x) = state.xdg_surfaces.get(&xdg_id) else {
                    return;
                };
                if !matches!(x.role, XdgRole::None) {
                    resource.post_error(
                        xdg_surface::Error::AlreadyConstructed,
                        "the xdg_surface already has a role",
                    );
                    return;
                }
                let (surface, wm_base) = (x.surface.clone(), x.wm_base.clone());
                let complete = positioner
                    .data::<Mutex<PositionerState>>()
                    .map(|m| {
                        let p = m.lock().unwrap_or_else(|e| e.into_inner());
                        p.size && p.anchor_rect
                    })
                    .unwrap_or(false);
                if !complete {
                    wm_base.post_error(
                        xdg_wm_base::Error::InvalidPositioner,
                        "the positioner has no size or anchor rectangle",
                    );
                    return;
                }
                if !compositor::set_role(state, &surface, Role::Popup) {
                    wm_base.post_error(xdg_wm_base::Error::Role, "the surface has another role");
                    return;
                }
                if let Some(x) = state.xdg_surfaces.get_mut(&xdg_id) {
                    x.role = XdgRole::Popup;
                }
                // Popups are not shown in this feature: dismiss it at once (research R9).
                popup.popup_done();
            }
            xdg_surface::Request::SetWindowGeometry {
                x,
                y,
                width,
                height,
            } => {
                if width <= 0 || height <= 0 {
                    resource.post_error(xdg_surface::Error::InvalidSize, "empty window geometry");
                    return;
                }
                if let Some(data) = state.xdg_surfaces.get_mut(&xdg_id) {
                    data.pending_geometry = Some(Rect::new(
                        Point::new(f64::from(x), f64::from(y)),
                        Size::new(f64::from(width), f64::from(height)),
                    ));
                }
            }
            xdg_surface::Request::AckConfigure { serial } => {
                let Some(x) = state.xdg_surfaces.get_mut(&xdg_id) else {
                    return;
                };
                match x.serials.iter().position(|s| *s == serial) {
                    Some(i) => {
                        x.serials.drain(..=i);
                        x.acked = true;
                        acked(state, &xdg_id, serial);
                    }
                    None => resource.post_error(
                        xdg_surface::Error::InvalidSerial,
                        "ack of a serial that was not sent",
                    ),
                }
            }
            xdg_surface::Request::Destroy => {
                if let Some(x) = state.xdg_surfaces.get(&xdg_id)
                    && let XdgRole::Toplevel(t) = &x.role
                    && t.is_alive()
                {
                    resource.post_error(
                        xdg_surface::Error::DefunctRoleObject,
                        "xdg_surface destroyed before its toplevel",
                    );
                }
            }
            _ => {}
        }
    }

    fn destroyed(state: &mut Self, _: ClientId, resource: &XdgSurface, _: &()) {
        let id = resource.id();
        unmap(state, &id);
        state.xdg_surfaces.remove(&id);
    }
}

impl Dispatch<XdgToplevel, ObjectId> for ServerState {
    fn request(
        state: &mut Self,
        _: &Client,
        resource: &XdgToplevel,
        request: xdg_toplevel::Request,
        xdg_id: &ObjectId,
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        let Some(x) = state.xdg_surfaces.get_mut(xdg_id) else {
            return;
        };
        match request {
            xdg_toplevel::Request::SetTitle { title } => x.title = title,
            xdg_toplevel::Request::SetAppId { app_id } => x.app_id = app_id,
            xdg_toplevel::Request::SetMinSize { width, height }
            | xdg_toplevel::Request::SetMaxSize { width, height }
                if width < 0 || height < 0 =>
            {
                resource.post_error(xdg_toplevel::Error::InvalidSize, "negative size");
            }
            xdg_toplevel::Request::SetMaximized | xdg_toplevel::Request::UnsetMaximized => {
                let on = matches!(request, xdg_toplevel::Request::SetMaximized);
                x.wants_maximized = on;
                if let Some(id) = x.window {
                    state
                        .window_events
                        .push(WindowEvent::MaximizeRequested { id, on });
                }
            }
            // The output is ignored: there is one viewer.
            xdg_toplevel::Request::SetFullscreen { .. }
            | xdg_toplevel::Request::UnsetFullscreen => {
                let on = matches!(request, xdg_toplevel::Request::SetFullscreen { .. });
                x.wants_fullscreen = on;
                if let Some(id) = x.window {
                    state
                        .window_events
                        .push(WindowEvent::FullscreenRequested { id, on });
                }
            }
            xdg_toplevel::Request::SetParent { parent } => {
                set_parent(state, resource, xdg_id, parent);
            }
            xdg_toplevel::Request::Move { serial, .. } => {
                if let Some(id) = x.window
                    && state.seat.last_press == Some((id, serial))
                {
                    state.window_events.push(WindowEvent::MoveRequested { id });
                }
            }
            xdg_toplevel::Request::Resize { serial, edges, .. } => {
                let edges = match edges {
                    WEnum::Value(e) => Edges::from_resize_edge(e as u32),
                    WEnum::Unknown(v) => Edges::from_resize_edge(v),
                };
                let Some(edges) = edges else {
                    resource.post_error(
                        xdg_toplevel::Error::InvalidResizeEdge,
                        "not a resize_edge value",
                    );
                    return;
                };
                if edges.is_valid()
                    && let Some(id) = x.window
                    && state.seat.last_press == Some((id, serial))
                {
                    state
                        .window_events
                        .push(WindowEvent::ResizeRequested { id, edges });
                }
            }
            // Both are non-negative here; i32 -> f64 is exact.
            xdg_toplevel::Request::SetMinSize { width, height } => {
                x.pending_limits.min = Size::new(f64::from(width), f64::from(height));
            }
            xdg_toplevel::Request::SetMaxSize { width, height } => {
                x.pending_limits.max = Size::new(f64::from(width), f64::from(height));
            }
            // Minimize and the window menu: nothing to minimize to, no menu (spec Edge Cases).
            _ => {}
        }
    }

    fn destroyed(state: &mut Self, _: ClientId, _: &XdgToplevel, xdg_id: &ObjectId) {
        unmap(state, xdg_id);
        if let Some(x) = state.xdg_surfaces.get_mut(xdg_id) {
            x.role = XdgRole::None;
        }
    }
}

impl Dispatch<XdgPopup, ()> for ServerState {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &XdgPopup,
        _: xdg_popup::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
    }
}
