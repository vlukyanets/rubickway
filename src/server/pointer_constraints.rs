//! `zwp_pointer_constraints_v1` version 1: applications lock the pointer or confine it to an
//! area of their window (pointer-lock contracts/protocols.md, research L2, L3, L12).
//!
//! The server keeps the protocol objects and their double-buffered state and reports each
//! window's constraint to the shell; the shell decides when one applies and says so with
//! `ClientInput::Constraint`.

use std::collections::HashMap;

use wayland_protocols::wp::pointer_constraints::zv1::server::zwp_confined_pointer_v1::{
    self, ZwpConfinedPointerV1,
};
use wayland_protocols::wp::pointer_constraints::zv1::server::zwp_locked_pointer_v1::{
    self, ZwpLockedPointerV1,
};
use wayland_protocols::wp::pointer_constraints::zv1::server::zwp_pointer_constraints_v1::{
    self, Lifetime, ZwpPointerConstraintsV1,
};
use wayland_server::backend::{ClientId, ObjectId};
use wayland_server::protocol::wl_region::WlRegion;
use wayland_server::{
    Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource, WEnum,
};

use super::compositor::Region;
use super::{ServerState, describe_application, xdg_shell};
use crate::canvas::{Point, Rect, Size, Vector};
use crate::shell::{
    ConstraintChange, ConstraintEnd, ConstraintInfo, ConstraintKind, ConstraintToken, WindowEvent,
    WindowId,
};

/// The version implemented and tested (Principle VI).
const VERSION: u32 = 1;

pub(crate) fn create_global(dh: &DisplayHandle) {
    dh.create_global::<ServerState, ZwpPointerConstraintsV1, ()>(VERSION, ());
}

/// The protocol object of a constraint.
#[derive(Debug, Clone)]
pub(crate) enum ConstraintResource {
    Lock(ZwpLockedPointerV1),
    Confine(ZwpConfinedPointerV1),
}

/// What a constraint was last reported against: the window and its geometry.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Reported {
    window: WindowId,
    origin: Point,
    size: Size,
}

/// One live constraint object (pointer-lock data-model "ConstraintData"). A defunct one is
/// removed, so its requests and the shell's decisions for it find nothing.
#[derive(Debug)]
pub(crate) struct ConstraintData {
    resource: ConstraintResource,
    surface: ObjectId,
    kind: ConstraintKind,
    /// `false`: `oneshot`.
    persistent: bool,
    /// `Some(None)`: the whole surface from the next commit.
    pending_area: Option<Option<Region>>,
    /// Surface coordinates; `None`: the whole surface.
    area: Option<Region>,
    pending_hint: Option<Point>,
    /// Surface coordinates.
    hint: Option<Point>,
    /// Between the shell's `Start` and `Stop`.
    applying: bool,
    reported: Option<Reported>,
    /// The area or hint changed since the last report.
    dirty: bool,
}

/// All live constraint objects.
#[derive(Debug, Default)]
pub(crate) struct ConstraintsState {
    by_token: HashMap<ConstraintToken, ConstraintData>,
    /// The live constraint of each surface, for `already_constrained`.
    by_surface: HashMap<ObjectId, ConstraintToken>,
    next_token: u64,
}

impl ConstraintsState {
    fn new_token(&mut self) -> ConstraintToken {
        self.next_token += 1;
        ConstraintToken(self.next_token)
    }
}

/// Why a constraint stopped applying, for the log (FR-015, research L13).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stopped {
    /// The shell's decision.
    Shell(ConstraintEnd),
    /// The application destroyed it while it applied.
    Application,
    /// Its surface or its application went away while it applied.
    WindowGone,
}

