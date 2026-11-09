//! The standalone backend: RubickWay directly on the hardware (research S2-S13).
//!
//! This layer stays thin (constitution III): it gets the devices through the login session,
//! drives the displays with atomic modesetting, reads input with libinput, and turns everything
//! into `BackendEvent`s. Decisions live in the core; this layer is verified manually through the
//! feature's `quickstart.md`.
#![allow(unsafe_code)]

mod device;
mod edid;
mod gpu;
mod hotplug;
mod input;
mod keyboard;
mod session;
mod surface;
mod timelines;

use thiserror::Error;

/// Why the standalone backend stopped or could not start. Messages follow
/// `contracts/cli.md`.
#[derive(Debug, Error)]
pub enum DrmError {
    #[error("cannot open the seat: {0}; start RubickWay from a local text console")]
    NoSeat(String),
    #[error("this console is not active; start RubickWay from the console being shown")]
    NotActive,
    #[error(
        "another program controls the display; start RubickWay from a text console, not inside a desktop"
    )]
    Busy,
    #[error("no graphics device with displays found")]
    NoGpu,
    #[error("cannot use the graphics device {path}: {reason}")]
    Gpu { path: String, reason: String },
    #[error("cannot read input devices: {0}")]
    Input(String),
    #[error("lost the graphics device: {0}")]
    Lost(String),
    #[error("lost the login session: {0}")]
    SessionLost(String),
}

/// How many reads in a row of a session or input fd may fail before it counts as broken: the
/// sources are level-triggered, so a broken fd would otherwise spin the loop (FR-012).
const READ_FAILURES: u32 = 10;

/// Consecutive failed reads of one fd.
#[derive(Debug, Default)]
pub(crate) struct ReadFailures(u32);

impl ReadFailures {
    /// Records one read; true once `READ_FAILURES` reads in a row failed.
    pub fn note(&mut self, failed: bool) -> bool {
        if failed {
            self.0 = self.0.saturating_add(1);
            self.0 >= READ_FAILURES
        } else {
            self.0 = 0;
            false
        }
    }
}

use std::cell::RefCell;
use std::io;
use std::os::fd::{AsFd, OwnedFd};
use std::path::PathBuf;
use std::rc::Rc;

use crate::backend::BackendEvent;
use crate::canvas::{Size, ViewerId};
use crate::input::InputEvent;
use crate::render::egl::Egl;
use device::{Card, Found};
use input::{InputContext, Translated, translate};
use keyboard::{DrmKeyboard, KeyOutcome};
use session::{Session, SessionEvent, SharedSession};
use surface::OutputSurface;

/// Key repeat offered to applications (FR-007).
const REPEAT_RATE: i32 = 25;
const REPEAT_DELAY: i32 = 600;

/// One display being driven.
struct Output {
    id: ViewerId,
    found: Found,
    surface: OutputSurface,
    /// A commit is waiting for its page flip.
    waiting_flip: bool,
    /// The next commit sets the mode (first commit, after a pause, after a failure).
    needs_modeset: bool,
    /// The last commit failed and the next one is its single modeset retry (S5).
    retrying: bool,
}

/// What follows a failed commit (S5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommitNext {
    /// Refused because a flip is pending: draw again later.
    Later,
    /// The device is gone (FR-012).
    Lost,
    /// Retry once as a full modeset on the next frame.
    RetryModeset,
    /// The retry failed too: the display stays dark.
    GiveUp,
}

fn after_failed_commit(errno: Option<i32>, retrying: bool) -> CommitNext {
    match errno {
        Some(libc::EBUSY) => CommitNext::Later,
        Some(libc::ENODEV) => CommitNext::Lost,
        _ if retrying => CommitNext::GiveUp,
        _ => CommitNext::RetryModeset,
    }
}

/// A display that went away while a flip was pending: it is turned off after that flip (S5).
struct Retiring {
    found: Found,
    surface: OutputSurface,
}

