//! The application: the host backend, the viewer and the renderer on one event loop.
//!
//! Frame scheduling (data-model.md): a frame is drawn only when the viewer is dirty, the
//! window is configured and no frame callback is pending. Hosts stop sending frame callbacks
//! for hidden windows, so hidden windows cost no frames.

use calloop::generic::Generic;
use calloop::signals::{Signal, Signals};
use calloop::{EventLoop, Interest, LoopSignal, Mode, PostAction};
use khronos_egl as egl;
use thiserror::Error;

use crate::backend::BackendEvent;
use crate::backend::wayland_host::{Cursor, HostError, WaylandHost};
use crate::render::egl::{Egl, EglError};
use crate::render::{GlesRenderer, RenderError};
use crate::viewer::Viewer;

/// Why the application stopped with an error. Messages follow `contracts/cli.md`.
#[derive(Debug, Error)]
pub enum AppError {
    #[error(transparent)]
    Host(#[from] HostError),
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
    viewer: Viewer,
    configured: bool,
    frame_pending: bool,
    cursor: Cursor,
    signal: LoopSignal,
    error: Option<AppError>,
}

/// Runs the nested viewer until the user quits, the window is closed or a signal arrives.
pub fn run() -> Result<(), AppError> {
    let host = WaylandHost::connect()?;
    let egl = Egl::wayland(host.connection()).map_err(|e| AppError::Init(e.to_string()))?;

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
        viewer: Viewer::new(),
        configured: false,
        frame_pending: false,
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

impl App {
    fn fail(&mut self, err: AppError) {
        if self.error.is_none() {
            self.error = Some(err);
        }
        self.signal.stop();
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
            self.update_cursor();
            if self.configured && !self.frame_pending && self.viewer.take_dirty() {
                self.render()?;
            }
            self.host.flush()?;
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
                    if self.viewer.handle(&input).quit {
                        tracing::info!("quit requested");
                        self.signal.stop();
                    }
                }
                BackendEvent::Configure { size } => {
                    self.viewer.configure(size);
                    self.configured = true;
                }
                BackendEvent::Scale(scale) => self.viewer.set_scale(scale),
                BackendEvent::FrameDone => self.frame_pending = false,
                BackendEvent::CloseRequested => {
                    tracing::info!("window closed by the host");
                    self.signal.stop();
                }
            }
        }
    }

    fn update_cursor(&mut self) {
        let wanted = if self.viewer.is_panning() {
            Cursor::Grabbing
        } else {
            Cursor::Default
        };
        if wanted != self.cursor {
            self.cursor = wanted;
            self.host.set_cursor(wanted);
        }
    }

    fn render(&mut self) -> Result<(), AppError> {
        if self.target.is_none() {
            // The window surface exists before the GPU limit is known; buffers are only
            // allocated at the first draw, after the resize below applies the limit.
            let window = self
                .host
                .egl_window(self.viewer.buffer_size(), self.viewer.size())?;
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
            self.viewer.set_max_buffer_side(renderer.max_buffer_side());
            self.viewer.take_dirty();
            self.target = Some(Target { surface, renderer });
        }
        let buffer = self.viewer.buffer_size();
        self.host.egl_window(buffer, self.viewer.size())?;
        let Some(target) = &self.target else {
            return Ok(());
        };
        self.egl.make_current(Some(target.surface))?;
        target
            .renderer
            .draw(&self.viewer.grid_params(), buffer.0, buffer.1);
        self.host.request_frame();
        self.egl.swap_buffers(target.surface)?;
        self.frame_pending = true;
        Ok(())
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