/// The debug log line for a constraint of `app` starting (`stop: None`) or stopping. Names the
/// application, the kind and the lifetime, and why it stopped; never coordinates (FR-015).
fn log_line(app: &str, kind: ConstraintKind, persistent: bool, stop: Option<Stopped>) -> String {
    let verb = match (kind, stop.is_some()) {
        (ConstraintKind::Lock, false) => "locked",
        (ConstraintKind::Lock, true) => "unlocked",
        (ConstraintKind::Confine, false) => "confined",
        (ConstraintKind::Confine, true) => "unconfined",
    };
    let lifetime = if persistent { "lasting" } else { "single-use" };
    let line = format!("pointer {verb} for {app}, {lifetime}");
    let Some(why) = stop else {
        return line;
    };
    let why = match why {
        Stopped::Application => "application",
        Stopped::WindowGone | Stopped::Shell(ConstraintEnd::WindowGone) => "window gone",
        Stopped::Shell(ConstraintEnd::Shortcut) => "release shortcut",
        Stopped::Shell(ConstraintEnd::Focus) => "focus",
        Stopped::Shell(ConstraintEnd::PointerLeft) => "pointer left",
        Stopped::Shell(ConstraintEnd::Resize) => "resize",
    };
    format!("{line}: {why}")
}

/// Writes the debug line for `d` starting or stopping.
fn log(d: &ConstraintData, stop: Option<Stopped>) {
    let client = match &d.resource {
        ConstraintResource::Lock(r) => r.client(),
        ConstraintResource::Confine(r) => r.client(),
    };
    let app = client.map_or_else(
        || "a gone application".to_string(),
        |c| describe_application(&c),
    );
    tracing::debug!("{}", log_line(&app, d.kind, d.persistent, stop));
}

/// Whether a constraint with `lifetime` is lasting. A value the protocol does not define counts
/// as `oneshot`, the stricter choice (contracts/protocols.md).
fn persistent(lifetime: WEnum<Lifetime>) -> bool {
    matches!(lifetime, WEnum::Value(Lifetime::Persistent))
}

/// A copy of `region`'s rectangles, or `None` (the whole surface) without one.
fn copy_region(state: &ServerState, region: Option<&WlRegion>) -> Option<Region> {
    region.map(|r| state.regions.get(&r.id()).cloned().unwrap_or_default())
}

/// The window whose root surface is `surface`, if it is shown.
fn window_of(state: &ServerState, surface: &ObjectId) -> Option<WindowId> {
    state
        .windows
        .iter()
        .find(|(_, root)| *root == surface)
        .map(|(id, _)| *id)
}

/// Reports `token`'s constraint to the shell if its window is shown and anything reported
/// changed: its area, its hint, the window, or the window geometry (research L12).
fn report(state: &mut ServerState, token: ConstraintToken) {
    let Some(surface) = state
        .constraints
        .by_token
        .get(&token)
        .map(|d| d.surface.clone())
    else {
        return;
    };
    let Some(window) = window_of(state, &surface) else {
        return;
    };
    let geometry = xdg_shell::geometry_of(state, &surface);
    let now = Reported {
        window,
        origin: geometry.origin,
        size: geometry.size,
    };
    let Some(d) = state.constraints.by_token.get_mut(&token) else {
        return;
    };
    if !d.dirty && d.reported == Some(now) {
        return;
    }
    d.dirty = false;
    d.reported = Some(now);
    // Window coordinates start at the window geometry's origin.
    let shift = Point::new(0.0, 0.0) - geometry.origin;
    let bounds = Rect::new(Point::new(0.0, 0.0), geometry.size);
    let info = ConstraintInfo {
        token,
        kind: d.kind,
        persistent: d.persistent,
        area: d
            .area
            .as_ref()
            .map(|rects| rects.iter().map(|r| r.translate(shift)).collect()),
        hint: d.hint.map(|h| h + shift).filter(|h| bounds.contains(*h)),
    };
    state.window_events.push(WindowEvent::ConstraintChanged {
        id: window,
        constraint: Some(info),
    });
}

/// The root surface of a window committed: its constraint's pending area and release point
/// apply (double-buffered, contracts/protocols.md).
pub(crate) fn apply_pending(state: &mut ServerState, root: &ObjectId) {
    let Some(token) = state.constraints.by_surface.get(root).copied() else {
        return;
    };
    if let Some(d) = state.constraints.by_token.get_mut(&token) {
        if let Some(area) = d.pending_area.take() {
            d.area = area;
            d.dirty = true;
        }
        if let Some(hint) = d.pending_hint.take() {
            d.hint = Some(hint);
            d.dirty = true;
        }
    }
}

