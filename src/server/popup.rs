//! `xdg_popup`: popups as part of their window (popups research P5, P6, P9, P12).
//!
//! A popup asks the shell where to go at its initial commit (`PopupRequested`); the shell answers
//! with a placement, sent as `xdg_popup.configure` + `xdg_surface.configure`, which applies at the
//! commit after the ack. A shown popup is drawn, hit-tested and given frame callbacks as part of
//! its window: a window is a list of layers, the toplevel's surface tree first, then each shown
//! popup's tree in creation order, every one with its origin in window coordinates. A popup that
//! is dismissed, hidden (a commit without a buffer) or destroyed is never shown again.

use wayland_protocols::xdg::shell::server::xdg_popup::{self, XdgPopup};
use wayland_protocols::xdg::shell::server::xdg_positioner::XdgPositioner;
use wayland_protocols::xdg::shell::server::xdg_surface;
use wayland_protocols::xdg::shell::server::xdg_wm_base::{self, XdgWmBase};
use wayland_server::backend::{ClientId, ObjectId};
use wayland_server::{Client, DataInit, Dispatch, DisplayHandle, Resource};

use super::{ServerState, compositor, output, seat, xdg_shell};
use crate::canvas::{Point, Rect, Size};
use crate::shell::{IRect, PopupId, PopupRules, WindowEvent, WindowId};

/// Whether a popup asked for a grab, and whether its serial passed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GrabState {
    None,
    Asked,
    /// A wrong serial or a dismissed grabbing parent: dismissed at the initial commit.
    Refused,
}

/// One `xdg_popup` (popups data-model "PopupData").
pub(crate) struct PopupData {
    resource: XdgPopup,
    wm_base: XdgWmBase,
    /// The `wl_surface`.
    surface: ObjectId,
    /// The parent's `xdg_surface` (a toplevel or another popup).
    parent: Option<ObjectId>,
    /// Given at the initial commit.
    id: Option<PopupId>,
    /// The toplevel window at the root of the parent chain, once known.
    window: Option<WindowId>,
    rules: PopupRules,
    grab: GrabState,
    committed: bool,
    /// Configure serials sent, with their placement.
    pending: Vec<(u32, IRect)>,
    /// The acked placement, applied at the next commit.
    acked: Option<IRect>,
    /// The placement in effect.
    at: Option<IRect>,
    /// `popup_done` sent, or hidden or destroyed by the application: never shown again.
    dismissed: bool,
    shown: bool,
    /// Position, size and input shape the shell was told last.
    reported: Option<((i32, i32), Size, Vec<Rect>)>,
    /// A reposition waiting to be reported: its token and rules; a newer one replaces it
    /// (research P11).
    reposition: Option<(u32, PopupRules)>,
}

/// A new `xdg_popup` on `xdg_id` (its `xdg_surface`), with a copy of the positioner's rules.
pub(crate) fn create(
    state: &mut ServerState,
    xdg_id: ObjectId,
    resource: XdgPopup,
    wm_base: XdgWmBase,
    surface: ObjectId,
    parent: Option<ObjectId>,
    rules: PopupRules,
) {
    state.popups.insert(
        xdg_id,
        PopupData {
            resource,
            wm_base,
            surface,
            parent,
            id: None,
            window: None,
            rules,
            grab: GrabState::None,
            committed: false,
            pending: Vec::new(),
            acked: None,
            at: None,
            dismissed: false,
            shown: false,
            reported: None,
            reposition: None,
        },
    );
}

fn visible(p: &PopupData) -> bool {
    p.shown && !p.dismissed
}

/// The canvas-free position of a popup's window geometry in its window's coordinates: the sum
/// of the placements along its parent chain. `None` while a popup on the chain has none.
fn position(state: &ServerState, xdg_id: &ObjectId) -> Option<Point> {
    let (mut x, mut y) = (0.0, 0.0);
    let mut current = xdg_id.clone();
    // The chain is acyclic (a parent exists before its child); the bound is for safety.
    for _ in 0..=state.popups.len() {
        let Some(p) = state.popups.get(&current) else {
            // The toplevel: window coordinates start at its window geometry.
            return Some(Point::new(x, y));
        };
        let at = p.at?;
        x += f64::from(at.x);
        y += f64::from(at.y);
        current = p.parent.clone()?;
    }
    None
}

