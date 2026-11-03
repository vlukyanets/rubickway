//! `wl_output`: one global per display with its real description (contracts/protocols.md
//! "wl_output", spec FR-015). Without a display there is no output.

use wayland_server::backend::{ClientId, GlobalId, ObjectId};
use wayland_server::protocol::wl_output::{self, WlOutput};
use wayland_server::{Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource};

use super::{ServerState, compositor};
use crate::canvas::ViewerId;
use crate::shell::WindowId;

/// Which display a global or resource belongs to (the backend's output id).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OutputKey(pub u32);

/// What applications are told about one display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputDesc {
    /// Connector name, e.g. `eDP-1`.
    pub name: String,
    pub make: String,
    pub model: String,
    /// Physical size in millimetres (0 when unknown).
    pub physical_mm: (u32, u32),
    /// Mode width and height in pixels, refresh in mHz.
    pub mode: (u32, u32, u32),
    /// Position in the row of displays, logical pixels.
    pub position: (i32, i32),
}

/// One output global and the `wl_output` objects clients bound from it.
pub(crate) struct OutputEntry {
    key: OutputKey,
    global: GlobalId,
    desc: OutputDesc,
    resources: Vec<WlOutput>,
}

fn int(v: u32) -> i32 {
    i32::try_from(v).unwrap_or(i32::MAX)
}

/// Adds the output of display `id`; for a known display, sends the new description (a re-flow
/// moved it) to every binding.
pub(crate) fn add(state: &mut ServerState, id: u32, desc: OutputDesc) {
    let key = OutputKey(id);
    let scale = state.scales().0;
    if let Some(entry) = state.outputs.iter_mut().find(|o| o.key == key) {
        entry.desc = desc;
        for output in entry.resources.iter().filter(|o| o.is_alive()) {
            send(scale, &entry.desc, output);
        }
        return;
    }
    let global = state
        .dh
        .create_global::<ServerState, WlOutput, OutputKey>(4, key);
    state.outputs.push(OutputEntry {
        key,
        global,
        desc,
        resources: Vec::new(),
    });
}

/// Removes the output of display `id`: windows on it get a `leave`; objects clients still hold
/// stay valid and inert.
pub(crate) fn remove(state: &mut ServerState, id: u32) {
    let Some(i) = state.outputs.iter().position(|o| o.key == OutputKey(id)) else {
        return;
    };
    let on_it: Vec<WindowId> = state
        .window_outputs
        .iter()
        .filter(|(_, ids)| ids.contains(&id))
        .map(|(w, _)| *w)
        .collect();
    for window in on_it {
        cross(state, window, id, false);
        if let Some(ids) = state.window_outputs.get_mut(&window) {
            ids.retain(|o| *o != id);
        }
    }
    let entry = state.outputs.remove(i);
    state.dh.remove_global::<ServerState>(entry.global);
}

/// Sends `enter` (or `leave`) of display `id` to every surface of `window` (its root and
/// subsurfaces), for each of the client's objects of that output.
fn cross(state: &ServerState, window: WindowId, id: u32, enter: bool) {
    let Some(root) = state.windows.get(&window) else {
        return;
    };
    cross_tree(state, root, id, enter);
}

/// A subsurface just joined a window's tree (`enter`) or is about to leave it (`leave`): it,
/// and its own subsurfaces, enter or leave the displays its window is on (FR-015).
pub(crate) fn cross_subtree(state: &ServerState, sub: &ObjectId, enter: bool) {
    let Some(window) = state
        .windows
        .iter()
        .find(|(_, root)| compositor::tree(state, root).contains(sub))
        .map(|(w, _)| *w)
    else {
        return;
    };
    for id in state.window_outputs.get(&window).into_iter().flatten() {
        cross_tree(state, sub, *id, enter);
    }
}

