//! The application: the host backend, the Wayland server for applications, the shell and the
//! renderer on one event loop.
//!
//! Frame scheduling (data-model.md): a frame is drawn only when the view or a window changed,
//! the viewer window is configured and no host frame callback is pending. Hosts stop sending
//! frame callbacks for hidden windows, so hidden windows cost no frames, and applications are
//! only told to draw when the host presented a frame that showed them (research R12).

use std::ffi::OsString;
use std::time::Instant;

use calloop::generic::Generic;
use calloop::signals::{Signal, Signals};
use calloop::{EventLoop, Interest, LoopSignal, Mode, PostAction};
use khronos_egl as egl;
use thiserror::Error;
use wayland_server::ListeningSocket;

use crate::backend::BackendEvent;
use crate::backend::wayland_host::{Cursor, HostError, WaylandHost};
use crate::canvas::{Point, Size};
use crate::render::egl::{Egl, EglError};
use crate::render::{FULLSCREEN_BACKGROUND, GlesRenderer, ImageRef, Quad, RenderError};
use crate::server::{Server, ServerError};
use crate::shell::{CursorShape, Edges, Shell, WindowId};

/// Why the application stopped with an error. Messages follow `contracts/cli.md`.
#[derive(Debug, Error)]
pub enum AppError {
    #[error(transparent)]
    Host(#[from] HostError),
    #[error(transparent)]
    Server(#[from] ServerError),
    #[error("cannot initialise OpenGL ES: {0}")]
    Init(String),
    #[error("rendering failed: {0}")]
    Render(String),
    #[error("event loop: {0}")]
    Loop(String),
}

impl From<EglError> for AppError {
    fn from(err: EglError) -> Self {
        AppError::Render(err.to_string())
    }
}

impl From<RenderError> for AppError {
    fn from(err: RenderError) -> Self {
        AppError::Render(err.to_string())
    }
}

/// Drawing state, created on the first configure.
struct Target {
    surface: egl::Surface,
    renderer: GlesRenderer,
}

/// Everything the event loop works on.
struct App {
    // Field order matters for drop: GL objects, then the EGL surface (destroyed in `Drop`),
    // then EGL, then the host connection.
    target: Option<Target>,
    egl: Egl,
    host: WaylandHost,
    server: Server,
    /// Kept for its lifetime: dropping it removes the socket and its lock file.
    listener: ListeningSocket,
    shell: Shell,
    configured: bool,
    frame_pending: bool,
    /// Windows shown in the frame the host has not presented yet.
    shown: Vec<WindowId>,
    started: Instant,
    cursor: Cursor,
    signal: LoopSignal,
    error: Option<AppError>,
}

/// Runs the nested compositor until the user quits, the window is closed or a signal arrives.
pub fn run() -> Result<(), AppError> {
    let host = WaylandHost::connect()?;
    let egl = Egl::wayland(host.connection()).map_err(|e| AppError::Init(e.to_string()))?;
    let mut server = Server::new()?;
    let (listener, name) = Server::listen()?;
    announce_socket(&name);

    let mut event_loop: EventLoop<App> =
        EventLoop::try_new().map_err(|e| AppError::Loop(e.to_string()))?;
    let handle = event_loop.handle();

    let fd = host.poll_fd().map_err(|e| AppError::Loop(e.to_string()))?;
    handle
        .insert_source(
            Generic::new(fd, Interest::READ, Mode::Level),
            |_, _, app| {
                if let Err(err) = app.host.read() {
                    app.fail(err.into());
                }
                Ok(PostAction::Continue)
            },
        )
        .map_err(|e| AppError::Loop(e.to_string()))?;

    let listen_fd = std::os::fd::AsFd::as_fd(&listener)
        .try_clone_to_owned()
        .map_err(|e| AppError::Loop(e.to_string()))?;
    handle
        .insert_source(
            Generic::new(listen_fd, Interest::READ, Mode::Level),
            |_, _, app| {
                app.accept_clients();
                Ok(PostAction::Continue)
            },
        )
        .map_err(|e| AppError::Loop(e.to_string()))?;

    let server_fd = server
        .poll_fd()
        .map_err(|e| AppError::Loop(e.to_string()))?;
    handle
        .insert_source(
            Generic::new(server_fd, Interest::READ, Mode::Level),
            |_, _, app| {
                if let Err(err) = app.server.dispatch() {
                    app.fail(AppError::Loop(format!("serving applications: {err}")));
                }
                Ok(PostAction::Continue)
            },
        )
        .map_err(|e| AppError::Loop(e.to_string()))?;

    let signals = Signals::new(&[Signal::SIGINT, Signal::SIGTERM])
        .map_err(|e| AppError::Loop(e.to_string()))?;
    handle
        .insert_source(signals, |event, _, app| {
            tracing::info!("received {:?}, exiting", event.signal());
            app.signal.stop();
        })
        .map_err(|e| AppError::Loop(e.to_string()))?;

    let mut app = App {
        target: None,
        egl,
        host,
        server,
        listener,
        shell: Shell::new(),
        configured: false,
        frame_pending: false,
        shown: Vec::new(),
        started: Instant::now(),
        cursor: Cursor::Default,
        signal: event_loop.get_signal(),
        error: None,
    };

    app.before_sleep();
    event_loop
        .run(None, &mut app, App::before_sleep)
        .map_err(|e| AppError::Loop(e.to_string()))?;

    match app.error.take() {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

fn announce_socket(name: &OsString) {
    tracing::info!(
        "applications can connect with WAYLAND_DISPLAY={}",
        name.to_string_lossy()
    );
}

impl App {
    fn fail(&mut self, err: AppError) {
        if self.error.is_none() {
            self.error = Some(err);
        }
        self.signal.stop();
    }

    fn accept_clients(&mut self) {
        loop {
            match self.listener.accept() {
                Ok(Some(stream)) => {
                    if let Err(err) = self.server.insert_client(stream) {
                        tracing::warn!("cannot serve a new application: {err}");
                    }
                }
                Ok(None) => break,
                Err(err) => {
                    tracing::warn!("accepting an application failed: {err}");
                    break;
                }
            }
        }
    }

    /// Runs after every loop dispatch, before sleeping: handle queued events, draw if needed,
    /// send requests.
    fn before_sleep(&mut self) {
        if self.error.is_some() {
            return;
        }
        if let Err(err) = self.step() {
            self.fail(err);
        }
    }

    /// Handles everything queued, draws if needed, and ends holding the host's read guard so
    /// the loop never sleeps with events left in the queue (wayland-client's integration
    /// pattern: dispatch, prepare_read, then poll).
    fn step(&mut self) -> Result<(), AppError> {
        // A guard still held here means something other than the socket woke us. It must be
        // released before any EGL call: EGL may read the socket itself, and libwayland would
        // make that read wait for ours, on this same thread.
        self.host.cancel_read();
        loop {
            self.host.dispatch_pending()?;
            self.handle_events();
            for event in self.server.take_window_events() {
                let inputs = self.shell.apply(event);
                self.server.deliver(&inputs, self.now_ms());
            }
            // After input and window events: configures and close requests (research W1).
            self.server.command(&self.shell.take_commands());
            self.update_cursor();
            let viewer = self.shell.viewer();
            self.server.set_viewer(viewer.buffer_size(), viewer.scale());
            self.upload_content()?;
            if self.configured && !self.frame_pending {
                let dirty = self.shell.take_dirty() | self.server.take_dirty();
                if dirty {
                    self.render()?;
                } else {
                    let visible = self.shell.visible();
                    if self.server.has_pending_callbacks(&visible) {
                        // Applications wait for a frame but nothing changed: ask the host for a
                        // frame callback without drawing.
                        self.host.request_empty_frame();
                        self.shown = visible;
                        self.frame_pending = true;
                    }
                }
            }
            self.host.flush()?;
            if let Err(err) = self.server.flush() {
                tracing::warn!("sending to applications failed: {err}");
            }
            // Fails when events arrived meanwhile (e.g. read by EGL during the swap): handle
            // them first.
            if self.host.prepare_read() {
                return Ok(());
            }
        }
    }

    fn handle_events(&mut self) {
        for event in self.host.take_events() {
            match event {
                BackendEvent::Input(input) => {
                    let (effect, inputs) = self.shell.handle(&input);
                    self.server.deliver(&inputs, self.now_ms());
                    if effect.quit {
                        tracing::info!("quit requested");
                        self.signal.stop();
                    }
                }
                BackendEvent::Configure { size } => {
                    self.shell.viewer_configured(size);
                    self.configured = true;
                }
                BackendEvent::Scale(scale) => {
                    self.shell.viewer_mut().set_scale(scale);
                    self.update_largest_window();
                }
                BackendEvent::FrameDone => {
                    self.frame_pending = false;
                    let shown = std::mem::take(&mut self.shown);
                    self.server.presented(self.now_ms(), &shown);
                }
                BackendEvent::CloseRequested => {
                    tracing::info!("window closed by the host");
                    self.signal.stop();
                }
                BackendEvent::Keymap(text) => self.server.set_keymap(&text),
                BackendEvent::RepeatInfo { rate, delay } => self.server.set_repeat(rate, delay),
            }
        }
    }

    /// Milliseconds since startup, wrapping: the clock of frame callbacks and input events.
    fn now_ms(&self) -> u32 {
        self.started.elapsed().as_millis() as u32
    }

    /// Tells the shell the largest window it may ask for: the GPU texture limit in logical
    /// pixels, using the scale's ceiling as applications round their buffers up (research W6).
    fn update_largest_window(&mut self) {
        let Some(target) = &self.target else {
            return;
        };
        let scale = self.shell.viewer().scale().ceil().max(1.0);
        let side = f64::from(target.renderer.max_texture_side()) / scale;
        self.shell.set_largest_window(Size::new(side, side));
    }

    fn update_cursor(&mut self) {
        let wanted = match self.shell.cursor() {
            CursorShape::Default => Cursor::Default,
            CursorShape::Grabbing => Cursor::Grabbing,
            CursorShape::Move => Cursor::Move,
            CursorShape::Resize(edges) => resize_cursor(edges),
        };
        if wanted != self.cursor {
            self.cursor = wanted;
            self.host.set_cursor(wanted);
        }
    }

    /// Copies newly committed buffers into textures and frees textures of destroyed surfaces.
    /// Needs the GL context, so it waits until the first frame created it.
    fn upload_content(&mut self) -> Result<(), AppError> {
        let Some(target) = &mut self.target else {
            return Ok(());
        };
        self.egl.make_current(Some(target.surface))?;
        let renderer = &mut target.renderer;
        self.server.upload_pending(|key, image| {
            let image = ImageRef {
                bytes: image.bytes,
                width: image.width,
                height: image.height,
                stride: image.stride,
                opaque: image.opaque,
            };
            if let Err(err) = renderer.upload(key.0, image) {
                tracing::warn!("cannot upload window content: {err}");
            }
        });
        for key in self.server.take_destroyed_surfaces() {
            renderer.remove(key.0);
        }
        Ok(())
    }

    /// The window surfaces to draw, back to front, in physical pixels of the buffer.
    fn window_quads(&self) -> (Vec<Quad>, Vec<WindowId>) {
        let viewer = self.shell.viewer();
        let camera = viewer.camera();
        let size = viewer.size();
        let scale = viewer.render_scale();
        let visible = self.shell.visible();
        let mut quads = Vec::new();
        for id in &visible {
            let Some(window) = self.shell.windows().get(*id) else {
                continue;
            };
            for part in self.server.window_parts(*id) {
                if !part.has_content {
                    continue;
                }
                // Window coordinates are canvas units relative to the window origin.
                let top_left = Point::new(
                    window.origin.x + part.rect.origin.x,
                    window.origin.y + part.rect.origin.y,
                );
                let p = camera.to_logical(top_left, size);
                let factor = camera.zoom * scale;
                quads.push(Quad {
                    key: part.key.0,
                    dst: [
                        (p.x * scale) as f32,
                        (p.y * scale) as f32,
                        (part.rect.size.w * factor) as f32,
                        (part.rect.size.h * factor) as f32,
                    ],
                    src: [
                        part.src.origin.x as f32,
                        part.src.origin.y as f32,
                        part.src.size.w as f32,
                        part.src.size.h as f32,
                    ],
                    transform: part.transform,
                });
            }
        }
        (quads, visible)
    }

    fn render(&mut self) -> Result<(), AppError> {
        if self.target.is_none() {
            // The window surface exists before the GPU limit is known; buffers are only
            // allocated at the first draw, after the resize below applies the limit.
            let window = self.host.egl_window(
                self.shell.viewer().buffer_size(),
                self.shell.viewer().size(),
            )?;
            let surface = self
                .egl
                .create_window_surface(window)
                .map_err(|e| AppError::Init(e.to_string()))?;
            self.egl
                .make_current(Some(surface))
                .map_err(|e| AppError::Init(e.to_string()))?;
            self.egl
                .disable_vsync()
                .map_err(|e| AppError::Init(e.to_string()))?;
            let renderer =
                GlesRenderer::new(self.egl.load_gl()).map_err(|e| AppError::Init(e.to_string()))?;
            self.shell
                .viewer_mut()
                .set_max_buffer_side(renderer.max_buffer_side());
            self.server
                .set_max_texture_side(renderer.max_texture_side());
            self.shell.take_dirty();
            self.target = Some(Target { surface, renderer });
            self.update_largest_window();
            // Content committed before the context existed.
            self.upload_content()?;
        }
        let buffer = self.shell.viewer().buffer_size();
        self.host.egl_window(buffer, self.shell.viewer().size())?;
        let (quads, visible) = self.window_quads();
        let Some(target) = &self.target else {
            return Ok(());
        };
        self.egl.make_current(Some(target.surface))?;
        if self.shell.fullscreen().is_some() {
            // Only the fullscreen window, on black (FR-014); `window_quads` follows `visible`.
            target
                .renderer
                .clear(FULLSCREEN_BACKGROUND, buffer.0, buffer.1);
        } else {
            let grid = self.shell.viewer().grid_params();
            target.renderer.draw(&grid, buffer.0, buffer.1);
        }
        target.renderer.draw_windows(&quads, buffer.0, buffer.1);
        self.host.request_frame();
        self.egl.swap_buffers(target.surface)?;
        self.frame_pending = true;
        self.shown = visible;
        Ok(())
    }
}

/// The host cursor for a resize from `edges`.
fn resize_cursor(edges: Edges) -> Cursor {
    match (edges.top, edges.bottom, edges.left, edges.right) {
        (true, _, true, _) => Cursor::ResizeNW,
        (true, _, _, true) => Cursor::ResizeNE,
        (_, true, true, _) => Cursor::ResizeSW,
        (_, true, _, true) => Cursor::ResizeSE,
        (true, _, _, _) => Cursor::ResizeN,
        (_, true, _, _) => Cursor::ResizeS,
        (_, _, true, _) => Cursor::ResizeW,
        (_, _, _, true) => Cursor::ResizeE,
        _ => Cursor::Default,
    }
}

impl Drop for App {
    fn drop(&mut self) {
        // No EGL call while holding the read guard (see `step`).
        self.host.cancel_read();
        if let Some(Target { surface, renderer }) = self.target.take() {
            // GL objects need the context current; then the surface goes before its window.
            let _ = self.egl.make_current(Some(surface));
            drop(renderer);
            let _ = self.egl.make_current(None);
            self.egl.destroy_surface(surface);
        }
        self.host.drop_egl_window();
    }
}
