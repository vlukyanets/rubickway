//! The standalone application: the DRM backend, the Wayland server, the shell and the
//! renderer on one event loop (research S2, S7).
//!
//! Frame scheduling per display (data-model "FrameState"): a display is drawn when its viewer
//! or the windows changed and no page flip is pending for it; the flip reports the windows it
//! showed to the server, which completes their frame callbacks. When applications wait for a
//! frame and nothing changed, an estimated-vblank timer completes them without drawing.

use std::collections::HashMap;
use std::ffi::OsString;
use std::os::fd::OwnedFd;
use std::time::{Duration, Instant};

use calloop::channel::{self, Sender, channel};
use calloop::generic::Generic;
use calloop::signals::{Signal, Signals};
use calloop::timer::{TimeoutAction, Timer};
use calloop::{EventLoop, Interest, LoopHandle, LoopSignal, Mode, PostAction};
use wayland_server::ListeningSocket;

use crate::backend::drm::{DrmBackend, DrmError};
use crate::backend::watchdog;
use crate::backend::{BackendEvent, OutputInfo};
use crate::canvas::{Point, ViewerId};
use crate::config::{Config, config_path};
use crate::ffi::spawn::{self, SpawnRequest, SpawnResult};
use crate::render::cursor::{self, CursorImage, CursorKind};
use crate::render::{FULLSCREEN_BACKGROUND, GlesRenderer, ImageRef, Quad};
use crate::server::{Damage, OutputDesc, Server};
use crate::shell::{CursorShape, Edges, Shell, WindowId};
use crate::state::AppError;
use crate::watchdog::Heartbeat;

/// How long a refused commit waits before drawing is tried again (S5).
const RETRY: Duration = Duration::from_millis(100);

/// One display's frame state, as `App` keeps it.
struct OutputState {
    info: OutputInfo,
    /// A new frame is wanted.
    dirty: bool,
    /// Windows in the frame waiting for its flip.
    shown: Vec<WindowId>,
    /// When the last frame was shown (frame clock, ms).
    last_present_ms: Option<u32>,
    /// An estimated-vblank timer is armed.
    timer_armed: bool,
    /// Its place in the row as last announced to applications (`wl_output.geometry`).
    position: Option<(i32, i32)>,
}

/// Everything the event loop works on.
struct App {
    // Field order matters for drop: GL objects (with the context current, see `Drop`), then
    // the backend (displays off, EGL, card, session).
    renderer: Option<GlesRenderer>,
    backend: DrmBackend,
    server: Server,
    /// Kept for its lifetime: dropping it removes the socket and its lock file.
    listener: ListeningSocket,
    shell: Shell,
    outputs: HashMap<ViewerId, OutputState>,
    cursors: Vec<(CursorKind, CursorImage)>,
    /// Where the pointer was drawn last, and with which image.
    drawn_pointer: Option<(ViewerId, Point, Option<CursorKind>)>,
    /// The windows each display shows now, and those it showed now or one step ago (a window
    /// that just left a display still needs a frame there to disappear).
    shown: HashMap<ViewerId, Vec<WindowId>>,
    recently_shown: HashMap<ViewerId, Vec<WindowId>>,
    homes_placed: bool,
    started: Instant,
    handle: LoopHandle<'static, App>,
    signal: LoopSignal,
    retry_armed: bool,
    /// The spawn bindings' commands, in the order of `Action::Spawn(n)` (FR-014), the socket
    /// for `WAYLAND_DISPLAY`, and where spawn results come back (S18).
    spawns: Vec<Vec<String>>,
    socket_name: OsString,
    spawned: Sender<SpawnResult>,
    error: Option<AppError>,
    /// Shows the watchdog the loop is alive (S19).
    heartbeat: Heartbeat,
}

/// How often an idle loop refreshes the heartbeat.
const HEARTBEAT: Duration = Duration::from_secs(1);

/// The signals that end RubickWay; a debug build also takes SIGUSR1, which freezes the loop to
/// try the watchdog (plan Complexity Tracking).
#[cfg(not(debug_assertions))]
const HANDLED_SIGNALS: &[Signal] = &[Signal::SIGINT, Signal::SIGTERM, Signal::SIGHUP];
#[cfg(debug_assertions)]
const HANDLED_SIGNALS: &[Signal] = &[
    Signal::SIGINT,
    Signal::SIGTERM,
    Signal::SIGHUP,
    Signal::SIGUSR1,
];

