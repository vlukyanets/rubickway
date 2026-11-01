//! The shell: windows on the canvas, focus and input routing. Pure core (Principle IV): no
//! Wayland, GL or device types.
//!
//! The server reports windows as [`WindowEvent`]s, which the application applies here; the
//! shell never calls the server (research R7). Input is routed per contracts/input.md: Mod
//! gestures, pinch and the background go to the viewer (canvas navigation), everything else to
//! the window under the pointer or with keyboard focus, as [`ClientInput`]s.

mod route;
mod windows;

pub use route::ClientInput;
pub use windows::*;

use route::{Grab, Router};

use crate::canvas::{Point, Rect, Size, Vector};
use crate::input::bindings;
use crate::input::{Button, InputEvent, Keysym, ScrollSource};
use crate::viewer::{Effect, Viewer};

/// A change to the windows, reported by the server.
#[derive(Debug, Clone, PartialEq)]
pub enum WindowEvent {
    /// A toplevel showed its first content.
    Mapped {
        id: WindowId,
        size: Size,
        input: Vec<Rect>,
    },
    /// Its geometry size or input shape changed.
    Updated {
        id: WindowId,
        size: Size,
        input: Vec<Rect>,
    },
    /// It was hidden, destroyed, or its application disconnected.
    Unmapped { id: WindowId },
}

/// The viewer, the windows it shows, and input routing.
#[derive(Debug, Clone)]
pub struct Shell {
    viewer: Viewer,
    windows: Windows,
    router: Router,
    dirty: bool,
}

impl Default for Shell {
    fn default() -> Self {
        Self::new()
    }
}

impl Shell {
    pub fn new() -> Shell {
        Shell {
            viewer: Viewer::new(),
            windows: Windows::default(),
            router: Router::default(),
            dirty: true,
        }
    }

    pub fn viewer(&self) -> &Viewer {
        &self.viewer
    }

    pub fn viewer_mut(&mut self) -> &mut Viewer {
        &mut self.viewer
    }

    pub fn windows(&self) -> &Windows {
        &self.windows
    }

    /// Applies a window change from the server; returns the resulting focus change, if any.
    pub fn apply(&mut self, event: WindowEvent) -> Vec<ClientInput> {
        let before = self.windows.focus();
        match event {
            WindowEvent::Mapped { id, size, input } => {
                let center = self.viewer.camera().center;
                self.windows.map(id, size, input, center);
            }
            WindowEvent::Updated { id, size, input } => {
                self.windows.update(id, size, input);
            }
            WindowEvent::Unmapped { id } => {
                self.windows.unmap(id);
                if self.router.grab.as_ref().is_some_and(|g| g.window == id) {
                    self.router.grab = None;
                }
                if self.router.entered == Some(id) {
                    self.router.entered = None;
                }
                if before == Some(id) {
                    // Keys held for it are forgotten (misbehaving case 3).
                    self.router.held_keys.clear();
                }
            }
        }
        self.dirty = true;
        let mut out = Vec::new();
        self.focus_changed(before, &mut out);
        out
    }

    /// The canvas rectangle the viewer shows.
    pub fn view_rect(&self) -> Rect {
        let size = self.viewer.size();
        let camera = self.viewer.camera();
        let top_left = camera.to_canvas(Point::new(0.0, 0.0), size);
        Rect::new(
            top_left,
            Size::new(size.w / camera.zoom, size.h / camera.zoom),
        )
    }

    /// Windows that at least partly intersect the view, back to front (FR-006).
    pub fn visible(&self) -> Vec<WindowId> {
        self.windows.visible(self.view_rect())
    }

    /// Whether a new frame is needed (the view or the windows changed); clears the flag.
    pub fn take_dirty(&mut self) -> bool {
        let viewer = self.viewer.take_dirty();
        std::mem::take(&mut self.dirty) || viewer
    }

