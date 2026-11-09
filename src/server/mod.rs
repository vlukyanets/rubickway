//! The Wayland server: RubickWay as a compositor for applications.
//!
//! `Server` owns the `wayland-server` display and all protocol state. It never touches the
//! shell: it reports windows as [`WindowEvent`]s and answers questions about their content
//! (research R7). Every value an application sends is validated; a violation disconnects only
//! that application (Principle II).

mod commit_queue;
mod compositor;
mod data_device;
mod descriptors;
mod dmabuf;
mod output;
mod release;
mod seat;
mod shm;
mod subcompositor;
mod syncobj;
mod viewporter;
mod xdg_shell;

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::io;
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use thiserror::Error;
use wayland_server::backend::{ClientData, ClientId, DisconnectReason, ObjectId};
use wayland_server::{Display, DisplayHandle, ListeningSocket};

use crate::canvas::{Point, Rect};
use crate::render::formats::FormatPair;
use crate::shell::{ClientInput, WindowCommand, WindowEvent, WindowId};

pub use commit_queue::WaitId;
pub use compositor::Transform;
pub use dmabuf::{BufferKey, GpuImage, GpuPlane, ImageFiles, ShownImage, Wait};
pub use output::OutputDesc;
pub use syncobj::{TimelineId, Timelines};

use commit_queue::CommitQueue;
use compositor::{Region, Surface, Transaction};
use dmabuf::{BufferParams, DmaBufFiles, GpuOffer, PendingImport};
use release::ReleaseTracker;
use shm::{BufferKind, Pool};
use xdg_shell::XdgSurfaceData;

/// Largest number of bytes queued for one application before it is disconnected (research
/// R8): an application that does not read its messages cannot pin more memory than this.
pub const MAX_CLIENT_BUFFER: usize = 1 << 20;

/// Errors starting the server.
#[derive(Debug, Error)]
pub enum ServerError {
    #[error("cannot create the Wayland display: {0}")]
    Display(String),
    #[error("cannot open a socket for applications: {0}")]
    Socket(String),
}

/// Identifies a surface's content for the renderer (its texture).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SurfaceKey(pub u64);

/// What changed on screen (`Server::take_damage`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Damage {
    /// Windows or subsurfaces came or went: redraw everything.
    pub all: bool,
    /// Windows whose content changed, in id order.
    pub windows: Vec<WindowId>,
}

/// One surface of a window, ready to draw.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfacePart {
    pub key: SurfaceKey,
    /// Where the surface is drawn, in window coordinates.
    pub rect: Rect,
    /// The part of the buffer shown, in normalised texture coordinates of the transformed
    /// buffer (`0..1`); the whole buffer without a viewport source.
    pub src: Rect,
    pub transform: Transform,
    /// Whether the content has been uploaded at least once.
    pub has_content: bool,
}

/// Pixels of a committed buffer, borrowed while RubickWay copies them.
#[derive(Debug)]
pub struct ShmImage<'a> {
    pub bytes: &'a [u8],
    pub width: u32,
    pub height: u32,
    /// Bytes per row.
    pub stride: u32,
    /// `xrgb8888`: the alpha byte is to be ignored.
    pub opaque: bool,
}

/// ` (pid 1234, kitty)` for the connect log line: the peer's pid from the socket and its
/// command name from `/proc/<pid>/comm` (an in-memory procfs file, read at once). For the log
/// only: both can be spoofed.
fn describe_process(client: &wayland_server::Client, dh: &DisplayHandle) -> String {
    let Ok(creds) = client.get_credentials(dh) else {
        return String::new();
    };
    let comm = std::fs::read_to_string(format!("/proc/{}/comm", creds.pid))
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "?".into());
    format!(" (pid {}, {comm})", creds.pid)
}

/// Per-connection data: a number for the logs.
#[derive(Debug)]
pub struct ClientState {
    pub id: u64,
}

impl ClientData for ClientState {
    // The connect line is logged by `Server::insert_client`, which knows the process.
    fn initialized(&self, _client_id: ClientId) {}