/// Debug builds only: stops the event loop for good, as a hang would (quickstart Q23).
#[cfg(debug_assertions)]
fn freeze() -> ! {
    tracing::warn!("SIGUSR1: freezing the event loop to try the watchdog");
    loop {
        std::thread::park();
    }
}

/// Runs RubickWay on the hardware until the user quits or a signal arrives.
pub fn run() -> Result<(), AppError> {
    // Before any thread starts, so all of them inherit the mask and only the loop's signal
    // source receives these signals (research S20).
    let handled: Vec<libc::c_int> = HANDLED_SIGNALS.iter().map(|s| *s as libc::c_int).collect();
    crate::ffi::signals::block(&handled)
        .map_err(|e| AppError::Loop(format!("blocking signals: {e}")))?;
    let config = load_config();
    // From here on a frozen RubickWay ends itself and gives the console back (FR-030).
    let heartbeat = Heartbeat::new();
    watchdog::start(heartbeat.clone(), config.watchdog_timeout);
    let backend = DrmBackend::new()?;
    heartbeat.beat();
    backend.make_current_none()?;
    let renderer = GlesRenderer::new(backend.egl().load_gl()).map_err(|e| backend.unusable(e))?;
    heartbeat.beat();
    let mut server = Server::new()?;
    server.set_max_texture_side(renderer.max_texture_side());
    let (listener, name) = Server::listen()?;
    announce_socket(&name);
    heartbeat.beat();

    let event_loop: EventLoop<'static, App> =
        EventLoop::try_new().map_err(|e| AppError::Loop(e.to_string()))?;
    let handle = event_loop.handle();
    let (session_fd, card_fd, input_fd) =
        backend.fds().map_err(|e| AppError::Loop(e.to_string()))?;
    insert_fd(&handle, session_fd, |app| {
        let r = app.backend.dispatch_session();
        app.check(r);
    })?;
    insert_fd(&handle, card_fd, |app| {
        let now = app.now_ms();
        let r = app.backend.dispatch_card(now);
        app.check(r);
    })?;
    insert_fd(&handle, input_fd, |app| {
        let r = app.backend.dispatch_input();
        app.check(r);
    })?;
    if let Some(hotplug_fd) = backend.hotplug_fd() {
        insert_fd(&handle, hotplug_fd, |app| {
            let r = app.backend.dispatch_hotplug();
            app.check(r);
        })?;
    }
    let listen_fd = std::os::fd::AsFd::as_fd(&listener)
        .try_clone_to_owned()
        .map_err(|e| AppError::Loop(e.to_string()))?;
    insert_fd(&handle, listen_fd, App::accept_clients)?;
    let server_fd = server
        .poll_fd()
        .map_err(|e| AppError::Loop(e.to_string()))?;
    insert_fd(&handle, server_fd, |app| {
        if let Err(err) = app.server.dispatch() {
            app.fail(AppError::Loop(format!("serving applications: {err}")));
        }
    })?;
    let signals = Signals::new(HANDLED_SIGNALS).map_err(|e| AppError::Loop(e.to_string()))?;
    handle
        .insert_source(signals, |event, _, app| {
            #[cfg(debug_assertions)]
            if event.signal() == Signal::SIGUSR1 {
                freeze();
            }
            tracing::info!("received {:?}, exiting", event.signal());
            app.signal.stop();
        })
        .map_err(|e| AppError::Loop(e.to_string()))?;
    // An idle loop still shows it is alive (FR-030).
    handle
        .insert_source(Timer::from_duration(HEARTBEAT), |_, _, app| {
            app.heartbeat.beat();
            TimeoutAction::ToDuration(HEARTBEAT)
        })
        .map_err(|e| AppError::Loop(e.to_string()))?;

    let (spawned, results) = channel::<SpawnResult>();
    handle
        .insert_source(results, |event, _, _| {
            if let channel::Event::Msg(result) = event {
                match result {
                    SpawnResult::Failed { command, error } => {
                        tracing::warn!("cannot start {command}: {error}");
                    }
                    SpawnResult::Ended { command, how } => tracing::info!("{command} {how}"),
                }
            }
        })
        .map_err(|e| AppError::Loop(e.to_string()))?;

    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    let cursors = cursor::load(&cursor::icon_dirs(home.as_deref()));
    let mut app = App {
        renderer: Some(renderer),
        backend,
        server,
        listener,
        shell: Shell::with_bindings(config.bindings()),
        outputs: HashMap::new(),
        cursors,
        drawn_pointer: None,
        shown: HashMap::new(),
        recently_shown: HashMap::new(),
        homes_placed: false,
        started: Instant::now(),
        handle: handle.clone(),
        signal: event_loop.get_signal(),
        retry_armed: false,
        spawns: config.spawns.iter().map(|s| s.command.clone()).collect(),
        socket_name: name,
        spawned,
        error: None,
        heartbeat,
    };
    app.upload_cursors();
    // News libseat already read while starting (a console switch during startup) would
    // otherwise wait for the next message on the seat's fd.
    let r = app.backend.dispatch_session();
    app.check(r);
    let mut event_loop = event_loop;
    app.before_sleep();
    let ran = event_loop.run(None, &mut app, App::before_sleep);
    app.heartbeat.beat();
    ran.map_err(|e| AppError::Loop(e.to_string()))?;
    match app.error.take() {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

/// Reads the configuration and logs its warnings (before the loop runs, Principle VII).
fn load_config() -> Config {
    let Some(path) = config_path(
        std::env::var_os("XDG_CONFIG_HOME").as_deref(),
        std::env::var_os("HOME").as_deref(),
    ) else {
        return Config::default();
    };
    let (config, warnings) = Config::load(&path);
    for w in &warnings {
        tracing::warn!("{}", w.render(&path));
    }
    config
}

fn announce_socket(name: &OsString) {
    tracing::info!(
        "applications can connect with WAYLAND_DISPLAY={}",
        name.to_string_lossy()
    );
}

fn insert_fd(
    handle: &LoopHandle<'static, App>,
    fd: OwnedFd,
    mut on_read: impl FnMut(&mut App) + 'static,
) -> Result<(), AppError> {
    handle
        .insert_source(
            Generic::new(fd, Interest::READ, Mode::Level),
            move |_, _, app| {
                on_read(app);
                Ok(PostAction::Continue)
            },
        )
        .map(|_| ())
        .map_err(|e| AppError::Loop(e.to_string()))
}

impl App {
    fn fail(&mut self, err: AppError) {
        if self.error.is_none() {
            self.error = Some(err);
        }
        self.signal.stop();
    }

    fn check(&mut self, result: Result<(), DrmError>) {
        if let Err(err) = result {
            self.fail(err.into());
        }
    }

    /// Milliseconds since startup, wrapping: the clock of frame callbacks and input events.
    fn now_ms(&self) -> u32 {
        self.started.elapsed().as_millis() as u32
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

    fn upload_cursors(&mut self) {
        let Some(renderer) = &mut self.renderer else {
            return;
        };
        for (kind, image) in &self.cursors {
            if let Err(err) = renderer.upload(kind.texture_key(), image.image_ref()) {
                tracing::warn!("cannot upload the pointer image {kind:?}: {err}");
            }
        }
    }

    /// Runs after every loop dispatch, before sleeping.
    fn before_sleep(&mut self) {
        // Called after every dispatch of the loop.
        self.heartbeat.beat();
        if self.error.is_some() {
            return;
        }
        if let Err(err) = self.step() {
            self.fail(err);
        }
    }

    fn step(&mut self) -> Result<(), AppError> {
        self.handle_backend_events();
        for event in self.server.take_window_events() {
            let inputs = self.shell.apply(event);
            self.server.deliver(&inputs, self.now_ms());
        }
        self.server.command(&self.shell.take_commands());
        self.upload_content()?;
        self.update_window_outputs();
        self.mark_dirty();
        self.draw_outputs()?;
        if let Err(err) = self.server.flush() {
            tracing::warn!("sending to applications failed: {err}");
        }
        Ok(())
    }

    fn handle_backend_events(&mut self) {
        let mut added = false;
        for event in self.backend.take_events() {
            match event {
                BackendEvent::Input(input) => {
                    let (effect, inputs) = self.shell.handle(&input);
                    self.server.deliver(&inputs, self.now_ms());
                    if effect.quit {
                        tracing::info!("quit requested");
                        self.signal.stop();
                    }
                    if let Some(n) = effect.spawn {
                        self.spawn(n);
                    }
                }
                BackendEvent::OutputAdded { id, info } => {
                    tracing::info!(
                        "display {} ({} {}) {}x{}@{} Hz",
                        info.name,
                        info.make,
                        info.model,
                        info.mode.width,
                        info.mode.height,
                        info.mode.refresh_mhz.div_ceil(1000)
                    );
                    if let Some(size) = self.backend.output_size(id) {
                        self.shell.add_viewer(id, size, info.internal);
                    }
                    self.outputs.insert(
                        id,
                        OutputState {
                            info,
                            dirty: true,
                            shown: Vec::new(),
                            last_present_ms: None,
                            timer_armed: false,
                            position: None,
                        },
                    );
                    self.reflow_outputs();
                    added = true;
                }
                BackendEvent::OutputRemoved { id } => {
                    if let Some(state) = self.outputs.remove(&id) {
                        tracing::info!("display {} disconnected", state.info.name);
                    }
                    self.server.remove_output(id.0);
                    let inputs = self.shell.remove_viewer(id);
                    self.server.deliver(&inputs, self.now_ms());
                    self.reflow_outputs();
                    // The other displays may show what it showed.
                    for state in self.outputs.values_mut() {
                        state.dirty = true;
                    }
                }
                BackendEvent::Presented { id, time_ms } => self.presented(id, time_ms),
                // Frame callbacks wait while nothing is shown (S12).
                BackendEvent::Paused => self.server.set_paused(true),
                BackendEvent::Resumed => {
                    self.server.set_paused(false);
                    for state in self.outputs.values_mut() {
                        state.dirty = true;
                    }
                }
                BackendEvent::Keymap(text) => self.server.set_keymap(&text),
                BackendEvent::RepeatInfo { rate, delay } => self.server.set_repeat(rate, delay),
            }
        }
        // The displays present at startup share one centred row of home views (FR-020).
        if added && !self.homes_placed {
            self.shell.place_home_views();
            self.homes_placed = true;
        }
    }

    /// Announces every display whose place in the row is new (it was added, or the row
    /// re-flowed): `wl_output` description with the position, then `done`.
    fn reflow_outputs(&mut self) {
        let ids: Vec<ViewerId> = self.outputs.keys().copied().collect();
        for id in ids {
            let position = self
                .shell
                .layout()
                .origin(id)
                .map_or((0, 0), |p| (p.x.round() as i32, p.y.round() as i32));
            let Some(state) = self.outputs.get_mut(&id) else {
                continue;
            };
            if state.position == Some(position) {
                continue;
            }
            state.position = Some(position);
            let info = &state.info;
            self.server.add_output(
                id.0,
                OutputDesc {
                    name: info.name.clone(),
                    make: info.make.clone(),
                    model: info.model.clone(),
                    physical_mm: info.physical_mm,
                    mode: (info.mode.width, info.mode.height, info.mode.refresh_mhz),
                    position,
                },
            );
        }
    }

    /// Tells applications which displays show each window (`wl_surface.enter`/`leave`); the
    /// server sends only the changes. Also remembers what each display shows, for `mark_dirty`.
    fn update_window_outputs(&mut self) {
        let mut on: HashMap<WindowId, Vec<u32>> = self
            .shell
            .windows()
            .iter()
            .map(|w| (w.id, Vec::new()))
            .collect();
        let viewers: Vec<ViewerId> = self.shell.viewers().map(|(id, _)| id).collect();
        let mut shown = HashMap::new();
        for viewer in viewers {
            let visible = self.shell.visible_in(viewer);
            for window in &visible {
                on.entry(*window).or_default().push(viewer.0);
            }
            shown.insert(viewer, visible);
        }
        for (window, ids) in on {
            self.server.set_window_outputs(window, &ids);
        }
        self.recently_shown = shown
            .iter()
            .map(|(viewer, now)| {
                let mut both = now.clone();
                both.extend(self.shown.get(viewer).into_iter().flatten());
                (*viewer, both)
            })
            .collect();
        self.shown = shown;
    }

    /// Starts spawn binding `n`'s command off the loop, in the home directory, with
    /// RubickWay's environment plus `WAYLAND_DISPLAY` (FR-028).
    fn spawn(&self, n: usize) {
        let Some(command) = self.spawns.get(n) else {
            return;
        };
        tracing::info!("starting {}", command.join(" "));
        let sender = self.spawned.clone();
        spawn::spawn(
            SpawnRequest {
                command: command.clone(),
                env: vec![("WAYLAND_DISPLAY".into(), self.socket_name.clone())],
                dir: std::env::var_os("HOME").map(std::path::PathBuf::from),
            },
            move |result| {
                let _ = sender.send(result);
            },
        );
    }

    fn presented(&mut self, id: ViewerId, time_ms: u32) {
        let Some(state) = self.outputs.get_mut(&id) else {
            return;
        };
        if let Some(last) = state.last_present_ms {
            tracing::debug!(
                "frame {} {} ms",
                state.info.name,
                time_ms.wrapping_sub(last)
            );
        }
        state.last_present_ms = Some(time_ms);
        let shown = std::mem::take(&mut state.shown);
        self.server.presented(time_ms, &shown);
    }

    /// Collects what changed since the last frame into each display's dirty flag.
    fn mark_dirty(&mut self) {
        let damage = self.server.take_damage();
        for id in self.shell.take_dirty_viewers() {
            if let Some(state) = self.outputs.get_mut(&id) {
                state.dirty = true;
            }
        }
        // New content redraws only the displays showing that window (S7).
        for (id, state) in &mut self.outputs {
            let shown = self.recently_shown.get(id).map_or(&[][..], Vec::as_slice);
            if needs_frame(&damage, shown) {
                state.dirty = true;
            }
        }
        // The pointer is drawn into the frame (S10): moving it redraws the displays it left
        // and entered.
        let now = self
            .shell
            .pointer()
            .map(|(id, p)| (id, p, self.pointer_kind()));
        if now != self.drawn_pointer {
            for (id, _, _) in [self.drawn_pointer, now].into_iter().flatten() {
                if let Some(state) = self.outputs.get_mut(&id) {
                    state.dirty = true;
                }
            }
        }
    }

    /// The image for the shell's cursor shape; `None` hides the pointer.
    fn pointer_kind(&self) -> Option<CursorKind> {
        Some(match self.shell.cursor() {
            CursorShape::Default => CursorKind::Arrow,
            CursorShape::Grabbing => CursorKind::Grab,
            CursorShape::Move => CursorKind::Move,
            CursorShape::Resize(edges) => resize_kind(edges),
            CursorShape::Hidden => return None,
        })
    }

    /// Copies newly committed buffers into textures and frees textures of destroyed surfaces.
    fn upload_content(&mut self) -> Result<(), AppError> {
        let Some(renderer) = &mut self.renderer else {
            return Ok(());
        };
        self.backend.make_current_none()?;
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

    fn draw_outputs(&mut self) -> Result<(), AppError> {
        let ids: Vec<ViewerId> = self.outputs.keys().copied().collect();
        let mut retry = false;
        for id in ids {
            let dirty = self.outputs.get(&id).is_some_and(|s| s.dirty);
            if !self.backend.can_draw(id) {
                continue;
            }
            if dirty {
                if self.draw(id)? {
                    if let Some(state) = self.outputs.get_mut(&id) {
                        state.dirty = false;
                        state.shown = self.shell.visible_in(id);
                    }
                } else {
                    retry = true;
                }
            } else {
                self.idle_callbacks(id);
            }
        }
        self.drawn_pointer = self
            .shell
            .pointer()
            .map(|(id, p)| (id, p, self.pointer_kind()));
        if retry {
            self.arm_retry();
        }
        Ok(())
    }

    /// Draws display `id` and commits it; `false` when the commit was refused.
    fn draw(&mut self, id: ViewerId) -> Result<bool, AppError> {
        let Some(viewer) = self.shell.viewer_of(id) else {
            return Ok(false);
        };
        let (w, h) = viewer.buffer_size();
        let grid = viewer.grid_params();
        let quads = self.window_quads(id);
        let pointer = self.pointer_quad(id);
        self.backend.begin_frame(id)?;
        let Some(renderer) = &self.renderer else {
            return Ok(false);
        };
        if self.shell.fullscreen_in(id).is_some() {
            renderer.clear(FULLSCREEN_BACKGROUND, w, h);
        } else {
            renderer.draw(&grid, w, h);
        }
        renderer.draw_windows(&quads, w, h);
        if let Some(q) = pointer {
            renderer.draw_windows(&[q], w, h);
        }
        Ok(self.backend.finish_frame(id)?)
    }

    /// The window surfaces to draw on display `id`, back to front, in physical pixels.
    fn window_quads(&self, id: ViewerId) -> Vec<Quad> {
        let Some(viewer) = self.shell.viewer_of(id) else {
            return Vec::new();
        };
        let camera = viewer.camera();
        let size = viewer.size();
        let scale = viewer.scale();
        let mut quads = Vec::new();
        for wid in self.shell.visible_in(id) {
            let Some(window) = self.shell.windows().get(wid) else {
                continue;
            };
            for part in self.server.window_parts(wid) {
                if !part.has_content {
                    continue;
                }
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
        quads
    }

    /// The pointer's quad if it is on display `id` and not hidden.
    fn pointer_quad(&self, id: ViewerId) -> Option<Quad> {
        let (on, pos) = self.shell.pointer()?;
        if on != id {
            return None;
        }
        let kind = self.pointer_kind()?;
        let (_, image) = self.cursors.iter().find(|(k, _)| *k == kind)?;
        Some(image.quad(kind, (pos.x as f32, pos.y as f32)))
    }

    /// Nothing to draw, but applications on display `id` wait for a frame: complete their
    /// callbacks one refresh after the last frame (estimated vblank, as niri; S7).
    fn idle_callbacks(&mut self, id: ViewerId) {
        let visible = self.shell.visible_in(id);
        if !self.server.has_pending_callbacks(&visible) {
            return;
        }
        let refresh = self.backend.refresh_interval_ms(id);
        let now = self.now_ms();
        let Some(state) = self.outputs.get_mut(&id) else {
            return;
        };
        if state.timer_armed {
            return;
        }
        let elapsed = state
            .last_present_ms
            .map_or(refresh, |t| now.wrapping_sub(t));
        let wait = refresh.saturating_sub(elapsed).max(1);
        state.timer_armed = true;
        let timer = Timer::from_duration(Duration::from_millis(u64::from(wait)));
        let inserted = self.handle.insert_source(timer, move |_, _, app| {
            let now = app.now_ms();
            if let Some(state) = app.outputs.get_mut(&id) {
                state.timer_armed = false;
                state.last_present_ms = Some(now);
            }
            let visible = app.shell.visible_in(id);
            app.server.presented(now, &visible);
            TimeoutAction::Drop
        });
        if inserted.is_err() {
            state.timer_armed = false;
        }
    }

    /// Wakes the loop in a moment to retry refused commits (S5).
    fn arm_retry(&mut self) {
        if self.retry_armed {
            return;
        }
        self.retry_armed = true;
        let timer = Timer::from_duration(RETRY);
        if self
            .handle
            .insert_source(timer, |_, _, app| {
                app.retry_armed = false;
                TimeoutAction::Drop
            })
            .is_err()
        {
            self.retry_armed = false;
        }
    }
}

/// Whether a display that shows (or just showed) `shown` needs a frame for `damage`.
fn needs_frame(damage: &Damage, shown: &[WindowId]) -> bool {
    damage.all || damage.windows.iter().any(|w| shown.contains(w))
}

/// The image for a resize from `edges`.
fn resize_kind(edges: Edges) -> CursorKind {
    match (edges.top, edges.bottom, edges.left, edges.right) {
        (true, _, true, _) | (_, true, _, true) => CursorKind::ResizeNwse,
        (true, _, _, true) | (_, true, true, _) => CursorKind::ResizeNesw,
        (true, _, _, _) | (_, true, _, _) => CursorKind::ResizeNs,
        _ => CursorKind::ResizeEw,
    }
}

impl Drop for App {
    fn drop(&mut self) {
        // GL objects need the context current; the backend then turns the displays off.
        if self.backend.make_current_none().is_ok() {
            drop(self.renderer.take());
        } else {
            std::mem::forget(self.renderer.take());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_content_redraws_only_the_displays_showing_that_window() {
        let damage = Damage {
            all: false,
            windows: vec![WindowId(2)],
        };
        assert!(needs_frame(&damage, &[WindowId(1), WindowId(2)]));
        assert!(!needs_frame(&damage, &[WindowId(1)]));
        assert!(!needs_frame(&damage, &[]));
        let all = Damage {
            all: true,
            windows: Vec::new(),
        };
        assert!(needs_frame(&all, &[]), "a window came or went");
    }
}
