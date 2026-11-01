//! `wl_subcompositor`, `wl_subsurface`: surfaces placed relative to a parent, with
//! synchronised or desynchronised commits (research R6). Reference: wlroots
//! `wlr_subcompositor.c`.

use wayland_server::backend::{ClientId, ObjectId};
use wayland_server::protocol::wl_subcompositor::{self, WlSubcompositor};
use wayland_server::protocol::wl_subsurface::{self, WlSubsurface};
use wayland_server::{Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource};

use super::ServerState;
use super::compositor::{self, Role, StackEntry};

pub(crate) fn create_global(dh: &DisplayHandle) {
    dh.create_global::<ServerState, WlSubcompositor, ()>(1, ());
}

impl GlobalDispatch<WlSubcompositor, ()> for ServerState {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<WlSubcompositor>,
        _: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<WlSubcompositor, ()> for ServerState {
    fn request(
        state: &mut Self,
        _: &Client,
        resource: &WlSubcompositor,
        request: wl_subcompositor::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        let wl_subcompositor::Request::GetSubsurface {
            id,
            surface,
            parent,
        } = request
        else {
            return;
        };
        let (sid, pid) = (surface.id(), parent.id());
        data_init.init(id, sid.clone());
        if compositor::is_ancestor_or_self(state, &sid, &pid) {
            resource.post_error(
                wl_subcompositor::Error::BadParent,
                "the parent is the surface itself or one of its descendants",
            );
            return;
        }
        let has_parent = state.surfaces.get(&sid).is_some_and(|s| s.parent.is_some());
        if has_parent || !compositor::set_role(state, &sid, Role::Subsurface) {
            resource.post_error(
                wl_subcompositor::Error::BadSurface,
                "the surface already has a role",
            );
            return;
        }
        if !state.surfaces.contains_key(&pid) {
            return;
        }
        if let Some(s) = state.surfaces.get_mut(&sid) {
            s.parent = Some(pid.clone());
            s.sync = true;
            s.position = (0, 0);
            s.pending_position = (0, 0);
        }
        if let Some(p) = state.surfaces.get_mut(&pid) {
            p.stack.push(StackEntry::Child(sid.clone()));
            p.pending_stack.push(StackEntry::Child(sid));
        }
    }
}

impl Dispatch<WlSubsurface, ObjectId> for ServerState {
    fn request(
        state: &mut Self,
        _: &Client,
        resource: &WlSubsurface,
        request: wl_subsurface::Request,
        surface: &ObjectId,
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        let parent = state.surfaces.get(surface).and_then(|s| s.parent.clone());
        match request {
            wl_subsurface::Request::SetPosition { x, y } => {
                if let Some(s) = state.surfaces.get_mut(surface) {
                    s.pending_position = (x, y);
                }
            }
            wl_subsurface::Request::PlaceAbove { sibling } => {
                place(state, resource, surface, parent, &sibling.id(), true);
            }
            wl_subsurface::Request::PlaceBelow { sibling } => {
                place(state, resource, surface, parent, &sibling.id(), false);
            }
            wl_subsurface::Request::SetSync => {
                if let Some(s) = state.surfaces.get_mut(surface) {
                    s.sync = true;
                }
            }
            wl_subsurface::Request::SetDesync => {
                if let Some(s) = state.surfaces.get_mut(surface) {
                    s.sync = false;
                }
                compositor::apply_cached_if_desync(state, surface);
            }
            _ => {}
        }
    }

    fn destroyed(state: &mut Self, _: ClientId, _: &WlSubsurface, surface: &ObjectId) {
        let parent = state
            .surfaces
            .get_mut(surface)
            .and_then(|s| s.parent.take());
        if let Some(parent) = parent
            && let Some(p) = state.surfaces.get_mut(&parent)
        {
            let entry = StackEntry::Child(surface.clone());
            p.stack.retain(|e| *e != entry);
            p.pending_stack.retain(|e| *e != entry);
            state.dirty = true;
        }
    }
}

/// Moves `surface` directly above or below `sibling` (the parent or another child) in the
/// parent's pending stacking order.
fn place(
    state: &mut ServerState,
    resource: &WlSubsurface,
    surface: &ObjectId,
    parent: Option<ObjectId>,
    sibling: &ObjectId,
    above: bool,
) {
    let Some(parent) = parent else {
        return;
    };
    let Some(p) = state.surfaces.get_mut(&parent) else {
        return;
    };
    let target = if *sibling == parent {
        StackEntry::Own
    } else {
        StackEntry::Child(sibling.clone())
    };
    let me = StackEntry::Child(surface.clone());
    if target == me || !p.pending_stack.contains(&target) {
        resource.post_error(
            wl_subsurface::Error::BadSurface,
            "the sibling is neither the parent nor a sibling",
        );
        return;
    }
    p.pending_stack.retain(|e| *e != me);
    let Some(at) = p.pending_stack.iter().position(|e| *e == target) else {
        return;
    };
    let at = if above { at + 1 } else { at };
    p.pending_stack.insert(at, me);
}