/// What follows a refused non-blocking off-commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OffNext {
    /// Try again once the pending flip has completed.
    AfterFlip,
    /// Leave it: the kernel turns the plane off when its buffers are freed.
    GiveUp,
}

/// Only a pending flip (`EBUSY`) is worth waiting for; nothing ever blocks the loop (S5).
fn after_refused_off(errno: Option<i32>, waiting_flip: bool) -> OffNext {
    if waiting_flip && errno == Some(libc::EBUSY) {
        OffNext::AfterFlip
    } else {
        OffNext::GiveUp
    }
}

/// RubickWay directly on the hardware.
pub struct DrmBackend {
    // Field order matters for drop (after `Drop::drop` turned the outputs off): the EGL
    // context, then the card, then input devices, then the session.
    outputs: Vec<Output>,
    retiring: Vec<Retiring>,
    egl: Egl,
    gbm: gbm::Device<Card>,
    card_path: PathBuf,
    input: InputContext,
    keyboard: DrmKeyboard,
    session: SharedSession,
    events: Vec<BackendEvent>,
    /// Output ids are never reused while RubickWay runs.
    next_id: u32,
    paused: bool,
    /// Display hotplug; without it, displays are only read at startup and on resume.
    hotplug: Option<hotplug::Hotplug>,
}

impl DrmBackend {
    /// Takes the session, the graphics card and the input devices, and finds the displays.
    pub fn new() -> Result<DrmBackend, DrmError> {
        let session: SharedSession = Rc::new(RefCell::new(Session::open()?));
        let card_path = gpu::primary_gpu()?;
        let gpu_err = |reason: String| DrmError::Gpu {
            path: card_path.display().to_string(),
            reason,
        };
        let fd = session
            .borrow_mut()
            .open_device(&card_path)
            .map_err(|e| gpu_err(e.to_string()))?;
        let gbm = gbm::Device::new(Card(fd)).map_err(|e| gpu_err(format!("GBM: {e}")))?;
        device::enable_atomic(&gbm).map_err(|e| gpu_err(format!("no atomic modesetting: {e}")))?;
        let egl = Egl::gbm(&gbm).map_err(|e| gpu_err(format!("EGL: {e}")))?;
        let keyboard = DrmKeyboard::new()
            .ok_or_else(|| DrmError::Input("the US English keymap does not compile".into()))?;
        let input = InputContext::new(Rc::clone(&session))?;
        let hotplug = hotplug::Hotplug::new(&card_path)
            .inspect_err(|e| tracing::warn!("display hotplug is off: {e}"))
            .ok();
        let mut backend = DrmBackend {
            outputs: Vec::new(),
            retiring: Vec::new(),
            egl,
            gbm,
            card_path,
            input,
            keyboard,
            session,
            events: Vec::new(),
            next_id: 1,
            paused: false,
            hotplug,
        };
        backend
            .events
            .push(BackendEvent::Keymap(backend.keyboard.keymap_text()));
        backend.events.push(BackendEvent::RepeatInfo {
            rate: REPEAT_RATE,
            delay: REPEAT_DELAY,
        });
        // The keyboard is RubickWay's from the start: the shell routes keys to windows only
        // while it has the keyboard (research S23).
        backend
            .events
            .push(BackendEvent::Input(InputEvent::FocusGained));
        backend.add_outputs()?;
        Ok(backend)
    }