/// The root surfaces of `window`, back to front, with their origins in window coordinates: the
/// toplevel's tree, then its shown popups in creation order. Empty for unknown windows.
pub(crate) fn layers(state: &ServerState, window: WindowId) -> Vec<(ObjectId, Point)> {
    let Some(root) = state.windows.get(&window) else {
        return Vec::new();
    };
    let geometry = xdg_shell::geometry_of(state, root);
    let mut out = vec![(
        root.clone(),
        Point::new(-geometry.origin.x, -geometry.origin.y),
    )];
    let mut popups: Vec<(PopupId, &ObjectId, &PopupData)> = state
        .popups
        .iter()
        .filter(|(_, p)| visible(p) && p.window == Some(window))
        .filter_map(|(xdg, p)| p.id.map(|id| (id, xdg, p)))
        .collect();
    popups.sort_by_key(|(id, _, _)| *id);
    for (_, xdg, p) in popups {
        if let Some(at) = position(state, xdg) {
            let g = xdg_shell::geometry_of(state, &p.surface);
            out.push((
                p.surface.clone(),
                Point::new(at.x - g.origin.x, at.y - g.origin.y),
            ));
        }
    }
    out
}

/// Every surface of `window`: the trees of all its layers.
pub(crate) fn window_surfaces(state: &ServerState, window: WindowId) -> Vec<ObjectId> {
    layers(state, window)
        .iter()
        .flat_map(|(root, _)| compositor::tree(state, root))
        .collect()
}

/// The window whose layers contain `surface`, if any.
pub(crate) fn window_of_surface(state: &ServerState, surface: &ObjectId) -> Option<WindowId> {
    state
        .windows
        .keys()
        .copied()
        .find(|w| window_surfaces(state, *w).contains(surface))
}

/// Whether `window` has a shown grabbing popup: the pointer then moves between its surfaces even
/// with a button held (research P6).
pub(crate) fn has_grab(state: &ServerState, window: WindowId) -> bool {
    state
        .popups
        .values()
        .any(|p| visible(p) && p.window == Some(window) && p.grab == GrabState::Asked)
}

/// The surface of shown popup `id` of `window`.
pub(crate) fn surface_of(state: &ServerState, window: WindowId, id: PopupId) -> Option<ObjectId> {
    state
        .popups
        .values()
        .find(|p| visible(p) && p.id == Some(id) && p.window == Some(window))
        .map(|p| p.surface.clone())
}

fn xdg_of_id(state: &ServerState, id: PopupId) -> Option<ObjectId> {
    state
        .popups
        .iter()
        .find(|(_, p)| p.id == Some(id))
        .map(|(xdg, _)| xdg.clone())
}

/// `xdg_id` and every popup above it, the deepest first (children before parents).
fn subtree_top_down(state: &ServerState, xdg_id: &ObjectId) -> Vec<ObjectId> {
    let mut all = vec![xdg_id.clone()];
    let mut i = 0;
    while i < all.len() {
        let parent = all[i].clone();
        all.extend(
            state
                .popups
                .iter()
                .filter(|(_, p)| p.parent.as_ref() == Some(&parent) && !p.dismissed)
                .map(|(xdg, _)| xdg.clone()),
        );
        i += 1;
    }
    all.reverse();
    all
}

