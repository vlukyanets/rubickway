//! The nested backend: RubickWay as a client of the host Wayland session.
//!
//! This layer stays thin (constitution III): it binds the host globals, owns the viewer window
//! and the seat, and turns host events into `BackendEvent`s. It is verified manually through
//! the feature's `quickstart.md`.

mod seat;
mod window;

use std::io;
use std::os::fd::{AsFd, OwnedFd};

use thiserror::Error;
use wayland_client::backend::{ReadEventsGuard, WaylandError};
use wayland_client::globals::{BindError, GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_compositor, wl_registry};
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle, delegate_noop};
use wayland_egl::WlEglSurface;
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_manager_v1::WpCursorShapeManagerV1;
use wayland_protocols::wp::fractional_scale::v1::client::wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1;
use wayland_protocols::wp::pointer_gestures::zv1::client::zwp_pointer_gestures_v1::ZwpPointerGesturesV1;
use wayland_protocols::wp::viewporter::client::wp_viewporter::WpViewporter;
use wayland_protocols::xdg::decoration::zv1::client::zxdg_decoration_manager_v1::ZxdgDecorationManagerV1;
use wayland_protocols::xdg::shell::client::xdg_wm_base::{self, XdgWmBase};

use crate::backend::BackendEvent;
use crate::canvas::Size;

pub use seat::Cursor;
use seat::Seat;
use window::Window;

/// Errors from the host connection. Messages follow `contracts/cli.md`.
#[derive(Debug, Error)]
pub enum HostError {
    #[error("cannot connect to a Wayland session: {0}")]
    Connect(String),
    #[error("the Wayland session does not provide {0}")]
    MissingGlobal(&'static str),
    #[error("lost the connection to the Wayland session: {0}")]
    Lost(String),
    #[error("cannot create the viewer window: {0}")]
    Window(String),
}

fn lost(err: impl std::fmt::Display) -> HostError {
    HostError::Lost(err.to_string())
}

/// Optional host globals, bound when present.
pub(crate) struct Globals {
    pub viewporter: Option<WpViewporter>,
    pub fractional_scale: Option<WpFractionalScaleManagerV1>,
    pub decoration: Option<ZxdgDecorationManagerV1>,
    pub pointer_gestures: Option<ZwpPointerGesturesV1>,
    pub cursor_shape: Option<WpCursorShapeManagerV1>,
}

/// State that the host's event handlers write into.
pub struct HostState {
    events: Vec<BackendEvent>,
    window: Window,
    seat: Seat,
    globals: Globals,
}

impl HostState {
    pub(crate) fn push(&mut self, event: BackendEvent) {
        self.events.push(event);
    }
}

/// The connection to the host and the viewer window in it.
pub struct WaylandHost {
    connection: Connection,
    queue: EventQueue<HostState>,
    state: HostState,
    /// Held while the event loop sleeps (see [`WaylandHost::prepare_read`]).
    read_guard: Option<ReadEventsGuard>,
}

fn optional<I>(result: Result<I, BindError>, name: &str) -> Option<I> {
    match result {
        Ok(proxy) => Some(proxy),
        Err(err) => {
            tracing::info!("host global {name} not used: {err}");
            None
        }
    }
}

impl WaylandHost {
    /// Connects to the session in `WAYLAND_DISPLAY`, binds globals (research R2) and creates
    /// the viewer window.
    pub fn connect() -> Result<WaylandHost, HostError> {
        let connection =
            Connection::connect_to_env().map_err(|e| HostError::Connect(e.to_string()))?;
        let (globals, queue) = registry_queue_init::<HostState>(&connection)
            .map_err(|e| HostError::Connect(e.to_string()))?;
        let qh = queue.handle();

        let compositor: wl_compositor::WlCompositor = globals
            .bind(&qh, 4..=6, ())
            .map_err(|_| HostError::MissingGlobal("wl_compositor (version 4 or later)"))?;
        let wm_base: XdgWmBase = globals
            .bind(&qh, 1..=5, ())
            .map_err(|_| HostError::MissingGlobal("xdg_wm_base"))?;

        let viewporter = optional(globals.bind(&qh, 1..=1, ()), "wp_viewporter");
        // Fractional scale is only usable together with a viewport.
        let fractional_scale = viewporter.as_ref().and_then(|_| {
            optional(
                globals.bind(&qh, 1..=1, ()),
                "wp_fractional_scale_manager_v1",
            )
        });
        let state_globals = Globals {
            viewporter,
            fractional_scale,
            decoration: optional(globals.bind(&qh, 1..=1, ()), "zxdg_decoration_manager_v1"),
            pointer_gestures: optional(globals.bind(&qh, 1..=1, ()), "zwp_pointer_gestures_v1"),
            cursor_shape: optional(globals.bind(&qh, 1..=1, ()), "wp_cursor_shape_manager_v1"),
        };
        let seat_proxy = optional(globals.bind(&qh, 5..=8, ()), "wl_seat");
        if seat_proxy.is_none() {
            tracing::warn!("the Wayland session has no seat: the viewer gets no input");
        }

        let window = Window::new(&compositor, &wm_base, &state_globals, &qh);
        let state = HostState {
            events: Vec::new(),
            window,
            seat: Seat::new(seat_proxy),
            globals: state_globals,
        };
        let host = WaylandHost {
            connection,
            queue,
            state,
            read_guard: None,
        };
        host.flush()?;
        Ok(host)
    }

