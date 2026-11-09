//! `wl_compositor`, `wl_surface`, `wl_region`: double-buffered surface state, commit, and the
//! surface tree (research R6). References: Smithay `wayland/compositor`, wlroots
//! `wlr_compositor.c`.

use std::collections::HashMap;

use wayland_protocols::wp::fractional_scale::v1::server::wp_fractional_scale_v1::WpFractionalScaleV1;
use wayland_protocols::wp::linux_drm_syncobj::v1::server::wp_linux_drm_syncobj_surface_v1::WpLinuxDrmSyncobjSurfaceV1;
use wayland_protocols::wp::viewporter::server::wp_viewport::{self, WpViewport};
use wayland_server::backend::{ClientId, ObjectId};
use wayland_server::protocol::wl_buffer::WlBuffer;
use wayland_server::protocol::wl_callback::WlCallback;
use wayland_server::protocol::wl_compositor::{self, WlCompositor};
use wayland_server::protocol::wl_output;
use wayland_server::protocol::wl_region::{self, WlRegion};
use wayland_server::protocol::wl_surface::{self, WlSurface};
use wayland_server::{Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource};

use super::commit_queue::WaitId;
use super::descriptors;
use super::dmabuf::{BufferKey, GpuBuffer, ImportState, ShownImage, Wait};
use super::shm::BufferKind;
use super::syncobj::{self, SyncPoint, ToRelease};
use super::{ServerState, SurfaceKey, SurfacePart, seat, shm, xdg_shell};
use crate::canvas::{Point, Rect, Size, Vector};
pub use crate::render::Transform;

pub(crate) fn create_globals(dh: &DisplayHandle) {
    dh.create_global::<ServerState, WlCompositor, ()>(6, ());
}

fn transform_from_wl(t: wl_output::Transform) -> Option<Transform> {
    use wl_output::Transform as T;
    Some(match t {
        T::Normal => Transform::Normal,
        T::_90 => Transform::Rotate90,
        T::_180 => Transform::Rotate180,
        T::_270 => Transform::Rotate270,
        T::Flipped => Transform::Flipped,
        T::Flipped90 => Transform::Flipped90,
        T::Flipped180 => Transform::Flipped180,
        T::Flipped270 => Transform::Flipped270,
        _ => return None,
    })
}

/// A region as disjoint rectangles.
pub(crate) type Region = Vec<Rect>;

/// Double-buffered surface state.
#[derive(Debug, Clone)]
pub(crate) struct SurfaceState {
    /// Pending only: `None` = no attach since the last commit; `Some(None)` = null buffer.
    pub attach: Option<Option<WlBuffer>>,
    pub scale: i32,
    pub transform: Transform,
    /// `None` = the whole surface accepts input.
    pub input: Option<Region>,
    pub viewport_src: Option<Rect>,
    pub viewport_dst: Option<(i32, i32)>,
    pub callbacks: Vec<WlCallback>,
    /// Pending only, like `attach`: the points of the buffer attached.
    pub acquire: Option<SyncPoint>,
    pub release: Option<SyncPoint>,
}

impl Default for SurfaceState {
    fn default() -> Self {
        SurfaceState {
            attach: None,
            scale: 1,
            transform: Transform::Normal,
            input: None,
            viewport_src: None,
            viewport_dst: None,
            callbacks: Vec::new(),
            acquire: None,
            release: None,
        }
    }
}

/// What a surface is used for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Role {
    None,
    Toplevel,
    Popup,
    Subsurface,
    Cursor,
}

/// An entry of a surface's stacking list: itself or one of its subsurfaces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StackEntry {
    Own,
    Child(ObjectId),
}

