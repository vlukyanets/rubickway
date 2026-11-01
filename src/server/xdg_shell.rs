//! `xdg_wm_base`, `xdg_surface`, `xdg_toplevel`, `xdg_popup`, `xdg_positioner` (research R9).
//!
//! A toplevel maps when its surface commits a buffer after acking the first configure; the
//! server then emits `WindowEvent::Mapped` and, on later changes, `Updated` / `Unmapped`.
//! Window-management requests are accepted and ignored; popups are dismissed at once.

use std::sync::Mutex;

use wayland_protocols::xdg::shell::server::xdg_popup::{self, XdgPopup};
use wayland_protocols::xdg::shell::server::xdg_positioner::{self, XdgPositioner};
use wayland_protocols::xdg::shell::server::xdg_surface::{self, XdgSurface};
use wayland_protocols::xdg::shell::server::xdg_toplevel::{self, XdgToplevel};
use wayland_protocols::xdg::shell::server::xdg_wm_base::{self, XdgWmBase};
use wayland_server::backend::{ClientId, ObjectId};
use wayland_server::{Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource};

use super::compositor::{self, Role};
use super::{ServerState, output};
use crate::canvas::{Point, Rect, Size, Vector};
use crate::shell::{WindowEvent, WindowId};

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
    /// Size and input shape last reported to the shell.
    reported: Option<(Size, Vec<Rect>)>,
    title: String,
    app_id: String,
    pub activated: bool,
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

/// Sends a configure with size 0x0 (the application chooses) and the `activated` state.
fn send_configure(state: &mut ServerState, xdg_id: &ObjectId) {
    let serial = state.next_serial();
    let Some(x) = state.xdg_surfaces.get_mut(xdg_id) else {
        return;
    };
    let XdgRole::Toplevel(toplevel) = &x.role else {
        return;
    };
    let mut states = Vec::new();
    if x.activated {
        states.extend_from_slice(&(xdg_toplevel::State::Activated as u32).to_ne_bytes());
    }
    toplevel.configure(0, 0, states);
    x.resource.configure(serial);
    x.serials.push(serial);
    x.configure_sent = true;
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
        send_configure(state, &xdg_id);
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
        if !x.configure_sent {
            if x.resource.version() >= 5
                && let XdgRole::Toplevel(t) = &x.role
            {
                t.wm_capabilities(Vec::new());
            }
            x.activated = true;
            send_configure(state, &xdg_id);
        }
        return;
    }
    match (x.window, has_buffer) {
        (None, true) => map(state, &xdg_id, root),
        (Some(_), false) => unmap(state, &xdg_id),
        (Some(id), true) => {
            let shape = window_shape(state, root);
            if let Some(x) = state.xdg_surfaces.get_mut(&xdg_id)
                && x.reported.as_ref() != Some(&shape)
            {
                x.reported = Some(shape.clone());
                state.window_events.push(WindowEvent::Updated {
                    id,
                    size: shape.0,
                    input: shape.1,
                });
            }
        }
        (None, false) => {}
    }
}

fn map(state: &mut ServerState, xdg_id: &ObjectId, root: &ObjectId) {
    let id = state.new_window_id();
    let shape = window_shape(state, root);
    let Some(x) = state.xdg_surfaces.get_mut(xdg_id) else {
        return;
    };
    x.window = Some(id);
    x.reported = Some(shape.clone());
    tracing::info!("window {} opened: {} \"{}\"", id.0, x.app_id, x.title);
    state.windows.insert(id, root.clone());
    state.window_events.push(WindowEvent::Mapped {
        id,
        size: shape.0,
        input: shape.1,
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
        return;
    };
    // The application has to start over with an initial commit (xdg_surface).
    x.reported = None;
    x.configure_sent = false;
    x.acked = false;
    x.serials.clear();
    tracing::info!("window {} closed", id.0);
    state.windows.remove(&id);
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
                        title: String::new(),
                        app_id: String::new(),
                        activated: false,
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
            // Window management is not part of this feature: move, resize, maximize,
            // fullscreen, minimize, window menu and parent are accepted and ignored.
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