    /// The host connection (EGL needs its display pointer).
    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    /// A duplicate of the connection fd, for the event loop to poll.
    pub fn poll_fd(&self) -> io::Result<OwnedFd> {
        self.connection.as_fd().try_clone_to_owned()
    }

    /// Announces the intent to read before the event loop sleeps. Returns `false` when events
    /// are already queued: dispatch them and try again. On `true` the guard is held until
    /// [`WaylandHost::read`] or [`WaylandHost::cancel_read`]. No EGL call may happen while it is
    /// held: EGL may read the socket, and libwayland would make that read wait for ours.
    pub fn prepare_read(&mut self) -> bool {
        if self.read_guard.is_none() {
            self.read_guard = self.queue.prepare_read();
        }
        self.read_guard.is_some()
    }

    /// Drops a held read guard (the loop woke up for another reason).
    pub fn cancel_read(&mut self) {
        self.read_guard = None;
    }

    /// Reads available events from the socket and dispatches them. Call when the fd is
    /// readable.
    pub fn read(&mut self) -> Result<(), HostError> {
        let guard = match self.read_guard.take() {
            Some(guard) => Some(guard),
            None => self.queue.prepare_read(),
        };
        if let Some(guard) = guard {
            match guard.read() {
                Ok(_) => {}
                Err(WaylandError::Io(e)) if e.kind() == io::ErrorKind::WouldBlock => {}
                Err(e) => return Err(lost(e)),
            }
        }
        self.dispatch_pending()
    }

    /// Dispatches events already read from the socket.
    pub fn dispatch_pending(&mut self) -> Result<(), HostError> {
        self.queue
            .dispatch_pending(&mut self.state)
            .map(|_| ())
            .map_err(lost)
    }

    /// Sends buffered requests. A full socket is not an error; the rest goes out later.
    pub fn flush(&self) -> Result<(), HostError> {
        match self.connection.flush() {
            Ok(()) => Ok(()),
            Err(WaylandError::Io(e)) if e.kind() == io::ErrorKind::WouldBlock => Ok(()),
            Err(e) => Err(lost(e)),
        }
    }

    /// Takes the events collected since the last call.
    pub fn take_events(&mut self) -> Vec<BackendEvent> {
        std::mem::take(&mut self.state.events)
    }

    /// The `wl_egl_window` of the viewer, created on first use and resized to `buffer` with
    /// the surface showing it at `logical` size.
    pub fn egl_window(
        &mut self,
        buffer: (u32, u32),
        logical: Size,
    ) -> Result<&WlEglSurface, HostError> {
        self.state.window.egl_window(buffer, logical)
    }

    /// Asks for a frame callback; the next commit (EGL swap) sends it.
    pub fn request_frame(&mut self) {
        let qh = self.queue.handle();
        self.state.window.request_frame(&qh);
    }

    /// Asks for a frame callback without drawing: an empty commit (research R12, used when
    /// only applications wait for a frame).
    pub fn request_empty_frame(&mut self) {
        let qh = self.queue.handle();
        self.state.window.request_frame(&qh);
        self.state.window.commit();
    }

    /// Sets the pointer cursor shown over the viewer.
    pub fn set_cursor(&mut self, cursor: Cursor) {
        self.state.seat.set_cursor(cursor);
    }

    /// Releases the EGL window before the surface goes away.
    pub fn drop_egl_window(&mut self) {
        self.state.window.drop_egl_window();
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for HostState {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // Globals appearing or disappearing after startup are not used yet.
    }
}

impl Dispatch<XdgWmBase, ()> for HostState {
    fn event(
        _: &mut Self,
        wm_base: &XdgWmBase,
        event: xdg_wm_base::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_wm_base::Event::Ping { serial } = event {
            wm_base.pong(serial);
        }
    }
}

delegate_noop!(HostState: ignore wl_compositor::WlCompositor);
delegate_noop!(HostState: ignore WpViewporter);
delegate_noop!(HostState: ignore WpFractionalScaleManagerV1);
delegate_noop!(HostState: ignore ZxdgDecorationManagerV1);
delegate_noop!(HostState: ignore ZwpPointerGesturesV1);
delegate_noop!(HostState: ignore WpCursorShapeManagerV1);