/// `enter` or `leave` of display `id` for every surface of the tree at `top`.
fn cross_tree(state: &ServerState, top: &ObjectId, id: u32, enter: bool) {
    let Some(entry) = state.outputs.iter().find(|o| o.key == OutputKey(id)) else {
        return;
    };
    for surface in compositor::tree(state, top)
        .iter()
        .filter_map(|s| state.surfaces.get(s))
    {
        for output in entry
            .resources
            .iter()
            .filter(|o| o.is_alive() && o.client().map(|c| c.id()) == Some(surface.client.clone()))
        {
            if enter {
                surface.resource.enter(output);
            } else {
                surface.resource.leave(output);
            }
        }
    }
}

/// `window` is shown on displays `ids` now: `enter` for new ones, `leave` for old ones.
pub(crate) fn set_window_outputs(state: &mut ServerState, window: WindowId, ids: &[u32]) {
    if !state.windows.contains_key(&window) {
        return;
    }
    let old = state
        .window_outputs
        .get(&window)
        .cloned()
        .unwrap_or_default();
    for id in old.iter().filter(|o| !ids.contains(o)) {
        cross(state, window, *id, false);
    }
    for id in ids.iter().filter(|o| !old.contains(o)) {
        cross(state, window, *id, true);
    }
    let mut now = ids.to_vec();
    now.dedup();
    state.window_outputs.insert(window, now);
}

/// The viewer of the display `output` belongs to, if that display is still there (FR-022).
pub(crate) fn viewer_of(state: &ServerState, output: &WlOutput) -> Option<ViewerId> {
    let key = *output.data::<OutputKey>()?;
    state
        .outputs
        .iter()
        .any(|o| o.key == key)
        .then_some(ViewerId(key.0))
}

/// Sends an output's description, then `done`.
fn send(scale: i32, desc: &OutputDesc, output: &WlOutput) {
    output.geometry(
        desc.position.0,
        desc.position.1,
        int(desc.physical_mm.0),
        int(desc.physical_mm.1),
        wl_output::Subpixel::Unknown,
        desc.make.clone(),
        desc.model.clone(),
        wl_output::Transform::Normal,
    );
    output.mode(
        wl_output::Mode::Current | wl_output::Mode::Preferred,
        int(desc.mode.0),
        int(desc.mode.1),
        int(desc.mode.2),
    );
    if output.version() >= 2 {
        output.scale(scale);
    }
    if output.version() >= 4 {
        output.name(desc.name.clone());
        output.description(format!("{} {} ({})", desc.make, desc.model, desc.name));
    }
    if output.version() >= 2 {
        output.done();
    }
}

impl GlobalDispatch<WlOutput, OutputKey> for ServerState {
    fn bind(
        state: &mut Self,
        _: &DisplayHandle,
        client: &Client,
        resource: New<WlOutput>,
        key: &OutputKey,
        data_init: &mut DataInit<'_, Self>,
    ) {
        let output = data_init.init(resource, *key);
        let scale = state.scales().0;
        // A global removed while the bind was in flight: the object stays inert.
        let Some(entry) = state.outputs.iter_mut().find(|o| o.key == *key) else {
            return;
        };
        send(scale, &entry.desc, &output);
        entry.resources.push(output.clone());
        // The surfaces of the client's windows on this display enter it.
        let client_id = client.id();
        for (window, root) in &state.windows {
            let on_it = state
                .window_outputs
                .get(window)
                .is_some_and(|ids| ids.contains(&key.0));
            if !on_it {
                continue;
            }
            for surface in compositor::tree(state, root)
                .iter()
                .filter_map(|s| state.surfaces.get(s))
                .filter(|s| s.client == client_id)
            {
                surface.resource.enter(&output);
            }
        }
    }
}

impl Dispatch<WlOutput, OutputKey> for ServerState {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &WlOutput,
        _: wl_output::Request,
        _: &OutputKey,
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
    }

    fn destroyed(state: &mut Self, _: ClientId, resource: &WlOutput, key: &OutputKey) {
        if let Some(entry) = state.outputs.iter_mut().find(|o| o.key == *key) {
            entry.resources.retain(|o| o.id() != resource.id());
        }
    }
}