    fn disconnected(&self, _client_id: ClientId, reason: DisconnectReason) {
        match reason {
            // wayland-backend also reports a client killed for a full outgoing buffer (it
            // stopped reading its messages) as `ConnectionClosed`, so name both causes.
            DisconnectReason::ConnectionClosed => {
                tracing::info!(
                    "application {} disconnected: connection closed or messages unread",
                    self.id
                );
            }
            DisconnectReason::ProtocolError(err) => {
                tracing::info!(
                    "application {} disconnected: protocol error {} on {}@{}: {}",
                    self.id,
                    err.code,
                    err.object_interface,
                    err.object_id,
                    err.message
                );
            }
        }
    }
}

/// Protocol state that request handlers work on.
pub struct ServerState {
    pub(crate) dh: DisplayHandle,
    pub(crate) surfaces: HashMap<ObjectId, Surface>,
    pub(crate) regions: HashMap<ObjectId, Region>,
    pub(crate) pools: HashMap<ObjectId, Pool>,
    pub(crate) buffers: HashMap<ObjectId, BufferKind>,
    /// Commits waiting for their images, per commit root (research G7).
    pub(crate) queues: HashMap<ObjectId, CommitQueue<Transaction>>,
    /// Files to watch for commits that wait, not handed to the event loop yet (research G6).
    pub(crate) waits: Vec<Wait>,
    /// Waits nothing depends on any more.
    pub(crate) cancelled_waits: Vec<WaitId>,
    /// Timelines for explicit synchronisation; `None`: it is not offered.
    pub(crate) timelines: Option<Rc<dyn Timelines>>,
    /// Imported timelines, by their objects.
    pub(crate) timeline_objects: HashMap<ObjectId, Rc<syncobj::Timeline>>,
    /// The GPU told to applications; `None`: GPU images are not offered (FR-014).
    pub(crate) gpu_offer: Option<GpuOffer>,
    pub(crate) image_files: Box<dyn ImageFiles>,
    pub(crate) buffer_params: HashMap<ObjectId, BufferParams>,
    pub(crate) pending_imports: Vec<PendingImport>,
    /// What surfaces show from now on, for the renderer.
    pub(crate) shown_images: Vec<ShownImage>,
    /// Images whose `wl_buffer` is gone, for the renderer.
    pub(crate) destroyed_images: Vec<BufferKey>,
    /// Replaced images waiting to go back (research G9).
    pub(crate) release: ReleaseTracker<syncobj::ToRelease>,
    /// The newest frame drawn so far.
    pub(crate) frame_seq: u64,
    /// What was logged already, so a redrawing application cannot flood the log (FR-015).
    pub(crate) logged_failures: HashSet<(u64, String)>,
    pub(crate) descriptors: descriptors::DescriptorWatch,
    pub(crate) next_buffer: u64,
    next_wait: u64,
    pub(crate) xdg_surfaces: HashMap<ObjectId, XdgSurfaceData>,
    pub(crate) outputs: Vec<output::OutputEntry>,
    pub(crate) seat: seat::SeatState,
    /// Mapped windows and their root surface.
    pub(crate) windows: HashMap<WindowId, ObjectId>,
    /// The displays each window was last told it is on (`wl_surface.enter`).
    pub(crate) window_outputs: HashMap<WindowId, Vec<u32>>,
    pub(crate) window_events: Vec<WindowEvent>,
    pub(crate) max_texture_side: u32,
    /// A window or subsurface came or went: every display needs a new frame.
    pub(crate) dirty: bool,
    /// Surfaces with new content since the last `take_damage`.
    pub(crate) damaged: Vec<ObjectId>,
    /// RubickWay is on another console: nothing is shown, frame callbacks wait (S12).
    paused: bool,
    /// Keys of destroyed surfaces whose textures can be freed.
    pub(crate) destroyed: Vec<SurfaceKey>,
    next_window: u64,
    next_surface: u64,
    next_serial: u32,
}

impl ServerState {
    pub(crate) fn next_serial(&mut self) -> u32 {
        self.next_serial = self.next_serial.wrapping_add(1);
        self.next_serial
    }

    pub(crate) fn new_surface_key(&mut self) -> SurfaceKey {
        self.next_surface += 1;
        SurfaceKey(self.next_surface)
    }

    pub(crate) fn new_wait(&mut self) -> WaitId {
        self.next_wait += 1;
        WaitId(self.next_wait)
    }

    pub(crate) fn new_window_id(&mut self) -> WindowId {
        self.next_window += 1;
        WindowId(self.next_window)
    }