    /// Starts driving every connected display not driven yet (startup, resume, hotplug).
    fn add_outputs(&mut self) -> Result<(), DrmError> {
        let taken: Vec<_> = self
            .outputs
            .iter()
            .map(|o| o.found.crtc)
            .chain(self.retiring.iter().map(|r| r.found.crtc))
            .collect();
        let found = device::scan(&self.gbm, &taken).map_err(|e| self.lost(e))?;
        for f in found {
            // A connector still waiting to be turned off is added after that (`retire`), so
            // the late off-commit never turns off its new output.
            if self
                .outputs
                .iter()
                .map(|o| o.found.connector)
                .chain(self.retiring.iter().map(|r| r.found.connector))
                .any(|c| c == f.connector)
            {
                continue;
            }
            let (w, h) = f.mode.size();
            let surface = match OutputSurface::new(&self.gbm, &self.egl, w.into(), h.into()) {
                Ok(s) => s,
                Err(reason) => {
                    tracing::warn!("display {} stays dark: {reason}", f.info.name);
                    continue;
                }
            };
            let id = ViewerId(self.next_id);
            self.next_id += 1;
            self.events.push(BackendEvent::OutputAdded {
                id,
                info: f.info.clone(),
            });
            self.outputs.push(Output {
                id,
                found: f,
                surface,
                waiting_flip: false,
                needs_modeset: true,
                retrying: false,
            });
        }
        Ok(())
    }

    /// Stops driving displays that are no longer connected: each is turned off without
    /// waiting (after its pending flip, if one refuses the commit) and reported as
    /// `OutputRemoved` (S5, S13).
    fn remove_gone(&mut self) -> Result<(), DrmError> {
        let connected = device::connected(&self.gbm).map_err(|e| self.lost(e))?;
        let driven: Vec<u32> = self
            .outputs
            .iter()
            .map(|o| u32::from(o.found.connector))
            .collect();
        let gone = hotplug::gone(&driven, &connected);
        if gone.is_empty() {
            return Ok(());
        }
        // No output surface may stay current while it is destroyed.
        let _ = self.egl.make_current(None);
        for connector in gone {
            let Some(i) = self
                .outputs
                .iter()
                .position(|o| u32::from(o.found.connector) == connector)
            else {
                continue;
            };
            let o = self.outputs.remove(i);
            self.events.push(BackendEvent::OutputRemoved { id: o.id });
            let refused = if self.paused {
                None
            } else {
                device::disable(&self.gbm, &o.found, false).err()
            };
            let Some(err) = refused else {
                o.surface.destroy(&self.gbm, &self.egl);
                continue;
            };
            match after_refused_off(err.raw_os_error(), o.waiting_flip) {
                OffNext::AfterFlip => {
                    tracing::debug!(
                        "display {}: turned off after its pending flip",
                        o.found.info.name
                    );
                    self.retiring.push(Retiring {
                        found: o.found,
                        surface: o.surface,
                    });
                }
                OffNext::GiveUp => {
                    tracing::warn!("display {}: cannot turn it off ({err})", o.found.info.name);
                    o.surface.destroy(&self.gbm, &self.egl);
                }
            }
        }
        Ok(())
    }

    /// Turns off a display that went away once its pending flip has completed (S5).
    /// Then rescans, in case its connector was plugged in again meanwhile.
    fn retire(&mut self, crtc: drm::control::crtc::Handle) -> Result<(), DrmError> {
        let Some(i) = self.retiring.iter().position(|r| r.found.crtc == crtc) else {
            return Ok(());
        };
        let r = self.retiring.remove(i);
        if let Err(err) = device::disable(&self.gbm, &r.found, false) {
            tracing::warn!("display {}: cannot turn it off ({err})", r.found.info.name);
        }
        let _ = self.egl.make_current(None);
        r.surface.destroy(&self.gbm, &self.egl);
        self.add_outputs()
    }

    /// Frees displays still waiting to be turned off (pause, exit).
    fn drop_retiring(&mut self) {
        if self.retiring.is_empty() {
            return;
        }
        let _ = self.egl.make_current(None);
        for r in std::mem::take(&mut self.retiring) {
            r.surface.destroy(&self.gbm, &self.egl);
        }
    }

    /// The hotplug monitor's fd, for the event loop (`None` without hotplug).
    pub fn hotplug_fd(&self) -> Option<OwnedFd> {
        self.hotplug.as_ref().and_then(|h| h.fd().ok())
    }