/// Ends `xdg_id` and the popups above it, children first: `popup_done`, never shown again.
/// `report`: tell the shell (`PopupUnmapped`) of each popup it knows; not when the shell asked.
fn end(state: &mut ServerState, xdg_id: &ObjectId, report: bool) {
    for xdg in subtree_top_down(state, xdg_id) {
        let Some(p) = state.popups.get(&xdg) else {
            continue;
        };
        if p.dismissed {
            continue;
        }
        let (shown, surface) = (p.shown, p.surface.clone());
        if shown {
            // While it still counts as part of its window.
            output::cross_subtree(state, &surface, false);
            state.dirty = true;
        }
        let Some(p) = state.popups.get_mut(&xdg) else {
            continue;
        };
        p.dismissed = true;
        p.shown = false;
        p.reposition = None;
        if p.resource.is_alive() {
            p.resource.popup_done();
        }
        if report && let Some(id) = p.id {
            state.window_events.push(WindowEvent::PopupUnmapped { id });
        }
    }
}

/// The toplevel `xdg_id` unmapped or was destroyed: each of its popups gets `popup_done`, the
/// newest chain first and each chain innermost first. The shell drops them with the window
/// (research P13), so nothing is reported.
pub(crate) fn window_gone(state: &mut ServerState, xdg_id: &ObjectId) {
    let mut roots: Vec<(Option<PopupId>, ObjectId)> = state
        .popups
        .iter()
        .filter(|(_, p)| p.parent.as_ref() == Some(xdg_id) && !p.dismissed)
        .map(|(xdg, p)| (p.id, xdg.clone()))
        .collect();
    roots.sort_by_key(|(id, _)| std::cmp::Reverse(*id));
    for (_, root) in roots {
        end(state, &root, false);
    }
}

/// The shell dismissed popup `id` (`WindowCommand::DismissPopup`).
pub(crate) fn dismiss(state: &mut ServerState, id: PopupId) {
    if let Some(xdg) = xdg_of_id(state, id) {
        end(state, &xdg, false);
    }
}

/// The shell placed popup `id` at `at`, answering reposition `token` if any.
pub(crate) fn place(state: &mut ServerState, id: PopupId, at: IRect, token: Option<u32>) {
    let Some(xdg) = xdg_of_id(state, id) else {
        return;
    };
    if state.popups.get(&xdg).is_none_or(|p| p.dismissed) {
        return;
    }
    let serial = state.next_serial();
    let Some(p) = state.popups.get_mut(&xdg) else {
        return;
    };
    if let Some(token) = token
        && p.resource.version() >= 3
    {
        p.resource.repositioned(token);
    }
    p.resource.configure(at.x, at.y, at.w, at.h);
    p.pending.push((serial, at));
    xdg_shell::send_popup_configure(state, &xdg, serial);
}

/// The application acked configure `serial` of `xdg_id`: its placement applies at the next
/// commit.
pub(crate) fn acked(state: &mut ServerState, xdg_id: &ObjectId, serial: u32) {
    let Some(p) = state.popups.get_mut(xdg_id) else {
        return;
    };
    if let Some(i) = p.pending.iter().position(|(s, _)| *s == serial) {
        p.acked = Some(p.pending[i].1);
        p.pending.drain(..=i);
    }
}

/// The parent's window and popup id, if the parent is shown: a mapped toplevel or a shown popup.
fn parent_of(state: &ServerState, parent: &ObjectId) -> Option<(WindowId, Option<PopupId>)> {
    match state.popups.get(parent) {
        Some(pp) if visible(pp) => Some((pp.window?, pp.id)),
        Some(_) => None,
        None => Some((state.xdg_surfaces.get(parent)?.window?, None)),
    }
}

