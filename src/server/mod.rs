//! The Wayland server: RubickWay as a compositor for applications.
//!
//! `Server` owns the `wayland-server` display and all protocol state. It never touches the
//! shell: it reports windows as [`WindowEvent`]s and answers questions about their content
//! (research R7). Every value an application sends is validated; a violation disconnects only
//! that application (Principle II).

mod compositor;
mod data_device;
mod output;
mod seat;
mod shm;
mod subcompositor;
mod viewporter;
mod xdg_shell;

use std::collections::HashMap;
use std::ffi::OsString;
use std::io;
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use thiserror::Error;
use wayland_server::backend::{ClientData, ClientId, DisconnectReason, ObjectId};
use wayland_server::protocol::wl_output::WlOutput;
use wayland_server::{Display, DisplayHandle, ListeningSocket};

use crate::canvas::{Point, Rect};
use crate::shell::{ClientInput, WindowCommand, WindowEvent, WindowId};

pub use compositor::Transform;

use compositor::{Region, Surface};
use shm::{Pool, ShmBuffer};
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

/// Per-connection data: a number for the logs.
#[derive(Debug)]
pub struct ClientState {
    pub id: u64,
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {
        tracing::info!("application {} connected", self.id);
    }

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
    pub(crate) buffers: HashMap<ObjectId, ShmBuffer>,
    pub(crate) xdg_surfaces: HashMap<ObjectId, XdgSurfaceData>,
    pub(crate) outputs: Vec<WlOutput>,
    pub(crate) seat: seat::SeatState,
    /// Mapped windows and their root surface.
    pub(crate) windows: HashMap<WindowId, ObjectId>,
    pub(crate) window_events: Vec<WindowEvent>,
    /// Viewer buffer size in physical pixels and scale.
    pub(crate) viewer: ((u32, u32), f64),
    pub(crate) max_texture_side: u32,
    pub(crate) dirty: bool,
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

    pub(crate) fn new_window_id(&mut self) -> WindowId {
        self.next_window += 1;
        WindowId(self.next_window)
    }

    /// The viewer scale applications should draw at, as an integer (ceil) and in 120ths.
    pub(crate) fn scales(&self) -> (i32, u32) {
        let scale = self.viewer.1;
        let integer = scale.ceil().clamp(1.0, 64.0) as i32;
        let fractional = (scale * 120.0).round().clamp(120.0, 64.0 * 120.0) as u32;
        (integer, fractional)
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
        output::create_global(&dh);
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
                xdg_surfaces: HashMap::new(),
                outputs: Vec::new(),
                seat: seat::SeatState::default(),
                windows: HashMap::new(),
                window_events: Vec::new(),
                viewer: ((1280, 800), 1.0),
                max_texture_side: i32::MAX as u32,
                dirty: false,
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

    /// Whether anything visible changed since the last call.
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.state.dirty)
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

    /// Surfaces destroyed since the last call, whose textures can be freed.
    pub fn take_destroyed_surfaces(&mut self) -> Vec<SurfaceKey> {
        compositor::take_destroyed(&mut self.state)
    }

    /// The host presented a frame that showed `windows`: complete their frame callbacks.
    pub fn presented(&mut self, time_ms: u32, windows: &[WindowId]) {
        for id in windows {
            if let Some(root) = self.state.windows.get(id).cloned() {
                compositor::complete_callbacks(&mut self.state, &root, time_ms);
            }
        }
    }

    /// Whether any surface of `windows` waits for a frame callback.
    pub fn has_pending_callbacks(&self, windows: &[WindowId]) -> bool {
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

    /// The host keymap (xkb text), shared with every application's keyboard.
    pub fn set_keymap(&mut self, text: &str) {
        seat::set_keymap(&mut self.state, text);
    }

    /// The host's key repeat rate (keys per second) and delay (ms).
    pub fn set_repeat(&mut self, rate: i32, delay: i32) {
        seat::set_repeat(&mut self.state, rate, delay);
    }

    /// The largest buffer side the renderer can show; larger buffers disconnect their
    /// application (misbehaving case 7).
    pub fn set_max_texture_side(&mut self, side: u32) {
        self.state.max_texture_side = side.max(1);
    }

    /// The viewer's buffer size (physical pixels) and scale, announced to applications.
    pub fn set_viewer(&mut self, buffer: (u32, u32), scale: f64) {
        if !scale.is_finite() || scale <= 0.0 || self.state.viewer == (buffer, scale) {
            return;
        }
        let old_scales = self.state.scales();
        self.state.viewer = (buffer, scale);
        output::announce_all(&self.state);
        if self.state.scales() != old_scales {
            compositor::announce_scale_all(&self.state);
        }
    }
}