pub(crate) struct Surface {
    pub resource: WlSurface,
    pub client: ClientId,
    pub key: SurfaceKey,
    pub pending: SurfaceState,
    pub current: SurfaceState,
    /// Committed state of a synchronised subsurface, applied on the parent's commit.
    pub cached: Option<SurfaceState>,
    /// Pixel size of the current buffer; `None` = no content.
    pub buffer_size: Option<(i32, i32)>,
    /// A committed buffer not copied yet.
    pub upload: Option<WlBuffer>,
    /// The GPU image shown: it stays the application's until replaced (research G9).
    pub image: Option<WlBuffer>,
    /// How that image goes back, when it came with explicit synchronisation.
    pub image_release: Option<SyncPoint>,
    /// The surface's `wp_linux_drm_syncobj_surface_v1`.
    pub syncobj: Option<WpLinuxDrmSyncobjSurfaceV1>,
    pub has_content: bool,
    pub role: Role,
    pub parent: Option<ObjectId>,
    pub stack: Vec<StackEntry>,
    pub pending_stack: Vec<StackEntry>,
    pub position: (i32, i32),
    pub pending_position: (i32, i32),
    pub sync: bool,
    /// Frame callbacks committed and waiting for a presented frame.
    pub frame_callbacks: Vec<WlCallback>,
    pub viewport: Option<WpViewport>,
    pub fractional: Option<WpFractionalScaleV1>,
}

impl Surface {
    /// Logical size of the current content.
    pub fn logical_size(&self) -> Option<Size> {
        let (w, h) = self.buffer_size?;
        if let Some((dw, dh)) = self.current.viewport_dst {
            return Some(Size::new(f64::from(dw), f64::from(dh)));
        }
        if let Some(src) = self.current.viewport_src {
            return Some(src.size);
        }
        let (w, h) = buffer_logical((w, h), self.current.scale, self.current.transform);
        Some(Size::new(w, h))
    }

    /// The buffer size after transform and scale.
    fn buffer_logical_size(&self) -> Option<Size> {
        let (w, h) = self.buffer_size?;
        let (w, h) = buffer_logical((w, h), self.current.scale, self.current.transform);
        Some(Size::new(w, h))
    }
}

fn buffer_logical((w, h): (i32, i32), scale: i32, transform: Transform) -> (f64, f64) {
    let (w, h) = if transform.swaps_axes() {
        (h, w)
    } else {
        (w, h)
    };
    let scale = f64::from(scale.max(1));
    (f64::from(w) / scale, f64::from(h) / scale)
}

impl GlobalDispatch<WlCompositor, ()> for ServerState {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<WlCompositor>,
        _: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<WlCompositor, ()> for ServerState {
    fn request(
        state: &mut Self,
        client: &Client,
        _: &WlCompositor,
        request: wl_compositor::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            wl_compositor::Request::CreateSurface { id } => {
                let resource = data_init.init(id, ());
                let key = state.new_surface_key();
                state.surfaces.insert(
                    resource.id(),
                    Surface {
                        resource,
                        client: client.id(),
                        key,
                        pending: SurfaceState::default(),
                        current: SurfaceState::default(),
                        cached: None,
                        buffer_size: None,
                        upload: None,
                        image: None,
                        image_release: None,
                        syncobj: None,
                        has_content: false,
                        role: Role::None,
                        parent: None,
                        stack: vec![StackEntry::Own],
                        pending_stack: vec![StackEntry::Own],
                        position: (0, 0),
                        pending_position: (0, 0),
                        sync: true,
                        frame_callbacks: Vec::new(),
                        viewport: None,
                        fractional: None,
                    },
                );
            }
            wl_compositor::Request::CreateRegion { id } => {
                let region = data_init.init(id, ());
                state.regions.insert(region.id(), Vec::new());
            }
            _ => {}
        }
    }
}

impl Dispatch<WlRegion, ()> for ServerState {
    fn request(
        state: &mut Self,
        _: &Client,
        region: &WlRegion,
        request: wl_region::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        let Some(rects) = state.regions.get_mut(&region.id()) else {
            return;
        };
        match request {
            wl_region::Request::Add {
                x,
                y,
                width,
                height,
            } => {
                let r = int_rect(x, y, width, height);
                if !r.is_empty() {
                    let mut out: Vec<Rect> = rects.iter().flat_map(|o| o.difference(&r)).collect();
                    out.push(r);
                    *rects = out;
                }
            }
            wl_region::Request::Subtract {
                x,
                y,
                width,
                height,
            } => {
                let r = int_rect(x, y, width, height);
                *rects = rects.iter().flat_map(|o| o.difference(&r)).collect();
            }
            _ => {}
        }
    }

    fn destroyed(state: &mut Self, _: ClientId, region: &WlRegion, _: &()) {
        state.regions.remove(&region.id());
    }
}