/// After a commit of the tree at `root`: report its constraint if anything changed, also when
/// the window was just shown or its geometry moved (research L12).
pub(crate) fn report_surface(state: &mut ServerState, root: &ObjectId) {
    if let Some(token) = state.constraints.by_surface.get(root).copied() {
        report(state, token);
    }
}

/// A surface was destroyed: its constraint stops applying and its requests are ignored from now
/// on (misbehaving case M5).
pub(crate) fn surface_destroyed(state: &mut ServerState, surface: &ObjectId) {
    if let Some(token) = state.constraints.by_surface.remove(surface)
        && let Some(d) = state.constraints.by_token.remove(&token)
        && d.applying
    {
        log(&d, Some(Stopped::WindowGone));
    }
}

/// The shell starts or stops applying `token`'s constraint (research L5): `locked`/`confined`
/// or `unlocked`/`unconfined`; a stopped `oneshot` constraint is defunct. A decision for a
/// token that is gone is ignored (research L3).
pub(crate) fn decide(state: &mut ServerState, token: ConstraintToken, change: ConstraintChange) {
    let Some(d) = state.constraints.by_token.get_mut(&token) else {
        return;
    };
    match change {
        ConstraintChange::Start => {
            if d.applying {
                return;
            }
            d.applying = true;
            match &d.resource {
                ConstraintResource::Lock(r) => r.locked(),
                ConstraintResource::Confine(r) => r.confined(),
            }
            log(d, None);
        }
        ConstraintChange::Stop(why) => {
            if !d.applying {
                return;
            }
            d.applying = false;
            match &d.resource {
                ConstraintResource::Lock(r) => r.unlocked(),
                ConstraintResource::Confine(r) => r.unconfined(),
            }
            log(d, Some(Stopped::Shell(why)));
            if !d.persistent {
                let surface = d.surface.clone();
                state.constraints.by_token.remove(&token);
                if state.constraints.by_surface.get(&surface) == Some(&token) {
                    state.constraints.by_surface.remove(&surface);
                }
            }
        }
    }
}

/// The application destroyed `token`'s object: the shell hears it is gone.
fn destroyed(state: &mut ServerState, token: ConstraintToken) {
    let Some(d) = state.constraints.by_token.remove(&token) else {
        return;
    };
    if state.constraints.by_surface.get(&d.surface) == Some(&token) {
        state.constraints.by_surface.remove(&d.surface);
    }
    if d.applying {
        // No decision follows: the shell's would name a gone token (research L13).
        log(&d, Some(Stopped::Application));
    }
    if let Some(reported) = d.reported
        && state.windows.contains_key(&reported.window)
    {
        state.window_events.push(WindowEvent::ConstraintChanged {
            id: reported.window,
            constraint: None,
        });
    }
}