/// A commit of the popup's surface tree rooted at `root`, its `wl_surface`.
pub(crate) fn committed(
    state: &mut ServerState,
    xdg_id: &ObjectId,
    root: &ObjectId,
    has_buffer: bool,
) {
    let Some(p) = state.popups.get(xdg_id) else {
        return;
    };
    if p.dismissed {
        return;
    }
    if !p.committed {
        initial_commit(state, xdg_id, has_buffer);
        return;
    }
    if !has_buffer {
        if p.shown {
            // Hiding a popup ends it (research P12).
            end(state, xdg_id, true);
        }
        return;
    }
    if p.at.is_none() && p.acked.is_none() {
        unconfigured(state, xdg_id);
        return;
    }
    if !p.shown && p.grab == GrabState::Asked && !parent_is_topmost(state, xdg_id) {
        p.wm_base.post_error(
            xdg_wm_base::Error::NotTheTopmostPopup,
            "a grabbing popup mapped while its parent is not the topmost grabbing popup",
        );
        return;
    }
    let (size, input) = xdg_shell::window_shape(state, root);
    let Some(p) = state.popups.get_mut(xdg_id) else {
        return;
    };
    let moved = match p.acked.take() {
        Some(a) => {
            let moved = p.at != Some(a);
            p.at = Some(a);
            moved
        }
        None => false,
    };
    let (Some(id), Some(at)) = (p.id, p.at) else {
        return;
    };
    let shape = ((at.x, at.y), size, input);
    if !p.shown {
        p.shown = true;
        p.reported = Some(shape.clone());
        state.window_events.push(WindowEvent::PopupMapped {
            id,
            at: shape.0,
            size: shape.1,
            input: shape.2,
        });
        state.dirty = true;
        compositor::announce_scale(state, root);
        output::cross_subtree(state, root, true);
    } else if p.reported.as_ref() != Some(&shape) {
        p.reported = Some(shape.clone());
        state.window_events.push(WindowEvent::PopupUpdated {
            id,
            at: shape.0,
            size: shape.1,
            input: shape.2,
        });
        if moved {
            state.dirty = true;
        }
    }
}

fn unconfigured(state: &ServerState, xdg_id: &ObjectId) {
    if let Some(x) = state.xdg_surfaces.get(xdg_id) {
        x.resource.post_error(
            xdg_surface::Error::UnconfiguredBuffer,
            "a buffer was committed before the first configure was acked",
        );
    }
}

/// A grabbing popup whose parent is a popup may map only while no other shown grabbing popup has
/// the same parent: its parent is then the top of the chain.
fn parent_is_topmost(state: &ServerState, xdg_id: &ObjectId) -> bool {
    let Some(parent) = state.popups.get(xdg_id).and_then(|p| p.parent.clone()) else {
        return true;
    };
    if !state.popups.contains_key(&parent) {
        return true;
    }
    !state.popups.iter().any(|(xdg, p)| {
        xdg != xdg_id
            && visible(p)
            && p.grab == GrabState::Asked
            && p.parent.as_ref() == Some(&parent)
    })
}

fn initial_commit(state: &mut ServerState, xdg_id: &ObjectId, has_buffer: bool) {
    if has_buffer {
        unconfigured(state, xdg_id);
        return;
    }
    let Some(p) = state.popups.get_mut(xdg_id) else {
        return;
    };
    p.committed = true;
    let (grab, parent, rules) = (p.grab, p.parent.clone(), p.rules);
    // No parent (no other protocol can set one), a parent that is not shown, or a refused grab:
    // dismissed at once, no error (research P6, P12).
    let target = parent.as_ref().and_then(|parent| parent_of(state, parent));
    let Some((window, parent_id)) = target.filter(|_| grab != GrabState::Refused) else {
        end(state, xdg_id, false);
        return;
    };
    let id = state.new_popup_id();
    if let Some(p) = state.popups.get_mut(xdg_id) {
        p.id = Some(id);
        p.window = Some(window);
    }
    state.window_events.push(WindowEvent::PopupRequested {
        id,
        window,
        parent: parent_id,
        rules,
        grab: grab == GrabState::Asked,
    });
}