fn int_rect(x: i32, y: i32, w: i32, h: i32) -> Rect {
    Rect::new(
        Point::new(f64::from(x), f64::from(y)),
        Size::new(f64::from(w), f64::from(h)),
    )
}

impl Dispatch<WlCallback, ()> for ServerState {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &WlCallback,
        _: <WlCallback as Resource>::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
    }
}

impl Dispatch<WlSurface, ()> for ServerState {
    fn request(
        state: &mut Self,
        _: &Client,
        resource: &WlSurface,
        request: wl_surface::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        let id = resource.id();
        if let wl_surface::Request::Frame { callback } = request {
            let cb = data_init.init(callback, ());
            if let Some(s) = state.surfaces.get_mut(&id) {
                s.pending.callbacks.push(cb);
            }
            return;
        }
        if let wl_surface::Request::Commit = request {
            commit(state, &id);
            return;
        }
        let Some(surface) = state.surfaces.get_mut(&id) else {
            return;
        };
        match request {
            wl_surface::Request::Attach { buffer, x, y } => {
                if resource.version() >= 5 && (x != 0 || y != 0) {
                    resource.post_error(
                        wl_surface::Error::InvalidOffset,
                        "attach with an offset; use wl_surface.offset",
                    );
                    return;
                }
                surface.pending.attach = Some(buffer);
            }
            wl_surface::Request::SetInputRegion { region } => {
                surface.pending.input =
                    region.map(|r| state.regions.get(&r.id()).cloned().unwrap_or_default());
            }
            wl_surface::Request::SetBufferTransform { transform } => {
                match transform.into_result().ok().and_then(transform_from_wl) {
                    Some(t) => surface.pending.transform = t,
                    None => resource.post_error(
                        wl_surface::Error::InvalidTransform,
                        "unknown buffer transform",
                    ),
                }
            }
            wl_surface::Request::SetBufferScale { scale } => {
                if scale < 1 {
                    resource.post_error(wl_surface::Error::InvalidScale, "buffer scale below 1");
                } else {
                    surface.pending.scale = scale;
                }
            }
            // Damage is recorded implicitly: every commit with a buffer re-uploads it.
            // Opaque regions and wl_surface.offset (cursor and drag icons) are not used yet.
            _ => {}
        }
    }

    fn destroyed(state: &mut Self, _: ClientId, resource: &WlSurface, _: &()) {
        surface_destroyed(state, &resource.id());
    }
}

/// Applies a commit (or caches it for a synchronised subsurface).
pub(crate) fn commit(state: &mut ServerState, id: &ObjectId) {
    if !syncobj::check_commit(state, id) {
        return;
    }
    let Some(surface) = state.surfaces.get_mut(id) else {
        return;
    };
    let mut committed = surface.pending.clone();
    surface.pending.attach = None;
    surface.pending.acquire = None;
    surface.pending.release = None;
    surface.pending.callbacks.clear();
    if surface.role == Role::Subsurface && effectively_sync(state, id) {
        let Some(surface) = state.surfaces.get_mut(id) else {
            return;
        };
        match surface.cached.take() {
            Some(mut cached) => {
                if committed.attach.is_some() {
                    cached.attach = committed.attach.take();
                    // The image it replaces will never be read: it goes back now.
                    if let Some(never_read) = cached.release.take() {
                        never_read.signal();
                    }
                    cached.acquire = committed.acquire.take();
                    cached.release = committed.release.take();
                }
                cached.callbacks.append(&mut committed.callbacks);
                committed.attach = cached.attach;
                committed.acquire = cached.acquire;
                committed.release = cached.release;
                committed.callbacks = cached.callbacks;
                surface.cached = Some(committed);
            }
            None => surface.cached = Some(committed),
        }
        return;
    }
    enqueue(state, id, committed);
}

/// One surface's share of a transaction.
pub(crate) struct Part {
    surface: ObjectId,
    /// The committed state to make current; `None` for a child that only moves.
    state: Option<SurfaceState>,
    /// It attaches a GPU image: if the application destroys that while the commit waits, the
    /// window keeps its picture (misbehaving case 9).
    attaches_image: bool,
    /// The stacking order that takes effect with it.
    stack: Option<Vec<StackEntry>>,
    /// The position that takes effect with it (subsurfaces).
    position: Option<(i32, i32)>,
}

