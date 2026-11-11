//! The standalone application: the DRM backend, the Wayland server, the shell and the
//! renderer on one event loop (research S2, S7).
//!
//! Frame scheduling per display (data-model "FrameState"): a display is drawn when its viewer
//! or the windows changed and no page flip is pending for it; the flip reports the windows it
//! showed to the server, which completes their frame callbacks. When applications wait for a
//! frame and nothing changed, an estimated-vblank timer completes them without drawing.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::os::fd::OwnedFd;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use calloop::channel::{self, Sender, channel};
use calloop::generic::Generic;
use calloop::signals::{Signal, Signals};
use calloop::timer::{TimeoutAction, Timer};
use calloop::{EventLoop, Interest, LoopHandle, LoopSignal, Mode, PostAction, RegistrationToken};
use wayland_server::ListeningSocket;

use crate::backend::drm::{DrmBackend, DrmError};
use crate::backend::watchdog;
use crate::backend::{BackendEvent, OutputInfo};
use crate::canvas::{Point, ViewerId};
use crate::config::{Config, config_path};
use crate::distant::{DistantWindows, FramePace};
use crate::ffi::clock::thread_cpu_time;
use crate::ffi::spawn::{self, SpawnRequest, SpawnResult};
use crate::render::cursor::{self, CursorKind, PointerImages};
use crate::render::formats;
use crate::render::{FULLSCREEN_BACKGROUND, GlesRenderer, ImageDesc, ImageRef, PlaneDesc, Quad};
use crate::server::{Damage, OutputDesc, Server, WaitId};
use crate::shell::{CursorName, CursorShape, Edges, Shell, WindowEvent, WindowId};
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
    /// The number of the frame waiting for its flip (research G9).
    in_flight: Option<u64>,
    /// The canvas zoom last written to the debug log.
    logged_zoom: Option<f64>,
    /// When applications last got their frame callbacks (frame clock, ms): at a flip or at an
    /// estimated vblank.
    last_present_ms: Option<u32>,
    /// When the last frame was really shown, for the frame-time log line: estimated vblanks do
    /// not count, or the line would hide a long gap between frames.
    last_flip_ms: Option<u32>,
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
    cursors: PointerImages,
    /// Shapes already reported as shown with the arrow (cursor-shapes FR-007).
    arrow_noted: HashSet<CursorName>,
    /// Where the pointer was drawn last, and with which image.
    drawn_pointer: Option<(ViewerId, Point, Option<CursorKind>)>,
    /// The windows each display shows now, and those it showed now or one step ago (a window
    /// that just left a display still needs a frame there to disappear).
    shown: HashMap<ViewerId, Vec<WindowId>>,
    recently_shown: HashMap<ViewerId, Vec<WindowId>>,
    /// Windows whose first frame reached a display (SC-001's log line).
    first_frames: HashSet<WindowId>,
    /// The event sources watching what commits wait for (research G6).
    wait_sources: HashMap<WaitId, RegistrationToken>,
    /// Less work for distant windows (FR-017).
    distant: DistantWindows,
    pace: FramePace,
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

