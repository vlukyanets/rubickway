//! `wl_output`: the viewer as the one display applications see (research R12).

use wayland_server::backend::{ClientId, ObjectId};
use wayland_server::protocol::wl_output::{self, WlOutput};
use wayland_server::{Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource};

use super::ServerState;

pub(crate) fn create_global(dh: &DisplayHandle) {
    dh.create_global::<ServerState, WlOutput, ()>(4, ());
}

/// Sends the current mode and scale to one output, then `done`.
fn announce(state: &ServerState, output: &WlOutput) {
    let ((w, h), _) = state.viewer;
    let (integer, _) = state.scales();
    let (w, h) = (
        i32::try_from(w).unwrap_or(i32::MAX),
        i32::try_from(h).unwrap_or(i32::MAX),
    );
    output.mode(
        wl_output::Mode::Current | wl_output::Mode::Preferred,
        w,
        h,
        60_000,
    );
    if output.version() >= 2 {
        output.scale(integer);
        output.done();
    }
}

/// Re-sends mode and scale to every output (after the viewer changed).
pub(crate) fn announce_all(state: &ServerState) {
    for output in state.outputs.iter().filter(|o| o.is_alive()) {
        announce(state, output);
    }
}

/// Tells surface `id` (of a mapped window) that it is on the client's outputs.
pub(crate) fn enter(state: &ServerState, id: &ObjectId) {
    let Some(surface) = state.surfaces.get(id) else {
        return;
    };
    for output in &state.outputs {
        if output.client().map(|c| c.id()) == Some(surface.client.clone()) {
            surface.resource.enter(output);
        }
    }
}

impl GlobalDispatch<WlOutput, ()> for ServerState {
    fn bind(
        state: &mut Self,
        _: &DisplayHandle,
        client: &Client,
        resource: New<WlOutput>,
        _: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        let output = data_init.init(resource, ());
        output.geometry(
            0,
            0,
            0,
            0,
            wl_output::Subpixel::Unknown,
            "RubickWay".to_string(),
            "viewer".to_string(),
            wl_output::Transform::Normal,
        );
        if output.version() >= 4 {
            output.name("RUBICKWAY-1".to_string());
            output.description("RubickWay viewer".to_string());
        }
        announce(state, &output);
        let client_id = client.id();
        for root in state.windows.values() {
            if let Some(s) = state.surfaces.get(root)
                && s.client == client_id
            {
                s.resource.enter(&output);
            }
        }
        state.outputs.push(output);
    }
}

impl Dispatch<WlOutput, ()> for ServerState {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &WlOutput,
        _: wl_output::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
    }

    fn destroyed(state: &mut Self, _: ClientId, resource: &WlOutput, _: &()) {
        state.outputs.retain(|o| o.id() != resource.id());
    }
}