/// One commit: the commit root's state and the cached state of its synchronised descendants, all
/// taking effect together (research G7).
pub(crate) struct Transaction {
    /// The commit root.
    root: ObjectId,
    parts: Vec<Part>,
    /// The toplevel's or popup's geometry and limits at the commit, if the root is one.
    xdg: Option<xdg_shell::XdgCapture>,
}

/// Makes a transaction of a commit on `id` and queues it; it applies at once if it has nothing
/// to wait for and nothing is ahead of it.
fn enqueue(state: &mut ServerState, id: &ObjectId, committed: SurfaceState) {
    let mut parts = vec![Part {
        surface: id.clone(),
        attaches_image: attached_image(state, &committed).is_some(),
        state: Some(committed),
        stack: state.surfaces.get(id).map(|s| s.pending_stack.clone()),
        position: None,
    }];
    gather_children(state, id, &mut parts);
    let mut waits: Vec<WaitId> = Vec::new();
    let mut files = Vec::new();
    let mut failures = Vec::new();
    for committed in parts.iter().filter_map(|p| p.state.as_ref()) {
        if let Some(image) = attached_image(state, committed) {
            match image.state {
                ImportState::Pending(wait) => waits.push(wait),
                ImportState::Ready => {}
                // Never read, so nothing to wait for.
                ImportState::Failed => continue,
            }
            // With an acquire point implicit synchronisation is ignored (contracts/protocols.md).
            let unfinished = match &committed.acquire {
                Some(acquire) => vec![acquire.unfinished()],
                None => image
                    .unfinished_planes()
                    .into_iter()
                    .map(|plane| plane.map(Some))
                    .collect(),
            };
            for file in unfinished {
                match file {
                    Ok(file) => files.extend(file),
                    Err(err) => failures.push(err),
                }
            }
        }
    }
    for err in failures {
        // A window must not stop for good for this, so the image is shown as it is.
        if !descriptors::note_failure(state, &err) {
            tracing::warn!("cannot wait for an image to be finished ({err}): showing it as it is");
        }
    }
    if !files.is_empty() {
        tracing::debug!("an image is not finished yet: its commit waits");
    }
    for fd in files {
        let wait = state.new_wait();
        state.waits.push(Wait { id: wait, fd });
        waits.push(wait);
    }
    let transaction = Transaction {
        root: id.clone(),
        parts,
        xdg: xdg_shell::capture(state, id),
    };
    let ready = state
        .queues
        .entry(id.clone())
        .or_default()
        .push(transaction, waits);
    apply_ready(state, id, ready);
}

/// The GPU image a committed state attaches, if it does.
fn attached_image<'a>(state: &'a ServerState, committed: &SurfaceState) -> Option<&'a GpuBuffer> {
    let buffer = committed.attach.as_ref()?.as_ref()?;
    match state.buffers.get(&buffer.id())? {
        BufferKind::Gpu(gpu) => Some(gpu),
        BufferKind::Shm(_) => None,
    }
}

/// Something a transaction waited for is finished: whatever can apply now, applies.
pub(crate) fn wait_done(state: &mut ServerState, wait: WaitId) {
    let roots: Vec<ObjectId> = state.queues.keys().cloned().collect();
    for root in roots {
        let Some(queue) = state.queues.get_mut(&root) else {
            continue;
        };
        let ready = queue.done(wait);
        apply_ready(state, &root, ready);
    }
}

/// Applies what a queue returned, and forgets the queue once it is empty.
fn apply_ready(state: &mut ServerState, root: &ObjectId, ready: Vec<Transaction>) {
    for transaction in ready {
        apply_transaction(state, transaction);
    }
    if state.queues.get(root).is_some_and(|q| q.is_empty()) {
        state.queues.remove(root);
    }
}