    /// Reads hotplug events: on a change of the card, removes displays that went away and
    /// adds new ones (FR-021). While paused, the change waits for the resume.
    pub fn dispatch_hotplug(&mut self) -> Result<(), DrmError> {
        let changed = self.hotplug.as_mut().is_some_and(hotplug::Hotplug::changed);
        if !changed || self.paused {
            return Ok(());
        }
        self.remove_gone()?;
        self.add_outputs()
    }

    fn lost(&self, err: io::Error) -> DrmError {
        DrmError::Lost(format!("{}: {err}", self.card_path.display()))
    }

    /// The device turned out unusable while starting, e.g. OpenGL ES cannot be set up on it
    /// (contracts/cli.md).
    pub fn unusable(&self, reason: impl std::fmt::Display) -> DrmError {
        DrmError::Gpu {
            path: self.card_path.display().to_string(),
            reason: reason.to_string(),
        }
    }

    /// Duplicates of the session's, the card's and libinput's fds, for the event loop.
    pub fn fds(&self) -> io::Result<(OwnedFd, OwnedFd, OwnedFd)> {
        let session = self.session.borrow_mut().fd()?.try_clone_to_owned()?;
        let card = self.gbm.as_fd().try_clone_to_owned()?;
        let input = self.input.fd()?;
        Ok((session, card, input))
    }

    /// The GPU's render node and its device number (research G2).
    pub fn render_node(&self) -> io::Result<(PathBuf, u64)> {
        gpu::render_node_of(&self.card_path)
    }

    /// Explicit synchronisation on the GPU's render node; `None` if its driver has no timelines.
    pub fn timelines(&self) -> io::Result<Option<Rc<dyn crate::server::Timelines>>> {
        let (node, _) = self.render_node()?;
        let timelines = timelines::RenderNodeTimelines::open(&node)?;
        Ok(timelines.map(|t| Rc::new(t) as Rc<dyn crate::server::Timelines>))
    }

    /// The EGL display and context all outputs draw with.
    pub fn egl(&self) -> &Egl {
        &self.egl
    }

    /// The logical size of display `id` (its mode at scale 1).
    pub fn output_size(&self, id: ViewerId) -> Option<Size> {
        self.output(id).map(|o| {
            let (w, h) = o.found.mode.size();
            Size::new(f64::from(w), f64::from(h))
        })
    }

    /// The display's refresh interval in milliseconds (for the estimated vblank, S7).
    pub fn refresh_interval_ms(&self, id: ViewerId) -> u32 {
        self.output(id)
            .map(|o| o.found.info.mode.refresh_mhz)
            .filter(|mhz| *mhz > 0)
            .map_or(16, |mhz| (1_000_000 / mhz).max(1))
    }

    fn output(&self, id: ViewerId) -> Option<&Output> {
        self.outputs.iter().find(|o| o.id == id)
    }

    /// Events collected since the last call.
    pub fn take_events(&mut self) -> Vec<BackendEvent> {
        std::mem::take(&mut self.events)
    }

