//! `zwp_relative_pointer_manager_v1` version 1: every device movement for applications that ask
//! (pointer-lock contracts/protocols.md, research L4, L11).

use wayland_protocols::wp::relative_pointer::zv1::server::zwp_relative_pointer_manager_v1::{
    self, ZwpRelativePointerManagerV1,
};
use wayland_protocols::wp::relative_pointer::zv1::server::zwp_relative_pointer_v1::{
    self, ZwpRelativePointerV1,
};
use wayland_server::backend::ClientId;
use wayland_server::{Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource};

use super::dmabuf::client_number;
use super::{ServerState, seat};
use crate::canvas::Vector;
use crate::shell::WindowId;

/// The version implemented and tested (Principle VI).
const VERSION: u32 = 1;

/// How many relative pointers of one application are told each motion (research L11): one per
/// `wl_pointer` is normal; more stay inert, so one motion costs at most this many events.
pub(crate) const MAX_PER_CLIENT: usize = 32;

/// Tells the application whose `window` has the pointer one device motion (research L4): on
/// its first `MAX_PER_CLIENT` live relative pointers, in creation order.
pub(crate) fn send_motion(
    state: &ServerState,
    window: WindowId,
    delta: Vector,
    unaccel: Vector,
    time_us: u64,
) {
    let Some(client) = seat::pointer_client(state, window) else {
        return;
    };
    let Some(list) = state.seat.relative_pointers.get(&client) else {
        return;
    };
    let hi = (time_us >> 32) as u32;
    let lo = (time_us & 0xffff_ffff) as u32;
    for relative in list.iter().filter(|r| r.is_alive()).take(MAX_PER_CLIENT) {
        relative.relative_motion(hi, lo, delta.x, delta.y, unaccel.x, unaccel.y);
    }
}

pub(crate) fn create_global(dh: &DisplayHandle) {
    dh.create_global::<ServerState, ZwpRelativePointerManagerV1, ()>(VERSION, ());
}

impl GlobalDispatch<ZwpRelativePointerManagerV1, ()> for ServerState {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<ZwpRelativePointerManagerV1>,
        _: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<ZwpRelativePointerManagerV1, ()> for ServerState {
    fn request(
        state: &mut Self,
        client: &Client,
        _: &ZwpRelativePointerManagerV1,
        request: zwp_relative_pointer_manager_v1::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        // `destroy` ends only the manager; its relative pointers keep working.
        if let zwp_relative_pointer_manager_v1::Request::GetRelativePointer { id, .. } = request {
            let relative = data_init.init(id, ());
            let list = state.seat.relative_pointers.entry(client.id()).or_default();
            list.push(relative);
            if list.len() == MAX_PER_CLIENT + 1 {
                tracing::debug!(
                    "application {} has more than {MAX_PER_CLIENT} relative pointers; the \
                     newer ones get no motion",
                    client_number(client)
                );
            }
        }
    }
}

impl Dispatch<ZwpRelativePointerV1, ()> for ServerState {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &ZwpRelativePointerV1,
        _: zwp_relative_pointer_v1::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        // `destroy` is the only request; `destroyed` forgets it.
    }

    fn destroyed(state: &mut Self, client: ClientId, relative: &ZwpRelativePointerV1, _: &()) {
        if let Some(list) = state.seat.relative_pointers.get_mut(&client) {
            list.retain(|r| r.id() != relative.id());
            if list.is_empty() {
                state.seat.relative_pointers.remove(&client);
            }
        }
    }
}