    /// Handles one input event: canvas navigation or input for an application.
    pub fn handle(&mut self, event: &InputEvent) -> (Effect, Vec<ClientInput>) {
        let mut out = Vec::new();
        let effect = match *event {
            InputEvent::PointerEnter { pos } | InputEvent::PointerMotion { pos } => {
                self.pointer_moved(event, pos, &mut out);
                Effect::default()
            }
            InputEvent::PointerLeave => {
                // During a grab the host keeps sending to us, so a leave means it broke the
                // grab: it ends.
                self.router.grab = None;
                self.leave(&mut out);
                self.viewer.handle(event)
            }
            InputEvent::PointerButton {
                button,
                code,
                pressed,
            } => self.button(event, button, code, pressed, &mut out),
            InputEvent::Scroll {
                source,
                delta,
                value120,
            } => self.scroll(event, source, delta, value120, &mut out),
            InputEvent::PinchBegin
            | InputEvent::PinchUpdate { .. }
            | InputEvent::PinchEnd { .. } => self.viewer.handle(event),
            InputEvent::Key {
                keycode,
                binding_sym,
                pressed,
            } => self.key(event, keycode, binding_sym, pressed, &mut out),
            InputEvent::Modifiers { mods, raw } => {
                self.router.mods = mods;
                if let Some(window) = self.focused_window() {
                    out.push(ClientInput::Modifiers { window, raw });
                }
                self.viewer.handle(event)
            }
            InputEvent::FocusLost => {
                if self.router.viewer_focused && self.windows.focus().is_some() {
                    out.push(ClientInput::KeyboardFocus(None));
                }
                self.router.viewer_focused = false;
                self.router.grab = None;
                self.router.held_keys.clear();
                self.router.consumed_keys.clear();
                self.viewer.handle(event)
            }
            InputEvent::FocusGained => {
                if !self.router.viewer_focused && self.windows.focus().is_some() {
                    out.push(ClientInput::KeyboardFocus(self.windows.focus()));
                }
                self.router.viewer_focused = true;
                self.viewer.handle(event)
            }
        };
        (effect, out)
    }

    /// The focused window, if the viewer has the keyboard.
    fn focused_window(&self) -> Option<WindowId> {
        self.windows.focus().filter(|_| self.router.viewer_focused)
    }

    fn focus_changed(&self, before: Option<WindowId>, out: &mut Vec<ClientInput>) {
        let now = self.windows.focus();
        if now != before && self.router.viewer_focused {
            out.push(ClientInput::KeyboardFocus(now));
        }
    }

    /// The pointer position on the canvas, if it is over the viewer.
    fn pointer_canvas(&self) -> Option<Point> {
        let pos = self.viewer.pointer()?;
        Some(self.viewer.camera().to_canvas(pos, self.viewer.size()))
    }

    /// `p` (canvas) in the coordinates of `window`.
    fn local(&self, window: WindowId, p: Point) -> Option<Point> {
        let w = self.windows.get(window)?;
        Some(Point::new(p.x - w.origin.x, p.y - w.origin.y))
    }

    fn pointer_moved(&mut self, event: &InputEvent, pos: Point, out: &mut Vec<ClientInput>) {
        self.viewer.handle(event);
        if self.viewer.is_panning() {
            // A canvas pan: applications get nothing (FR-012).
            return;
        }
        let canvas = self.viewer.camera().to_canvas(pos, self.viewer.size());
        if let Some(grab) = &self.router.grab {
            let window = grab.window;
            if let Some(local) = self.local(window, canvas) {
                out.push(ClientInput::PointerMotion { window, pos: local });
                out.push(ClientInput::PointerFrame { window });
            }
            return;
        }
        self.retarget(canvas, true, out);
    }

    /// Points the pointer at the window under canvas point `p` (enter/leave as needed); with
    /// `motion`, also reports the motion inside an unchanged window.
    fn retarget(&mut self, p: Point, motion: bool, out: &mut Vec<ClientInput>) {
        let hit = self.windows.hit(p);
        match (self.router.entered, hit) {
            (Some(old), Some((id, local))) if old == id => {
                if motion {
                    out.push(ClientInput::PointerMotion {
                        window: id,
                        pos: local,
                    });
                    out.push(ClientInput::PointerFrame { window: id });
                }
            }
            (_, hit) => {
                self.leave(out);
                if let Some((window, pos)) = hit {
                    out.push(ClientInput::PointerEnter { window, pos });
                    out.push(ClientInput::PointerFrame { window });
                    self.router.entered = Some(window);
                }
            }
        }
    }

    fn leave(&mut self, out: &mut Vec<ClientInput>) {
        if let Some(window) = self.router.entered.take() {
            out.push(ClientInput::PointerLeave { window });
            out.push(ClientInput::PointerFrame { window });
        }
    }