    /// Reads the session's news: pause and resume (S12).
    pub fn dispatch_session(&mut self) -> Result<(), DrmError> {
        let news = self
            .session
            .borrow_mut()
            .dispatch()
            .map_err(DrmError::SessionLost)?;
        for event in news {
            match event {
                SessionEvent::Disabled if !self.paused => {
                    self.paused = true;
                    self.input.suspend();
                    self.keyboard.reset();
                    for o in &mut self.outputs {
                        o.surface.discard_pending();
                        o.waiting_flip = false;
                    }
                    // Their flips will not be reported; the next master resets the CRTCs.
                    self.drop_retiring();
                    self.session.borrow_mut().acknowledge_disable();
                    self.events
                        .push(BackendEvent::Input(InputEvent::PointerLeave));
                    self.events.push(BackendEvent::Input(InputEvent::FocusLost));
                    self.events.push(BackendEvent::Paused);
                    tracing::info!("paused");
                }
                SessionEvent::Enabled if self.paused => {
                    self.paused = false;
                    self.input.resume();
                    for o in &mut self.outputs {
                        o.needs_modeset = true;
                        o.waiting_flip = false;
                    }
                    // Displays may have come and gone while another console was shown.
                    self.remove_gone()?;
                    self.add_outputs()?;
                    self.events.push(BackendEvent::Resumed);
                    self.events
                        .push(BackendEvent::Input(InputEvent::FocusGained));
                    tracing::info!("resumed");
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Reads page-flip events; `now_ms` is the frame clock (S5, S7).
    pub fn dispatch_card(&mut self, now_ms: u32) -> Result<(), DrmError> {
        let crtcs = device::flipped(&self.gbm).map_err(|e| self.lost(e))?;
        for crtc in crtcs {
            self.retire(crtc)?;
            if let Some(o) = self.outputs.iter_mut().find(|o| o.found.crtc == crtc) {
                o.surface.flipped();
                o.waiting_flip = false;
                self.events.push(BackendEvent::Presented {
                    id: o.id,
                    time_ms: now_ms,
                });
            }
        }
        Ok(())
    }

    /// Reads input devices (S8); Ctrl+Alt+F<n> switches consoles here (S11).
    pub fn dispatch_input(&mut self) -> Result<(), DrmError> {
        for raw in self.input.dispatch()? {
            match translate(raw) {
                Some(Translated::Input(event)) => self.events.push(BackendEvent::Input(event)),
                Some(Translated::Key {
                    code,
                    pressed,
                    time_ms,
                }) => match self.keyboard.key(code, pressed, time_ms) {
                    KeyOutcome::SwitchConsole(vt) => self.session.borrow_mut().switch_vt(vt),
                    KeyOutcome::Events(events) => self
                        .events
                        .extend(events.into_iter().map(BackendEvent::Input)),
                },
                None => {}
            }
        }
        Ok(())
    }

    /// Whether display `id` can take a new frame now (active, not waiting for a flip).
    pub fn can_draw(&self, id: ViewerId) -> bool {
        !self.paused && self.output(id).is_some_and(|o| !o.waiting_flip)
    }

    /// Makes display `id`'s surface current for drawing.
    pub fn begin_frame(&self, id: ViewerId) -> Result<(), DrmError> {
        let Some(o) = self.output(id) else {
            return Ok(());
        };
        self.egl
            .make_current(Some(o.surface.egl_surface()))
            .map_err(|e| DrmError::Lost(format!("EGL: {e}")))
    }

    /// Makes the context current without a surface (uploads with no display, S6).
    pub fn make_current_none(&self) -> Result<(), DrmError> {
        self.egl
            .make_current(None)
            .map_err(|e| DrmError::Lost(format!("EGL: {e}")))
    }

    /// Shows what was drawn on display `id`: swap, lock, commit (non-blocking). Returns
    /// whether the frame was committed; a refused commit (`EBUSY`) is retried by drawing
    /// again later, any other failure once as a modeset; a display whose retry fails too
    /// stays dark, and when no display is left the device counts as lost (S5).
    pub fn finish_frame(&mut self, id: ViewerId) -> Result<bool, DrmError> {
        let Some(i) = self.outputs.iter().position(|o| o.id == id) else {
            return Ok(false);
        };
        let egl_surface = self.outputs[i].surface.egl_surface();
        let started = std::time::Instant::now();
        self.egl
            .swap_buffers(egl_surface)
            .map_err(|e| DrmError::Lost(format!("EGL: {e}")))?;
        let swapped = started.elapsed();
        let fb = match self.outputs[i].surface.lock_front(&self.gbm) {
            Ok(fb) => fb,
            Err(err) => {
                tracing::warn!(
                    "display {}: no front buffer: {err}",
                    self.outputs[i].found.info.name
                );
                return Ok(false);
            }
        };
        let o = &mut self.outputs[i];
        let commit = device::commit(&self.gbm, &o.found, fb, o.needs_modeset);
        if started.elapsed().as_millis() >= 5 {
            tracing::debug!(
                "showing a frame: swap {} ms, then commit {} ms",
                swapped.as_millis(),
                started.elapsed().saturating_sub(swapped).as_millis()
            );
        }
        match commit {
            Ok(()) => {
                o.waiting_flip = true;
                o.needs_modeset = false;
                o.retrying = false;
                Ok(true)
            }
            Err(err) => {
                o.surface.discard_pending();
                match after_failed_commit(err.raw_os_error(), o.retrying) {
                    CommitNext::Later => Ok(false),
                    CommitNext::Lost => Err(DrmError::Lost(err.to_string())),
                    CommitNext::RetryModeset => {
                        tracing::warn!(
                            "display {}: commit failed ({err}); retrying with a modeset",
                            o.found.info.name
                        );
                        o.needs_modeset = true;
                        o.retrying = true;
                        Ok(false)
                    }
                    CommitNext::GiveUp => {
                        let o = self.outputs.remove(i);
                        tracing::warn!(
                            "display {} stays dark: the modeset retry failed too ({err}); it is \
                             tried again when a display is plugged in or out, or after a console \
                             switch",
                            o.found.info.name
                        );
                        self.events.push(BackendEvent::OutputRemoved { id: o.id });
                        // Nothing is pending on it (the commit failed): off without waiting.
                        if let Err(off) = device::disable(&self.gbm, &o.found, false) {
                            tracing::debug!("display {}: off failed ({off})", o.found.info.name);
                        }
                        let _ = self.egl.make_current(None);
                        o.surface.destroy(&self.gbm, &self.egl);
                        if self.outputs.is_empty() {
                            return Err(DrmError::Lost(format!(
                                "no display accepts a frame: {err}"
                            )));
                        }
                        Ok(false)
                    }
                }
            }
        }
    }
}

impl Drop for DrmBackend {
    fn drop(&mut self) {
        // Everything off (may block: the loop has ended), then the surfaces before EGL.
        if !self.paused {
            for found in self
                .outputs
                .iter()
                .map(|o| &o.found)
                .chain(self.retiring.iter().map(|r| &r.found))
            {
                let _ = device::disable(&self.gbm, found, true);
            }
        }
        self.drop_retiring();
        let _ = self.egl.make_current(None);
        for o in self.outputs.drain(..) {
            o.surface.destroy(&self.gbm, &self.egl);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ten_failed_reads_in_a_row_count_as_broken() {
        let mut f = ReadFailures::default();
        for _ in 0..9 {
            assert!(!f.note(true));
        }
        assert!(!f.note(false), "a good read starts over");
        for _ in 0..9 {
            assert!(!f.note(true));
        }
        assert!(f.note(true));
    }

    #[test]
    fn only_a_pending_flip_is_waited_for_before_turning_off() {
        assert_eq!(
            after_refused_off(Some(libc::EBUSY), true),
            OffNext::AfterFlip
        );
        assert_eq!(after_refused_off(Some(libc::EBUSY), false), OffNext::GiveUp);
        assert_eq!(after_refused_off(Some(libc::EINVAL), true), OffNext::GiveUp);
        assert_eq!(after_refused_off(None, true), OffNext::GiveUp);
    }

    #[test]
    fn a_failed_commit_is_retried_once_as_a_modeset() {
        assert_eq!(
            after_failed_commit(Some(libc::EBUSY), false),
            CommitNext::Later
        );
        assert_eq!(
            after_failed_commit(Some(libc::EBUSY), true),
            CommitNext::Later
        );
        assert_eq!(
            after_failed_commit(Some(libc::ENODEV), false),
            CommitNext::Lost
        );
        assert_eq!(
            after_failed_commit(Some(libc::EINVAL), false),
            CommitNext::RetryModeset
        );
        assert_eq!(
            after_failed_commit(Some(libc::EINVAL), true),
            CommitNext::GiveUp
        );
        assert_eq!(after_failed_commit(None, true), CommitNext::GiveUp);
    }
}