    /// The scale applications should draw at, as an integer and in 120ths: 1 on every display
    /// (contracts/protocols.md); a per-display scale is a later feature.
    pub(crate) fn scales(&self) -> (i32, u32) {
        (1, 120)
    }
}

/// The Wayland display and its state.
pub struct Server {
    display: Display<ServerState>,
    state: ServerState,
}

static NEXT_CLIENT: AtomicU64 = AtomicU64::new(1);

impl Server {
    /// A display with all globals of contracts/protocols.md and no clients yet.
    pub fn new() -> Result<Server, ServerError> {
        let display = Display::new().map_err(|e| ServerError::Display(e.to_string()))?;
        let dh = display.handle();
        dh.set_default_max_buffer_size(MAX_CLIENT_BUFFER);
        compositor::create_globals(&dh);
        subcompositor::create_global(&dh);
        shm::create_global(&dh);
        xdg_shell::create_global(&dh);
        seat::create_global(&dh);
        data_device::create_global(&dh);
        viewporter::create_globals(&dh);
        Ok(Server {
            display,
            state: ServerState {
                dh,
                surfaces: HashMap::new(),
                regions: HashMap::new(),
                pools: HashMap::new(),
                buffers: HashMap::new(),
                queues: HashMap::new(),
                waits: Vec::new(),
                cancelled_waits: Vec::new(),
                timelines: None,
                timeline_objects: HashMap::new(),
                gpu_offer: None,
                image_files: Box::new(DmaBufFiles),
                buffer_params: HashMap::new(),
                pending_imports: Vec::new(),
                shown_images: Vec::new(),
                destroyed_images: Vec::new(),
                release: ReleaseTracker::default(),
                frame_seq: 0,
                logged_failures: HashSet::new(),
                descriptors: descriptors::DescriptorWatch::default(),
                next_buffer: 0,
                next_wait: 0,
                xdg_surfaces: HashMap::new(),
                outputs: Vec::new(),
                seat: seat::SeatState::default(),
                windows: HashMap::new(),
                window_outputs: HashMap::new(),
                window_events: Vec::new(),
                max_texture_side: i32::MAX as u32,
                dirty: false,
                damaged: Vec::new(),
                paused: false,
                destroyed: Vec::new(),
                next_window: 0,
                next_surface: 0,
                next_serial: 0,
            },
        })
    }

    /// Opens the first free `wayland-N` socket in `$XDG_RUNTIME_DIR` (research R1).
    pub fn listen() -> Result<(ListeningSocket, OsString), ServerError> {
        let socket = ListeningSocket::bind_auto("wayland", 1..33)
            .map_err(|e| ServerError::Socket(e.to_string()))?;
        let name = socket
            .socket_name()
            .map(OsString::from)
            .ok_or_else(|| ServerError::Socket("the socket has no name".to_string()))?;
        Ok((socket, name))
    }

    /// Serves an application connected through `stream`.
    pub fn insert_client(&mut self, stream: UnixStream) -> io::Result<ClientId> {
        let id = NEXT_CLIENT.fetch_add(1, Ordering::Relaxed);
        let client = self
            .state
            .dh
            .insert_client(stream, Arc::new(ClientState { id }))?;
        tracing::info!(
            "application {id} connected{}",
            describe_process(&client, &self.state.dh)
        );
        Ok(client.id())
    }

    /// Reads and handles the requests of all applications.
    pub fn dispatch(&mut self) -> io::Result<()> {
        self.display.dispatch_clients(&mut self.state).map(|_| ())
    }

    /// Sends queued events to all applications without blocking; an application whose buffer
    /// overflows is disconnected by the backend.
    pub fn flush(&mut self) -> io::Result<()> {
        match self.display.flush_clients() {
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => Ok(()),
            other => other,
        }
    }

    /// A duplicate of the fd the event loop polls for client requests.
    pub fn poll_fd(&mut self) -> io::Result<OwnedFd> {
        self.display
            .backend()
            .poll_fd()
            .as_fd()
            .try_clone_to_owned()
    }

    /// Window changes since the last call, in order.
    pub fn take_window_events(&mut self) -> Vec<WindowEvent> {
        std::mem::take(&mut self.state.window_events)
    }