/// Frame work on the loop this long or longer gets a debug line saying where the time went.
const SLOW_FRAME_WORK: Duration = Duration::from_millis(5);

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
    // GPU images hold descriptors for as long as they live (research G10); programs started
    // later get the original limit back.
    if let Err(err) = crate::ffi::spawn::raise_file_limit() {
        tracing::warn!("cannot raise the limit of open files: {err}");
    }
    let config = load_config();
    if let Some(line) = config.distant_windows.describe() {
        tracing::info!("{line}");
    }
    // From here on a frozen RubickWay ends itself and gives the console back (FR-030).
    let heartbeat = Heartbeat::new();
    watchdog::start(heartbeat.clone(), config.watchdog_timeout);
    let backend = DrmBackend::new()?;
    heartbeat.beat();
    backend.make_current_none()?;
    let renderer = GlesRenderer::new(backend.egl().load_gl()).map_err(|e| backend.unusable(e))?;
    heartbeat.beat();
    let mut server = Server::new()?;
    offer_gpu_images(&backend, &renderer, &mut server);
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
    // The cursor theme is read here, before the loop runs (cursor-shapes research C12).
    let (cursors, report) = cursor::load(
        &cursor::icon_dirs(home.as_deref()),
        config.cursor_theme.as_deref(),
    );
    for line in report.lines() {
        if line.warn {
            tracing::warn!("{}", line.text);
        } else {
            tracing::info!("{}", line.text);
        }
    }
    let mut app = App {
        renderer: Some(renderer),
        backend,
        server,
        listener,
        shell: Shell::with_bindings(config.bindings()),
        outputs: HashMap::new(),
        cursors,
        arrow_noted: HashSet::new(),
        drawn_pointer: None,
        shown: HashMap::new(),
        recently_shown: HashMap::new(),
        first_frames: HashSet::new(),
        wait_sources: HashMap::new(),
        distant: config.distant_windows,
        pace: FramePace::default(),
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

/// Tells applications the GPU and what it reads, or logs why they will draw in software (FR-014,
/// FR-015).
fn offer_gpu_images(backend: &DrmBackend, renderer: &GlesRenderer, server: &mut Server) {
    let gpu = renderer.name();
    let offered = (|| -> Result<(PathBuf, usize), String> {
        let egl = backend.egl();
        let can_import = egl.can_import() && renderer.can_sample_images();
        let pairs = if can_import {
            formats::table(&egl.dma_buf_formats().map_err(|e| e.to_string())?)
        } else {
            Vec::new()
        };
        let count = pairs
            .iter()
            .map(|(format, _)| *format)
            .collect::<HashSet<_>>()
            .len();
        let pairs = formats::offer(can_import, pairs)
            .ok_or_else(|| "it does not offer image import".to_string())?;
        let (node, device) = backend.render_node().map_err(|e| e.to_string())?;
        server
            .set_gpu_offer(device, pairs)
            .map_err(|e| e.to_string())?;
        Ok((node, count))
    })();
    match offered {
        Ok((node, count)) => {
            tracing::info!(
                "composing on {gpu} ({}), offering applications {count} image formats",
                node.display()
            );
            match backend.timelines() {
                Ok(Some(timelines)) => server.set_timelines(timelines),
                Ok(None) => {
                    tracing::warn!("explicit synchronisation is off: {gpu} does not support it");
                }
                Err(err) => tracing::warn!(
                    "explicit synchronisation is off: {gpu} does not support it ({err})"
                ),
            }
        }
        Err(reason) => {
            tracing::warn!(
                "{gpu} cannot import GPU images ({reason}): applications will draw in software"
            );
        }
    }
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
                    if !self.server.note_descriptor_failure(&err) {
                        tracing::warn!("accepting an application failed: {err}");
                    }
                    break;
                }
            }
        }
    }

    fn upload_cursors(&mut self) {
        let Some(renderer) = &mut self.renderer else {
            return;
        };
        for (kind, image) in self.cursors.iter() {
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
        self.server.check_descriptors();
        self.handle_backend_events();
        // Before window events: commits that waited for an import apply here (research G7).
        self.import_images()?;
        self.watch_waits();
        for event in self.server.take_window_events() {
            if let WindowEvent::Unmapped { id } = &event {
                self.first_frames.remove(id);
                self.pace.forget(id.0);
            }
            let inputs = self.shell.apply(event);
            self.server.deliver(&inputs, self.now_ms());
        }
        self.server.command(&self.shell.take_commands());
        self.upload_content()?;
        self.update_window_outputs();
        self.mark_dirty();
        self.draw_outputs()?;
        let oldest = self.outputs.values().filter_map(|s| s.in_flight).min();
        self.server.release_images(oldest);
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
                            in_flight: None,
                            logged_zoom: None,
                            last_present_ms: None,
                            last_flip_ms: None,
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
                BackendEvent::Paused => {
                    self.server.set_paused(true);
                    // Their flips will never be reported.
                    for state in self.outputs.values_mut() {
                        state.in_flight = None;
                    }
                }
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
        if let Some(last) = state.last_flip_ms {
            tracing::debug!(
                "frame {} {} ms",
                state.info.name,
                time_ms.wrapping_sub(last)
            );
        }
        state.last_flip_ms = Some(time_ms);
        state.last_present_ms = Some(time_ms);
        state.in_flight = None;
        let shown = std::mem::take(&mut state.shown);
        for window in &shown {
            // A window closed while its last frame waited for the flip is not a first frame.
            if self.shell.windows().get(*window).is_some() && self.first_frames.insert(*window) {
                tracing::debug!("window {}: first frame presented", window.0);
            }
        }
        let due = self.due_windows(id, &shown, time_ms);
        self.server.presented(time_ms, &due);
    }

    /// Those of `windows` that get their frame callbacks now: distant ones are held to their pace,
    /// and a window near on another display gets them from that display (FR-017).
    fn due_windows(&mut self, id: ViewerId, windows: &[WindowId], now_ms: u32) -> Vec<WindowId> {
        let zoom = self.shell.viewer_of(id).map(|v| v.camera().zoom);
        let Some(interval) = zoom.and_then(|z| self.distant.frame_interval_ms(z)) else {
            return windows.to_vec();
        };
        windows
            .iter()
            .copied()
            .filter(|w| self.pace.due(w.0, now_ms, interval))
            .collect()
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
        if let Some((_, _, Some(kind))) = now {
            let stand_in = self.cursors.arrow_stand_in(kind);
            if let Some(line) = arrow_stand_in_note(&mut self.arrow_noted, kind, stand_in) {
                tracing::info!("{line}");
            }
        }
    }

    /// The image for the shell's cursor shape; `None` hides the pointer.
    fn pointer_kind(&self) -> Option<CursorKind> {
        pointer_kind(self.shell.cursor())
    }

    /// Copies newly committed buffers into textures and frees textures of destroyed surfaces.
    fn upload_content(&mut self) -> Result<(), AppError> {
        let Some(renderer) = &mut self.renderer else {
            return Ok(());
        };
        self.backend.make_current_none()?;
        // Images first: a shared-memory commit after one replaces it, and one before it was
        // dropped when the image was attached.
        for shown in self.server.take_shown_images() {
            renderer.show_image(shown.surface.0, shown.image.0, shown.y_invert);
        }
        for image in self.server.take_destroyed_images() {
            renderer.remove_image(image.0);
        }
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

    /// Watches what commits wait for from the event loop, which never waits itself (Principle VII,
    /// research G6). A wait that cannot be watched counts as finished: the image is shown as it is
    /// rather than the window stopping for good.
    fn watch_waits(&mut self) {
        for wait in self.server.take_waits() {
            let id = wait.id;
            let inserted = self.handle.insert_source(
                Generic::new(wait.fd, Interest::READ, Mode::Level),
                move |_, _, app| {
                    app.wait_sources.remove(&id);
                    app.server.wait_done(id);
                    Ok(PostAction::Remove)
                },
            );
            match inserted {
                Ok(token) => {
                    self.wait_sources.insert(id, token);
                }
                Err(err) => {
                    tracing::warn!(
                        "cannot wait for an image to be finished ({err}): showing it as it is"
                    );
                    self.server.wait_done(id);
                }
            }
        }
        for id in self.server.take_cancelled_waits() {
            if let Some(token) = self.wait_sources.remove(&id) {
                self.handle.remove(token);
            }
        }
    }

    /// Imports the GPU images applications handed over (research G7, G8).
    fn import_images(&mut self) -> Result<(), AppError> {
        let Some(renderer) = &mut self.renderer else {
            return Ok(());
        };
        self.backend.make_current_none()?;
        let egl = self.backend.egl();
        self.server.process_imports(|image| {
            let planes: Vec<PlaneDesc<'_>> = image
                .planes
                .iter()
                .map(|p| PlaneDesc {
                    fd: p.fd,
                    offset: p.offset,
                    stride: p.stride,
                })
                .collect();
            let imported = egl
                .import_dma_buf(&ImageDesc {
                    width: image.width,
                    height: image.height,
                    fourcc: image.fourcc,
                    modifier: image.modifier,
                    planes: &planes,
                })
                .map_err(|e| e.to_string())
                .and_then(|imported| {
                    renderer
                        .add_image(
                            image.key.0,
                            imported,
                            image.width.unsigned_abs(),
                            image.height.unsigned_abs(),
                        )
                        .map_err(|e| e.to_string())
                });
            imported.map_err(|detail| {
                tracing::debug!("importing an image: {detail}");
                formats::refusal(&renderer.name(), image.fourcc, image.modifier)
            })
        });
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
        let zoom = viewer.camera().zoom;
        let detail = self.distant.copy_detail(zoom);
        if let Some(state) = self.outputs.get_mut(&id)
            && state.logged_zoom != Some(zoom)
        {
            state.logged_zoom = Some(zoom);
            tracing::debug!(
                "display {}: canvas zoom {:.0}%, windows {}",
                state.info.name,
                zoom * 100.0,
                if self.distant.is_distant(zoom) {
                    "distant"
                } else {
                    "near"
                }
            );
        }
        let quads = self.window_quads(id);
        let pointer = self.pointer_quad(id);
        let started = Instant::now();
        let cpu_at_start = thread_cpu_time();
        self.backend.begin_frame(id)?;
        let Some(renderer) = &mut self.renderer else {
            return Ok(false);
        };
        if let Err(err) = renderer.prepare_frame(&quads, detail) {
            tracing::warn!("cannot prepare a zoomed-out window: {err}");
        }
        let prepared = started.elapsed();
        if self.shell.fullscreen_in(id).is_some() {
            renderer.clear(FULLSCREEN_BACKGROUND, w, h);
        } else {
            renderer.draw(&grid, w, h);
        }
        renderer.draw_windows(&quads, w, h);
        if let Some(q) = pointer {
            renderer.draw_windows(&[q], w, h);
        }
        let drawn = started.elapsed();
        let committed = self.backend.finish_frame(id)?;
        let finished = started.elapsed();
        if finished >= SLOW_FRAME_WORK {
            tracing::debug!(
                "frame work {} ms, {} ms of it on the CPU: copies {} ms, drawing {} ms, showing {} ms",
                finished.as_millis(),
                thread_cpu_time().saturating_sub(cpu_at_start).as_millis(),
                prepared.as_millis(),
                drawn.saturating_sub(prepared).as_millis(),
                finished.saturating_sub(drawn).as_millis(),
            );
        }
        if committed {
            let seq = self.server.frame_drawn();
            if let Some(state) = self.outputs.get_mut(&id) {
                state.in_flight = Some(seq);
            }
        }
        Ok(committed)
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
        let image = self.cursors.get(kind)?;
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
            let due = app.due_windows(id, &visible, now);
            app.server.presented(now, &due);
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

/// The image for the shell's cursor shape (cursor-shapes data-model "App"); `None` hides the
/// pointer.
fn pointer_kind(shape: CursorShape) -> Option<CursorKind> {
    Some(match shape {
        CursorShape::Default => CursorKind::Shape(CursorName::Default),
        CursorShape::Grabbing => CursorKind::Shape(CursorName::Grabbing),
        CursorShape::Move => CursorKind::WindowMove,
        CursorShape::Resize(edges) => CursorKind::Shape(resize_name(edges)),
        CursorShape::Named(name) => CursorKind::Shape(name),
        CursorShape::Hidden => return None,
    })
}

/// The line for the first time a shape is shown with the arrow because there is no image for
/// it (`stand_in`), once per shape per run (cursor-shapes FR-007, contracts/cli.md).
fn arrow_stand_in_note(
    noted: &mut HashSet<CursorName>,
    kind: CursorKind,
    stand_in: bool,
) -> Option<String> {
    let CursorKind::Shape(name) = kind else {
        return None;
    };
    if !stand_in || !noted.insert(name) {
        return None;
    }
    Some(format!(
        "an application asked for the {} pointer, which RubickWay has no image for: showing \
         the arrow",
        name.css_name()
    ))
}

/// The double arrow for a resize from `edges`.
fn resize_name(edges: Edges) -> CursorName {
    match (edges.top, edges.bottom, edges.left, edges.right) {
        (true, _, true, _) | (_, true, _, true) => CursorName::NwseResize,
        (true, _, _, true) | (_, true, true, _) => CursorName::NeswResize,
        (true, _, _, _) | (_, true, _, _) => CursorName::NsResize,
        _ => CursorName::EwResize,
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

    fn edges(top: bool, bottom: bool, left: bool, right: bool) -> Edges {
        Edges {
            top,
            bottom,
            left,
            right,
        }
    }

    #[test]
    fn the_shells_cursor_maps_to_its_image() {
        use CursorName as N;
        assert_eq!(
            pointer_kind(CursorShape::Default),
            Some(CursorKind::Shape(N::Default))
        );
        assert_eq!(
            pointer_kind(CursorShape::Grabbing),
            Some(CursorKind::Shape(N::Grabbing))
        );
        assert_eq!(
            pointer_kind(CursorShape::Move),
            Some(CursorKind::WindowMove)
        );
        assert_eq!(
            pointer_kind(CursorShape::Named(N::ZoomIn)),
            Some(CursorKind::Shape(N::ZoomIn))
        );
        assert_eq!(pointer_kind(CursorShape::Hidden), None);
    }

    #[test]
    fn an_arrow_stand_in_is_reported_once_per_shape() {
        use CursorName as N;
        let mut noted = HashSet::new();
        let copy = CursorKind::Shape(N::Copy);
        assert_eq!(
            arrow_stand_in_note(&mut noted, copy, true).as_deref(),
            Some(
                "an application asked for the copy pointer, which RubickWay has no image for: \
                 showing the arrow"
            )
        );
        assert_eq!(arrow_stand_in_note(&mut noted, copy, true), None, "once");
        let ask = CursorKind::Shape(N::DndAsk);
        assert!(arrow_stand_in_note(&mut noted, ask, true).is_some());
        // A theme image, or a drawing of its own: nothing to say.
        let alias = CursorKind::Shape(N::Alias);
        assert_eq!(arrow_stand_in_note(&mut noted, alias, false), None);
        assert!(arrow_stand_in_note(&mut noted, alias, true).is_some());
    }

    #[test]
    fn resizes_show_the_double_arrow_of_their_axis() {
        use CursorName as N;
        let cases = [
            (edges(true, false, true, false), N::NwseResize),
            (edges(false, true, false, true), N::NwseResize),
            (edges(true, false, false, true), N::NeswResize),
            (edges(false, true, true, false), N::NeswResize),
            (edges(true, false, false, false), N::NsResize),
            (edges(false, true, false, false), N::NsResize),
            (edges(false, false, true, false), N::EwResize),
            (edges(false, false, false, true), N::EwResize),
        ];
        for (e, name) in cases {
            assert_eq!(
                pointer_kind(CursorShape::Resize(e)),
                Some(CursorKind::Shape(name)),
                "{e:?}"
            );
        }
    }
}