impl GlobalDispatch<ZwpPointerConstraintsV1, ()> for ServerState {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<ZwpPointerConstraintsV1>,
        _: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<ZwpPointerConstraintsV1, ()> for ServerState {
    fn request(
        state: &mut Self,
        _: &Client,
        manager: &ZwpPointerConstraintsV1,
        request: zwp_pointer_constraints_v1::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        // `destroy` ends only the manager; its constraints keep working.
        let token = state.constraints.new_token();
        let (kind, surface, region, lifetime, resource) = match request {
            zwp_pointer_constraints_v1::Request::LockPointer {
                id,
                surface,
                region,
                lifetime,
                ..
            } => {
                let lock = data_init.init(id, token);
                (
                    ConstraintKind::Lock,
                    surface,
                    region,
                    lifetime,
                    ConstraintResource::Lock(lock),
                )
            }
            zwp_pointer_constraints_v1::Request::ConfinePointer {
                id,
                surface,
                region,
                lifetime,
                ..
            } => {
                let confine = data_init.init(id, token);
                (
                    ConstraintKind::Confine,
                    surface,
                    region,
                    lifetime,
                    ConstraintResource::Confine(confine),
                )
            }
            _ => return,
        };
        let surface = surface.id();
        if state.constraints.by_surface.contains_key(&surface) {
            manager.post_error(
                zwp_pointer_constraints_v1::Error::AlreadyConstrained,
                "the surface already has a lock or confinement",
            );
            return;
        }
        let area = copy_region(state, region.as_ref());
        state.constraints.by_token.insert(
            token,
            ConstraintData {
                resource,
                surface: surface.clone(),
                kind,
                persistent: persistent(lifetime),
                pending_area: None,
                area,
                pending_hint: None,
                hint: None,
                applying: false,
                reported: None,
                dirty: true,
            },
        );
        state.constraints.by_surface.insert(surface, token);
        report(state, token);
    }
}

/// `set_region` on a lock or confinement: the area from the next commit.
fn set_region(state: &mut ServerState, token: ConstraintToken, region: Option<&WlRegion>) {
    let area = copy_region(state, region);
    if let Some(d) = state.constraints.by_token.get_mut(&token) {
        d.pending_area = Some(area);
    }
}

impl Dispatch<ZwpLockedPointerV1, ConstraintToken> for ServerState {
    fn request(
        state: &mut Self,
        _: &Client,
        _: &ZwpLockedPointerV1,
        request: zwp_locked_pointer_v1::Request,
        token: &ConstraintToken,
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        match request {
            zwp_locked_pointer_v1::Request::SetCursorPositionHint {
                surface_x,
                surface_y,
            } => {
                if let Some(d) = state.constraints.by_token.get_mut(token) {
                    d.pending_hint = Some(Point::new(0.0, 0.0) + Vector::new(surface_x, surface_y));
                }
            }
            zwp_locked_pointer_v1::Request::SetRegion { region } => {
                set_region(state, *token, region.as_ref());
            }
            _ => {}
        }
    }

    fn destroyed(state: &mut Self, _: ClientId, _: &ZwpLockedPointerV1, token: &ConstraintToken) {
        destroyed(state, *token);
    }
}

impl Dispatch<ZwpConfinedPointerV1, ConstraintToken> for ServerState {
    fn request(
        state: &mut Self,
        _: &Client,
        _: &ZwpConfinedPointerV1,
        request: zwp_confined_pointer_v1::Request,
        token: &ConstraintToken,
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        if let zwp_confined_pointer_v1::Request::SetRegion { region } = request {
            set_region(state, *token, region.as_ref());
        }
    }

    fn destroyed(state: &mut Self, _: ClientId, _: &ZwpConfinedPointerV1, token: &ConstraintToken) {
        destroyed(state, *token);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_persistent_is_lasting_and_unknown_values_are_single_use() {
        assert!(persistent(WEnum::Value(Lifetime::Persistent)));
        assert!(!persistent(WEnum::Value(Lifetime::Oneshot)));
        assert!(!persistent(WEnum::Unknown(7)));
    }

    const APP: &str = "application 3 (pid 4321, game)";

    #[test]
    fn a_log_line_names_the_application_the_kind_and_the_lifetime() {
        assert_eq!(
            log_line(APP, ConstraintKind::Lock, true, None),
            "pointer locked for application 3 (pid 4321, game), lasting"
        );
        assert_eq!(
            log_line(APP, ConstraintKind::Confine, false, None),
            "pointer confined for application 3 (pid 4321, game), single-use"
        );
    }

    #[test]
    fn a_stop_line_names_why_and_no_coordinates() {
        let reasons = [
            (Stopped::Application, "application"),
            (Stopped::Shell(ConstraintEnd::Shortcut), "release shortcut"),
            (Stopped::Shell(ConstraintEnd::Focus), "focus"),
            (Stopped::Shell(ConstraintEnd::PointerLeft), "pointer left"),
            (Stopped::Shell(ConstraintEnd::WindowGone), "window gone"),
            (Stopped::WindowGone, "window gone"),
            (Stopped::Shell(ConstraintEnd::Resize), "resize"),
        ];
        for (why, text) in reasons {
            let line = log_line(APP, ConstraintKind::Lock, false, Some(why));
            assert_eq!(
                line,
                format!("pointer unlocked for application 3 (pid 4321, game), single-use: {text}")
            );
            assert!(!line.contains('.'), "no coordinates: {line}");
        }
        assert!(
            log_line(
                APP,
                ConstraintKind::Confine,
                true,
                Some(Stopped::Application)
            )
            .starts_with("pointer unconfined for")
        );
    }
}