    /// What changed on screen since the last call: everything (a window or subsurface came or
    /// went), or the content of these windows only.
    pub fn take_damage(&mut self) -> Damage {
        let surfaces = std::mem::take(&mut self.state.damaged);
        let mut windows: Vec<WindowId> = Vec::new();
        for (window, root) in &self.state.windows {
            let tree = compositor::tree(&self.state, root);
            if surfaces.iter().any(|s| tree.contains(s)) {
                windows.push(*window);
            }
        }
        windows.sort_unstable();
        Damage {
            all: std::mem::take(&mut self.state.dirty),
            windows,
        }
    }

    /// The surfaces of a window in drawing order (back to front), in window coordinates. Empty
    /// for unknown windows.
    pub fn window_parts(&self, id: WindowId) -> Vec<SurfacePart> {
        let Some(root) = self.state.windows.get(&id) else {
            return Vec::new();
        };
        let origin = self.window_origin(root);
        let mut parts = Vec::new();
        compositor::collect_parts(&self.state, root, origin, &mut parts);
        parts
    }

    /// Window coordinates of a window's root surface origin: minus the geometry offset.
    fn window_origin(&self, root: &ObjectId) -> Point {
        let geometry = xdg_shell::geometry_of(&self.state, root);
        Point::new(-geometry.origin.x, -geometry.origin.y)
    }