/// Adds to `parts` what the children of `id` take from their parent's commit: their position,
/// and the cached state of synchronised ones, with theirs in turn.
fn gather_children(state: &mut ServerState, id: &ObjectId, parts: &mut Vec<Part>) {
    let children: Vec<ObjectId> = state.surfaces.get(id).map_or_else(Vec::new, |s| {
        s.pending_stack
            .iter()
            .filter_map(|e| match e {
                StackEntry::Child(c) => Some(c.clone()),
                StackEntry::Own => None,
            })
            .collect()
    });
    for child in children {
        let Some(c) = state.surfaces.get_mut(&child) else {
            continue;
        };
        let cached = c.cached.take();
        let has_cached = cached.is_some();
        let stack = has_cached.then(|| c.pending_stack.clone());
        let position = Some(c.pending_position);
        parts.push(Part {
            surface: child.clone(),
            attaches_image: cached
                .as_ref()
                .is_some_and(|s| attached_image(state, s).is_some()),
            stack,
            position,
            state: cached,
        });
        if has_cached {
            gather_children(state, &child, parts);
        }
    }
}

/// Makes a transaction current. Stops at a protocol violation: that client is gone.
fn apply_transaction(state: &mut ServerState, transaction: Transaction) {
    for part in transaction.parts {
        let Some(surface) = state.surfaces.get_mut(&part.surface) else {
            continue;
        };
        if let Some(position) = part.position {
            surface.position = position;
        }
        if let Some(committed) = part.state
            && !apply(state, &part.surface, committed, part.attaches_image)
        {
            return;
        }
        if let Some(stack) = part.stack
            && let Some(surface) = state.surfaces.get_mut(&part.surface)
        {
            surface.stack = stack;
        }
    }
    let tree_root = root_of(state, &transaction.root);
    xdg_shell::tree_committed(state, &tree_root, &transaction.root, transaction.xdg);
}

/// Whether a subsurface (or one of its ancestors) is in synchronised mode.
fn effectively_sync(state: &ServerState, id: &ObjectId) -> bool {
    let mut current = Some(id.clone());
    while let Some(cid) = current {
        let Some(s) = state.surfaces.get(&cid) else {
            return false;
        };
        if s.role != Role::Subsurface {
            return false;
        }
        if s.sync {
            return true;
        }
        current = s.parent.clone();
    }
    false
}

/// After `set_desync`: a subsurface that is no longer synchronised applies its cached state.
pub(crate) fn apply_cached_if_desync(state: &mut ServerState, id: &ObjectId) {
    if effectively_sync(state, id) {
        return;
    }
    let Some(cached) = state.surfaces.get_mut(id).and_then(|s| s.cached.take()) else {
        return;
    };
    enqueue(state, id, cached);
}