/// `xdg_popup.grab` with `serial` from `client` (research P6, P9).
fn grab(state: &mut ServerState, xdg_id: &ObjectId, client: &ClientId, serial: u32) {
    let Some(p) = state.popups.get(xdg_id) else {
        return;
    };
    if p.committed {
        p.resource.post_error(
            xdg_popup::Error::InvalidGrab,
            "grab asked for after the initial commit",
        );
        return;
    }
    let parent = p.parent.clone();
    let mut refused = false;
    if let Some(pp) = parent.as_ref().and_then(|parent| state.popups.get(parent)) {
        if pp.dismissed {
            refused = true;
        } else if pp.grab != GrabState::Asked {
            p.resource.post_error(
                xdg_popup::Error::InvalidGrab,
                "the parent popup has no grab",
            );
            return;
        }
    }
    let ok = !refused && seat::grab_serial_ok(state, client, serial);
    if let Some(p) = state.popups.get_mut(xdg_id) {
        p.grab = if ok {
            GrabState::Asked
        } else {
            GrabState::Refused
        };
    }
}

/// `xdg_popup.reposition`: the new rules apply from now on; the shell hears of the newest
/// waiting request only, when the window events are taken (research P11).
fn reposition(state: &mut ServerState, xdg_id: &ObjectId, positioner: &XdgPositioner, token: u32) {
    let Some(p) = state.popups.get_mut(xdg_id) else {
        return;
    };
    let Some(rules) = xdg_shell::positioner_rules(positioner) else {
        p.wm_base.post_error(
            xdg_wm_base::Error::InvalidPositioner,
            "the positioner has no size or anchor rectangle",
        );
        return;
    };
    if p.dismissed {
        return;
    }
    p.rules = rules;
    p.reposition = Some((token, rules));
}

/// Reports the waiting repositions of popups the shell knows, in id order.
pub(crate) fn report_repositions(state: &mut ServerState) {
    let mut waiting: Vec<(PopupId, u32, PopupRules)> = state
        .popups
        .values_mut()
        .filter(|p| !p.dismissed)
        .filter_map(|p| {
            let id = p.id?;
            let (token, rules) = p.reposition.take()?;
            Some((id, token, rules))
        })
        .collect();
    waiting.sort_by_key(|(id, _, _)| *id);
    state.window_events.extend(
        waiting
            .into_iter()
            .map(|(id, token, rules)| WindowEvent::PopupRepositionRequested { id, rules, token }),
    );
}

/// The popup's `wl_surface` was destroyed: the popup ends, children first, without an error.
pub(crate) fn surface_destroyed(state: &mut ServerState, xdg_id: &ObjectId) {
    if state.popups.contains_key(xdg_id) {
        end(state, xdg_id, true);
    }
}

/// Whether `xdg_id` still has a live `xdg_popup` (its `xdg_surface` may not be destroyed yet).
pub(crate) fn has_role_object(state: &ServerState, xdg_id: &ObjectId) -> bool {
    state
        .popups
        .get(xdg_id)
        .is_some_and(|p| p.resource.is_alive())
}

/// The `xdg_surface` is gone: forget its popup.
pub(crate) fn forget(state: &mut ServerState, xdg_id: &ObjectId) {
    end(state, xdg_id, true);
    state.popups.remove(xdg_id);
}

impl Dispatch<XdgPopup, ObjectId> for ServerState {
    fn request(
        state: &mut Self,
        client: &Client,
        resource: &XdgPopup,
        request: xdg_popup::Request,
        xdg_id: &ObjectId,
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        match request {
            xdg_popup::Request::Grab { serial, .. } => grab(state, xdg_id, &client.id(), serial),
            xdg_popup::Request::Destroy => {
                let has_children = state
                    .popups
                    .values()
                    .any(|p| p.parent.as_ref() == Some(xdg_id) && visible(p));
                if has_children && let Some(p) = state.popups.get(xdg_id) {
                    p.wm_base.post_error(
                        xdg_wm_base::Error::NotTheTopmostPopup,
                        "a popup destroyed while it has shown child popups",
                    );
                }
                let _ = resource;
            }
            xdg_popup::Request::Reposition { positioner, token } => {
                reposition(state, xdg_id, &positioner, token);
            }
            _ => {}
        }
    }

    fn destroyed(state: &mut Self, _: ClientId, _: &XdgPopup, xdg_id: &ObjectId) {
        end(state, xdg_id, true);
        state.popups.remove(xdg_id);
    }
}