    /// Copies every newly committed buffer through `upload` (with the GL context current),
    /// then releases the buffers. An application whose pool file shrank meanwhile is
    /// disconnected (`invalid_fd`).
    pub fn upload_pending(&mut self, upload: impl FnMut(SurfaceKey, ShmImage<'_>)) {
        shm::upload_pending(&mut self.state, upload);
    }

    /// Offers GPU images to applications (FR-001, FR-002): the GPU's device number and the
    /// formats and layouts it can read. Call before applications connect; without this call GPU
    /// images are not offered at all (FR-014).
    pub fn set_gpu_offer(&mut self, main_device: u64, pairs: Vec<FormatPair>) -> io::Result<()> {
        dmabuf::set_offer(&mut self.state, main_device, pairs)
    }

    /// Offers explicit synchronisation through `timelines` (research G6). Only with an offer of GPU
    /// images, and when the GPU has timelines.
    pub fn set_timelines(&mut self, timelines: Rc<dyn Timelines>) {
        syncobj::set_timelines(&mut self.state, timelines);
    }

    /// How client files are recognised as GPU images.
    pub fn set_image_files(&mut self, files: Box<dyn ImageFiles>) {
        self.state.image_files = files;
    }

    /// Imports the images applications handed over since the last call, through `import` (with
    /// the GL context current), answers them, and applies the commits that waited for them. Call
    /// right after reading requests, before taking window events and drawing (research G7).
    pub fn process_imports(&mut self, import: impl FnMut(GpuImage<'_>) -> Result<(), String>) {
        dmabuf::process_imports(&mut self.state, import);
    }

    /// Notes a failure to get a file descriptor, telling the log once when they run out. Returns
    /// whether `err` was that: anything else the caller still has to tell (FR-015).
    pub fn note_descriptor_failure(&mut self, err: &io::Error) -> bool {
        descriptors::note_failure(&mut self.state, err)
    }

    /// Tells the log when descriptors that ran out are available again. Call on each pass of the loop.
    pub fn check_descriptors(&mut self) {
        descriptors::check(&mut self.state);
    }

    /// Whether file descriptors have run out.
    pub fn out_of_descriptors(&self) -> bool {
        self.state.descriptors.is_exhausted()
    }

    /// The number of the application holding the most GPU images, and how many.
    pub fn application_holding_most_images(&self) -> Option<(u64, usize)> {
        descriptors::most_images(&self.state)
    }

    /// Images and points to wait for since the last call (research G6). Report each that becomes
    /// readable through [`Server::wait_done`], and never wait for one on the event loop.
    pub fn take_waits(&mut self) -> Vec<Wait> {
        std::mem::take(&mut self.state.waits)
    }

    /// Waits nothing depends on any more, since their surface is gone: stop watching their files.
    pub fn take_cancelled_waits(&mut self) -> Vec<WaitId> {
        std::mem::take(&mut self.state.cancelled_waits)
    }

    /// A wait's file became readable: the commits waiting only for it apply. One whose commit is
    /// gone is ignored.
    pub fn wait_done(&mut self, wait: WaitId) {
        compositor::wait_done(&mut self.state, wait);
    }

    /// What surfaces show from now on, in order.
    pub fn take_shown_images(&mut self) -> Vec<ShownImage> {
        std::mem::take(&mut self.state.shown_images)
    }

    /// Images whose `wl_buffer` was destroyed since the last call.
    pub fn take_destroyed_images(&mut self) -> Vec<BufferKey> {
        std::mem::take(&mut self.state.destroyed_images)
    }

    /// A frame was drawn; returns its sequence number, for [`Server::release_images`].
    pub fn frame_drawn(&mut self) -> u64 {
        self.state.frame_seq += 1;
        self.state.frame_seq
    }

    /// Gives back replaced images no frame in flight can read (research G9). `oldest_in_flight`
    /// is the oldest frame still waiting for its flip on any display.
    pub fn release_images(&mut self, oldest_in_flight: Option<u64>) {
        for entry in self.state.release.release_ready(oldest_in_flight) {
            syncobj::release(&self.state, entry);
        }
    }

    /// Surfaces destroyed since the last call, whose textures can be freed.
    pub fn take_destroyed_surfaces(&mut self) -> Vec<SurfaceKey> {
        compositor::take_destroyed(&mut self.state)
    }

    /// A frame that showed `windows` reached the screen: complete their frame callbacks. Does
    /// nothing while paused (nothing is shown then).
    pub fn presented(&mut self, time_ms: u32, windows: &[WindowId]) {
        if self.state.paused {
            return;
        }
        for id in windows {
            if let Some(root) = self.state.windows.get(id).cloned() {
                compositor::complete_callbacks(&mut self.state, &root, time_ms);
            }
        }
    }

    /// Pauses or resumes showing windows (console switched away and back, research S12):
    /// while paused, frame callbacks wait; content and window changes are still accepted, and
    /// only the latest content of each surface is kept.
    pub fn set_paused(&mut self, paused: bool) {
        self.state.paused = paused;
    }

    /// Whether any surface of `windows` waits for a frame callback (never while paused).
    pub fn has_pending_callbacks(&self, windows: &[WindowId]) -> bool {
        if self.state.paused {
            return false;
        }
        windows.iter().any(|id| {
            self.state
                .windows
                .get(id)
                .is_some_and(|root| compositor::has_callbacks(&self.state, root))
        })
    }

    /// Applies the shell's window commands: configures and close requests (research W1).
    pub fn command(&mut self, commands: &[WindowCommand]) {
        xdg_shell::command(&mut self.state, commands);
    }

    /// Delivers the shell's input decisions to the applications. `time_ms` is the
    /// compositor clock.
    pub fn deliver(&mut self, inputs: &[ClientInput], time_ms: u32) {
        seat::deliver(&mut self.state, inputs, time_ms);
    }

    /// The configured keymap (xkb text), shared with every application's keyboard.
    pub fn set_keymap(&mut self, text: &str) {
        seat::set_keymap(&mut self.state, text);
    }

    /// The key repeat rate (keys per second) and delay (ms) applications use.
    pub fn set_repeat(&mut self, rate: i32, delay: i32) {
        seat::set_repeat(&mut self.state, rate, delay);
    }

    /// The largest buffer side the renderer can show; larger buffers disconnect their
    /// application (misbehaving case 7).
    pub fn set_max_texture_side(&mut self, side: u32) {
        self.state.max_texture_side = side.max(1);
    }

    /// Adds (or re-describes) the output of display `id` at its position in the row (FR-015).
    pub fn add_output(&mut self, id: u32, desc: OutputDesc) {
        output::add(&mut self.state, id, desc);
    }

    /// Removes the output of display `id`; windows on it get a `leave`, and clients' objects
    /// stay valid and inert.
    pub fn remove_output(&mut self, id: u32) {
        output::remove(&mut self.state, id);
    }

    /// The displays showing `window` now: its surface gets `enter` for each new one and
    /// `leave` for each one it is no longer on (FR-018, contracts/protocols.md).
    pub fn set_window_outputs(&mut self, window: WindowId, ids: &[u32]) {
        // Windows keep the outputs they were on while paused; the next call after the resume
        // sends the difference (FR-015).
        if self.state.paused {
            return;
        }
        output::set_window_outputs(&mut self.state, window, ids);
    }
}