/// Makes `committed` the current state of surface `id`. Returns false if the client was
/// disconnected for a protocol violation. A GPU image that failed, or was destroyed since the
/// commit (`attaches_image`), changes nothing: the window keeps its picture, the frame callbacks
/// complete, and the buffer goes back (research G8).
fn apply(
    state: &mut ServerState,
    id: &ObjectId,
    committed: SurfaceState,
    attaches_image: bool,
) -> bool {
    let attach = committed.attach.clone();
    if let Some(Some(buffer)) = &attach {
        let unusable = match state.buffers.get(&buffer.id()) {
            Some(BufferKind::Gpu(gpu)) => gpu.state == ImportState::Failed,
            Some(BufferKind::Shm(_)) => false,
            None => attaches_image,
        };
        if unusable {
            let mut committed = committed;
            if let Some(surface) = state.surfaces.get_mut(id) {
                surface.frame_callbacks.append(&mut committed.callbacks);
            }
            match &committed.release {
                Some(point) => point.signal(),
                None => shm::release(buffer),
            }
            return true;
        }
    }
    let new_size = match &attach {
        Some(Some(buffer)) => match state.buffers.get(&buffer.id()) {
            Some(b) => Some(Some(b.size())),
            None => Some(None),
        },
        Some(None) => Some(None),
        None => None,
    };
    let max = state.max_texture_side;
    let Some(surface) = state.surfaces.get_mut(id) else {
        return false;
    };
    let size = new_size.unwrap_or(surface.buffer_size);
    if let Some((w, h)) = size {
        if w as u32 > max || h as u32 > max {
            // wayland-server cannot send a wl_display error, so the surface's own
            // invalid_size error carries the reason (contracts/protocols.md).
            surface.resource.post_error(
                wl_surface::Error::InvalidSize,
                format!("buffer {w}x{h} larger than the {max} px the GPU can show"),
            );
            return false;
        }
        let swapped = if committed.transform.swaps_axes() {
            (h, w)
        } else {
            (w, h)
        };
        let divisible = swapped.0 % committed.scale == 0 && swapped.1 % committed.scale == 0;
        if committed.viewport_dst.is_none() && committed.viewport_src.is_none() && !divisible {
            surface.resource.post_error(
                wl_surface::Error::InvalidSize,
                "buffer size not divisible by the buffer scale",
            );
            return false;
        }
        if let Some(src) = committed.viewport_src {
            let (bw, bh) = buffer_logical((w, h), committed.scale, committed.transform);
            let end = src.end();
            if src.origin.x < 0.0 || src.origin.y < 0.0 || end.x > bw || end.y > bh {
                if let Some(viewport) = &surface.viewport {
                    viewport.post_error(
                        wp_viewport::Error::OutOfBuffer,
                        "viewport source outside the buffer",
                    );
                }
                return false;
            }
        }
    }
    let mut committed = committed;
    surface.frame_callbacks.append(&mut committed.callbacks);
    let client = surface.client.clone();
    let mut old_release = None;
    let replaced = if let Some(buffer) = attach {
        if let Some(old) = surface.upload.take() {
            shm::release(&old);
        }
        let old_image = surface.image.take();
        old_release = surface.image_release.take();
        match buffer {
            Some(buffer) => {
                surface.buffer_size = size;
                match image_of(&state.buffers, &buffer) {
                    Some((image, y_invert)) => {
                        surface.has_content = true;
                        state.shown_images.push(ShownImage {
                            surface: surface.key,
                            image,
                            y_invert,
                        });
                        surface.image = Some(buffer);
                        surface.image_release = committed.release.take();
                    }
                    None => surface.upload = Some(buffer),
                }
            }
            None => {
                surface.buffer_size = None;
                surface.has_content = false;
            }
        }
        old_image
    } else {
        None
    };
    let now_shown = surface.image.clone();
    committed.attach = None;
    committed.acquire = None;
    committed.release = None;
    surface.current = committed;
    if let Some(shown) = &now_shown {
        // Attached again before it went back: it is in use again.
        state.release.retain(|entry| !entry.is_buffer(shown));
    }
    if let Some(point) = old_release {
        // Each commit's point is signalled, also when the same image is attached again.
        state
            .release
            .retire(ToRelease::Point(point, client), state.frame_seq);
    } else if let Some(old) = replaced
        && now_shown
            .as_ref()
            .is_none_or(|shown| shown.id() != old.id())
    {
        state
            .release
            .retire(ToRelease::Buffer(old), state.frame_seq);
    }
    // Only the displays showing this surface's window need a new frame (research S7).
    state.damaged.push(id.clone());
    true
}

/// The image and orientation of a `wl_buffer` that is a GPU image.
fn image_of(
    buffers: &HashMap<ObjectId, BufferKind>,
    buffer: &WlBuffer,
) -> Option<(BufferKey, bool)> {
    match buffers.get(&buffer.id())? {
        BufferKind::Gpu(gpu) => Some((gpu.key, gpu.y_invert)),
        BufferKind::Shm(_) => None,
    }
}

/// The root of the surface tree containing `id`.
pub(crate) fn root_of(state: &ServerState, id: &ObjectId) -> ObjectId {
    let mut current = id.clone();
    while let Some(parent) = state.surfaces.get(&current).and_then(|s| s.parent.clone()) {
        current = parent;
    }
    current
}

/// `root` and all its descendants.
pub(crate) fn tree(state: &ServerState, root: &ObjectId) -> Vec<ObjectId> {
    let mut out = vec![root.clone()];
    let mut i = 0;
    while i < out.len() {
        if let Some(s) = state.surfaces.get(&out[i]) {
            for entry in &s.stack {
                if let StackEntry::Child(c) = entry {
                    out.push(c.clone());
                }
            }
        }
        i += 1;
    }
    out
}