    fn button(
        &mut self,
        event: &InputEvent,
        button: Button,
        code: u32,
        pressed: bool,
        out: &mut Vec<ClientInput>,
    ) -> Effect {
        if pressed {
            if let Some(grab) = &mut self.router.grab {
                grab.buttons.push(code);
                let window = grab.window;
                out.push(ClientInput::PointerButton {
                    window,
                    code,
                    pressed,
                });
                out.push(ClientInput::PointerFrame { window });
                return Effect::default();
            }
            if button == Button::Left && bindings::mod_held(self.router.mods) {
                // Mod + left pans anywhere and does not focus (FR-012).
                self.leave(out);
                return self.viewer.handle(event);
            }
            let hit = self.pointer_canvas().and_then(|p| self.windows.hit(p));
            let Some((window, local)) = hit else {
                // The background: left pans, other buttons do nothing; focus stays (FR-013).
                return if button == Button::Left {
                    self.viewer.handle(event)
                } else {
                    Effect::default()
                };
            };
            let before = self.windows.focus();
            self.windows.raise_and_focus(window);
            self.dirty = true;
            self.focus_changed(before, out);
            if self.router.entered != Some(window) {
                self.leave(out);
                out.push(ClientInput::PointerEnter { window, pos: local });
                self.router.entered = Some(window);
            }
            self.router.grab = Some(Grab {
                window,
                buttons: vec![code],
            });
            out.push(ClientInput::PointerButton {
                window,
                code,
                pressed,
            });
            out.push(ClientInput::PointerFrame { window });
            return Effect::default();
        }

        if let Some(grab) = &mut self.router.grab {
            grab.buttons.retain(|b| *b != code);
            let (window, done) = (grab.window, grab.buttons.is_empty());
            out.push(ClientInput::PointerButton {
                window,
                code,
                pressed,
            });
            out.push(ClientInput::PointerFrame { window });
            if done {
                self.router.grab = None;
                if let Some(p) = self.pointer_canvas() {
                    self.retarget(p, false, out);
                }
            }
            return Effect::default();
        }
        let was_panning = self.viewer.is_panning();
        let effect = self.viewer.handle(event);
        if was_panning
            && !self.viewer.is_panning()
            && let Some(p) = self.pointer_canvas()
        {
            self.retarget(p, false, out);
        }
        effect
    }

    fn scroll(
        &mut self,
        event: &InputEvent,
        source: ScrollSource,
        delta: Vector,
        value120: Option<Vector>,
        out: &mut Vec<ClientInput>,
    ) -> Effect {
        if self.viewer.is_panning() || bindings::mod_held(self.router.mods) {
            return self.viewer.handle(event);
        }
        let target = match &self.router.grab {
            Some(grab) => Some(grab.window),
            None => self
                .pointer_canvas()
                .and_then(|p| self.windows.hit(p))
                .map(|(id, _)| id),
        };
        match target {
            Some(window) => {
                out.push(ClientInput::Scroll {
                    window,
                    source,
                    delta,
                    value120,
                });
                out.push(ClientInput::PointerFrame { window });
                Effect::default()
            }
            None => self.viewer.handle(event),
        }
    }

    fn key(
        &mut self,
        event: &InputEvent,
        keycode: u32,
        sym: Keysym,
        pressed: bool,
        out: &mut Vec<ClientInput>,
    ) -> Effect {
        if pressed {
            if bindings::lookup(self.router.mods, sym, true).is_some() {
                self.router.consumed_keys.push(keycode);
                return self.viewer.handle(event);
            }
            if let Some(window) = self.focused_window() {
                self.router.held_keys.push(keycode);
                out.push(ClientInput::Key {
                    window,
                    keycode,
                    pressed,
                });
            }
            return Effect::default();
        }
        if let Some(i) = self.router.consumed_keys.iter().position(|k| *k == keycode) {
            self.router.consumed_keys.remove(i);
            return Effect::default();
        }
        if let Some(i) = self.router.held_keys.iter().position(|k| *k == keycode) {
            self.router.held_keys.remove(i);
            if let Some(window) = self.focused_window() {
                out.push(ClientInput::Key {
                    window,
                    keycode,
                    pressed,
                });
            }
        }
        Effect::default()
    }
}