/// Appends the parts of the tree at `id` drawn with its origin at `offset`.
pub(crate) fn collect_parts(
    state: &ServerState,
    id: &ObjectId,
    offset: Point,
    out: &mut Vec<SurfacePart>,
) {
    let Some(surface) = state.surfaces.get(id) else {
        return;
    };
    for entry in &surface.stack {
        match entry {
            StackEntry::Own => {
                if let Some(size) = surface.logical_size() {
                    out.push(SurfacePart {
                        key: surface.key,
                        rect: Rect::new(offset, size),
                        src: normalised_src(surface),
                        transform: surface.current.transform,
                        has_content: surface.has_content,
                    });
                }
            }
            StackEntry::Child(child) => {
                if let Some(c) = state.surfaces.get(child) {
                    let (x, y) = c.position;
                    let at = offset + Vector::new(f64::from(x), f64::from(y));
                    collect_parts(state, child, at, out);
                }
            }
        }
    }
}

fn normalised_src(surface: &Surface) -> Rect {
    let full = Rect::new(Point::new(0.0, 0.0), Size::new(1.0, 1.0));
    match (surface.current.viewport_src, surface.buffer_logical_size()) {
        (Some(src), Some(buf)) if buf.w > 0.0 && buf.h > 0.0 => Rect::new(
            Point::new(src.origin.x / buf.w, src.origin.y / buf.h),
            Size::new(src.size.w / buf.w, src.size.h / buf.h),
        ),
        _ => full,
    }
}

/// The topmost surface of the tree at `id` (drawn with its origin at `offset`) that accepts
/// input at point `p`, and `p` relative to that surface.
pub(crate) fn surface_at(
    state: &ServerState,
    id: &ObjectId,
    offset: Point,
    p: Point,
) -> Option<(ObjectId, Point)> {
    let surface = state.surfaces.get(id)?;
    for entry in surface.stack.iter().rev() {
        match entry {
            StackEntry::Own => {
                let Some(size) = surface.logical_size() else {
                    continue;
                };
                let local = Point::new(p.x - offset.x, p.y - offset.y);
                let bounds = Rect::new(Point::new(0.0, 0.0), size);
                let accepts = bounds.contains(local)
                    && surface
                        .current
                        .input
                        .as_ref()
                        .is_none_or(|region| region.iter().any(|r| r.contains(local)));
                if accepts {
                    return Some((id.clone(), local));
                }
            }
            StackEntry::Child(child) => {
                if let Some(c) = state.surfaces.get(child) {
                    let (x, y) = c.position;
                    let at = offset + Vector::new(f64::from(x), f64::from(y));
                    if let Some(hit) = surface_at(state, child, at, p) {
                        return Some(hit);
                    }
                }
            }
        }
    }
    None
}

/// Where surface `target` of the tree at `id` (origin at `offset`) is drawn.
pub(crate) fn surface_origin(
    state: &ServerState,
    id: &ObjectId,
    offset: Point,
    target: &ObjectId,
) -> Option<Point> {
    if id == target {
        return Some(offset);
    }
    let surface = state.surfaces.get(id)?;
    surface.stack.iter().find_map(|entry| match entry {
        StackEntry::Child(child) => {
            let c = state.surfaces.get(child)?;
            let (x, y) = c.position;
            let at = offset + Vector::new(f64::from(x), f64::from(y));
            surface_origin(state, child, at, target)
        }
        StackEntry::Own => None,
    })
}

/// Where the tree accepts input, as rectangles relative to the root surface origin.
pub(crate) fn input_shape(state: &ServerState, root: &ObjectId) -> Vec<Rect> {
    let mut out = Vec::new();
    input_shape_at(state, root, Point::new(0.0, 0.0), &mut out);
    out
}

fn input_shape_at(state: &ServerState, id: &ObjectId, offset: Point, out: &mut Vec<Rect>) {
    let Some(surface) = state.surfaces.get(id) else {
        return;
    };
    for entry in &surface.stack {
        match entry {
            StackEntry::Own => {
                let Some(size) = surface.logical_size() else {
                    continue;
                };
                let bounds = Rect::new(Point::new(0.0, 0.0), size);
                let v = offset - Point::new(0.0, 0.0);
                match &surface.current.input {
                    None => out.push(bounds.translate(v)),
                    Some(region) => out.extend(
                        region
                            .iter()
                            .filter_map(|r| r.intersection(&bounds))
                            .map(|r| r.translate(v)),
                    ),
                }
            }
            StackEntry::Child(child) => {
                if let Some(c) = state.surfaces.get(child) {
                    let (x, y) = c.position;
                    let at = offset + Vector::new(f64::from(x), f64::from(y));
                    input_shape_at(state, child, at, out);
                }
            }
        }
    }
}

/// The bounding box of the tree's content, relative to the root surface origin.
pub(crate) fn tree_bounds(state: &ServerState, root: &ObjectId) -> Option<Rect> {
    let mut parts = Vec::new();
    collect_parts(state, root, Point::new(0.0, 0.0), &mut parts);
    let rects: Vec<Rect> = parts.iter().map(|p| p.rect).collect();
    Rect::bounding(&rects)
}

pub(crate) fn complete_callbacks(state: &mut ServerState, root: &ObjectId, time_ms: u32) {
    for id in tree(state, root) {
        if let Some(s) = state.surfaces.get_mut(&id) {
            for cb in s.frame_callbacks.drain(..) {
                cb.done(time_ms);
            }
        }
    }
}

pub(crate) fn has_callbacks(state: &ServerState, root: &ObjectId) -> bool {
    tree(state, root).iter().any(|id| {
        state
            .surfaces
            .get(id)
            .is_some_and(|s| !s.frame_callbacks.is_empty())
    })
}

/// Tells a surface the preferred integer and fractional scale.
pub(crate) fn announce_scale(state: &ServerState, id: &ObjectId) {
    let (integer, fractional) = state.scales();
    if let Some(s) = state.surfaces.get(id) {
        if s.resource.version() >= 6 {
            s.resource.preferred_buffer_scale(integer);
        }
        if let Some(f) = &s.fractional {
            f.preferred_scale(fractional);
        }
    }
}

fn surface_destroyed(state: &mut ServerState, id: &ObjectId) {
    xdg_shell::surface_destroyed(state, id);
    seat::surface_destroyed(state, id);
    if let Some(queue) = state.queues.remove(id) {
        state.cancelled_waits.extend(queue.waits());
    }
    for queue in state.queues.values_mut() {
        queue.retain_mut(|transaction| {
            transaction.parts.retain(|part| part.surface != *id);
            true
        });
    }
    let Some(surface) = state.surfaces.remove(id) else {
        return;
    };
    if let Some(old) = surface.upload {
        shm::release(&old);
    }
    match (surface.image_release, surface.image) {
        (Some(point), _) => state
            .release
            .retire(ToRelease::Point(point, surface.client), state.frame_seq),
        (None, Some(old)) => state
            .release
            .retire(ToRelease::Buffer(old), state.frame_seq),
        (None, None) => {}
    }
    state.destroyed.push(surface.key);
    if let Some(parent) = &surface.parent
        && let Some(p) = state.surfaces.get_mut(parent)
    {
        let entry = StackEntry::Child(id.clone());
        p.stack.retain(|e| *e != entry);
        p.pending_stack.retain(|e| *e != entry);
        state.dirty = true;
    }
    for entry in surface.stack {
        if let StackEntry::Child(child) = entry
            && let Some(c) = state.surfaces.get_mut(&child)
        {
            c.parent = None;
        }
    }
}

pub(crate) fn take_destroyed(state: &mut ServerState) -> Vec<SurfaceKey> {
    std::mem::take(&mut state.destroyed)
}

/// Whether `ancestor` is `id` or one of its ancestors.
pub(crate) fn is_ancestor_or_self(state: &ServerState, ancestor: &ObjectId, id: &ObjectId) -> bool {
    let mut current = Some(id.clone());
    while let Some(c) = current {
        if c == *ancestor {
            return true;
        }
        current = state.surfaces.get(&c).and_then(|s| s.parent.clone());
    }
    false
}

/// Assigns `role` to a surface without one; `false` if it already has another role.
pub(crate) fn set_role(state: &mut ServerState, id: &ObjectId, role: Role) -> bool {
    match state.surfaces.get_mut(id) {
        Some(s) if s.role == Role::None || s.role == role => {
            s.role = role;
            true
        }
        _ => false,
    }
}
