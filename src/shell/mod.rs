//! The shell: windows on the canvas, focus and input routing. Pure core (Principle IV): no
//! Wayland, GL or device types.
//!
//! The server reports windows and applications' requests as [`WindowEvent`]s, which the
//! application applies here; the shell never calls the server (client-windows research R7) and
//! answers with [`WindowCommand`]s instead (window-management research W1). Input is routed per
//! contracts/input.md: Mod gestures, pinch and the background go to the viewer (canvas
//! navigation), Mod + Shift + left and Mod + right on a window move and resize it, Mod+Tab
//! cycles focus, everything else goes to the window under the pointer or with keyboard focus,
//! as [`ClientInput`]s. Maximize and fullscreen are decided here too (research W7, W8).

mod geometry;
mod route;
mod windows;

pub use geometry::*;
pub use route::ClientInput;
pub use windows::*;

use route::{Grab, Op, Operation, Router};

use crate::canvas::{Camera, Point, Rect, Size, Vector};
use crate::input::bindings::{self, Action};
use crate::input::{Button, InputEvent, Keysym, ScrollSource};
use crate::viewer::{Effect, Viewer};

/// A change to the windows, or a request from an application, reported by the server.
#[derive(Debug, Clone, PartialEq)]
pub enum WindowEvent {
    /// A toplevel that asked for an initial state made its initial commit; the shell must
    /// answer with a [`WindowCommand::Configure`] for `id` (research W13). `Mapped` later
    /// uses the same id.
    Created {
        id: WindowId,
        parent: Option<WindowId>,
        limits: SizeLimits,
        maximized: bool,
        fullscreen: bool,
    },
    /// A toplevel showed its first content.
    Mapped {
        id: WindowId,
        size: Size,
        input: Vec<Rect>,
        limits: SizeLimits,
        parent: Option<WindowId>,
    },
    /// Its geometry size, input shape or size limits changed, or it applied the newest
    /// configure the shell asked for (`configured`).
    Updated {
        id: WindowId,
        size: Size,
        input: Vec<Rect>,
        limits: SizeLimits,
        configured: bool,
    },
    /// It was hidden, destroyed, or its application disconnected.
    Unmapped { id: WindowId },
    /// The application asked to start an interactive move (serial already checked).
    MoveRequested { id: WindowId },
    /// The application asked to start an interactive resize (serial already checked).
    ResizeRequested { id: WindowId, edges: Edges },
    /// The application asked to be maximized (`on`) or restored.
    MaximizeRequested { id: WindowId, on: bool },
    /// The application asked to go fullscreen (`on`) or leave it.
    FullscreenRequested { id: WindowId, on: bool },
}

/// States the shell asks a window to have (`activated` follows keyboard focus).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct States {
    pub maximized: bool,
    pub fullscreen: bool,
    pub resizing: bool,
}

/// What the shell asks the server to do with a window (research W1).
#[derive(Debug, Clone, PartialEq)]
pub enum WindowCommand {
    /// Ask for this size (`None`: the application chooses) and these states.
    Configure {
        window: WindowId,
        size: Option<Size>,
        states: States,
    },
    /// Ask the application to close the window.
    Close { window: WindowId },
}

/// The pointer cursor the shell wants (research W11); `App` maps it to the backend's cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorShape {
    Default,
    /// A canvas pan.
    Grabbing,
    /// An interactive move.
    Move,
    /// An interactive resize from these edges.
    Resize(Edges),
}

/// The viewer, the windows it shows, and input routing.
#[derive(Debug, Clone)]
pub struct Shell {
    viewer: Viewer,
    windows: Windows,
    router: Router,
    commands: Vec<WindowCommand>,
    /// Windows whose resize ended; their anchor returns to the top-left once they apply the
    /// final configure (research W4).
    settling: Vec<WindowId>,
    /// The largest window size that can be displayed (research W6).
    largest: Size,
    /// The fullscreen window, if any (research W8).
    fullscreen: Option<Fullscreen>,
    /// Initial states answered at `Created`, entered at `Mapped` (research W13).
    initial: Vec<Initial>,
    dirty: bool,
}

/// A fullscreen window and what to restore when it leaves fullscreen.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Fullscreen {
    window: WindowId,
    /// Its rect before (for a maximized window, the maximized rect).
    saved_rect: Rect,
    /// The view before; always the one from before any window was fullscreen.
    saved_camera: Camera,
}

/// A state a window asked for before it was shown, granted at `Created`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Initial {
    window: WindowId,
    limits: SizeLimits,
    maximized: bool,
    fullscreen: bool,
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
            commands: Vec::new(),
            settling: Vec::new(),
            largest: Size {
                w: f64::INFINITY,
                h: f64::INFINITY,
            },
            fullscreen: None,
            initial: Vec::new(),
            dirty: true,
        }
    }

    /// The fullscreen window, if any.
    pub fn fullscreen(&self) -> Option<WindowId> {
        self.fullscreen.map(|f| f.window)
    }

    /// Commands for the server queued since the last call.
    pub fn take_commands(&mut self) -> Vec<WindowCommand> {
        std::mem::take(&mut self.commands)
    }

    /// The cursor to show.
    pub fn cursor(&self) -> CursorShape {
        match &self.router.operation {
            Some(Operation {
                op: Op::Move { .. },
                ..
            }) => CursorShape::Move,
            Some(Operation {
                op: Op::Resize { edges, .. },
                ..
            }) => CursorShape::Resize(*edges),
            None if self.viewer.is_panning() => CursorShape::Grabbing,
            None => CursorShape::Default,
        }
    }

    /// The viewer window got a new logical size: maximized and fullscreen windows follow it.
    pub fn viewer_configured(&mut self, size: Size) {
        let before = self.viewer.size();
        self.viewer.configure(size);
        if self.viewer.size() == before {
            return;
        }
        let maximized: Vec<WindowId> = self
            .windows
            .iter()
            .filter(|w| matches!(w.mode, Mode::Maximized { .. }) || self.is_fullscreen(w.id))
            .map(|w| w.id)
            .collect();
        for id in maximized {
            self.configure_viewer_size(id);
        }
        for init in self.initial.clone() {
            self.commands.push(WindowCommand::Configure {
                window: init.window,
                size: Some(init.limits.clamp(self.viewer.size(), self.largest)),
                states: States {
                    maximized: init.maximized,
                    fullscreen: init.fullscreen,
                    resizing: false,
                },
            });
        }
    }

    fn is_fullscreen(&self, id: WindowId) -> bool {
        self.fullscreen.is_some_and(|f| f.window == id)
    }

    /// Asks a maximized or fullscreen window for the viewer size.
    fn configure_viewer_size(&mut self, id: WindowId) {
        let size = self.clamp_size(id, self.viewer.size());
        self.commands.push(WindowCommand::Configure {
            window: id,
            size: Some(size),
            states: self.states_of(id, false),
        });
    }

    /// The largest window the renderer can display, in logical pixels (research W6).
    pub fn set_largest_window(&mut self, size: Size) {
        self.largest = size;
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
        let mut out = Vec::new();
        match event {
            WindowEvent::Created {
                id,
                parent,
                limits,
                maximized,
                fullscreen,
            } => self.created(id, parent, limits, maximized, fullscreen),
            WindowEvent::Mapped {
                id,
                size,
                input,
                limits,
                parent,
            } => self.mapped(id, size, input, limits, parent, &mut out),
            WindowEvent::Updated {
                id,
                size,
                input,
                limits,
                configured,
            } => {
                self.windows.update(id, size, input);
                self.windows.set_limits(id, limits);
                if configured && let Some(i) = self.settling.iter().position(|w| *w == id) {
                    // The size asked for after a resize or a restore arrived: back to the
                    // top-left anchor, and the application chooses its size again.
                    self.settling.remove(i);
                    self.windows.set_anchor(id, Anchor::TopLeft);
                    self.commands.push(WindowCommand::Configure {
                        window: id,
                        size: None,
                        states: self.states_of(id, false),
                    });
                }
            }
            WindowEvent::MoveRequested { id } => self.move_requested(id, &mut out),
            WindowEvent::ResizeRequested { id, edges } => {
                self.resize_requested(id, edges, &mut out);
            }
            // Only the focused window may change these states (FR-019).
            WindowEvent::MaximizeRequested { id, on } if self.windows.focus() == Some(id) => {
                if on {
                    self.maximize(id, true, true, &mut out);
                } else {
                    self.unmaximize(id);
                }
            }
            WindowEvent::FullscreenRequested { id, on } if self.windows.focus() == Some(id) => {
                if on {
                    self.enter_fullscreen(id, true, &mut out);
                } else if self.is_fullscreen(id) {
                    self.exit_fullscreen(true);
                }
            }
            WindowEvent::MaximizeRequested { .. } | WindowEvent::FullscreenRequested { .. } => {}
            WindowEvent::Unmapped { id } => {
                self.windows.unmap(id);
                self.settling.retain(|w| *w != id);
                self.initial.retain(|i| i.window != id);
                if let Some(fs) = self.fullscreen.take_if(|f| f.window == id) {
                    // The view returns to where it was before fullscreen.
                    self.viewer.set_camera(fs.saved_camera);
                }
                if self.router.grab.as_ref().is_some_and(|g| g.window == id) {
                    self.router.grab = None;
                }
                if self
                    .router
                    .operation
                    .as_ref()
                    .is_some_and(|o| o.op.window() == id)
                {
                    self.router.operation = None;
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
        self.focus_changed(before, &mut out);
        out
    }

    /// A window that asked for an initial state made its initial commit: answer with its
    /// initial configure (FR-019, FR-020, research W13).
    fn created(
        &mut self,
        id: WindowId,
        parent: Option<WindowId>,
        limits: SizeLimits,
        maximized: bool,
        fullscreen: bool,
    ) {
        let unfocused = self.opens_unfocused(parent);
        if unfocused || !(maximized || fullscreen) {
            self.commands.push(WindowCommand::Configure {
                window: id,
                size: None,
                states: States::default(),
            });
            return;
        }
        let init = Initial {
            window: id,
            limits,
            maximized,
            fullscreen,
        };
        self.initial.push(init);
        self.commands.push(WindowCommand::Configure {
            window: id,
            size: Some(limits.clamp(self.viewer.size(), self.largest)),
            states: States {
                maximized,
                fullscreen,
                resizing: false,
            },
        });
    }

    /// Whether a new window with `parent` opens without focus: while another window is
    /// fullscreen, unless it is that window's dialog (FR-020).
    fn opens_unfocused(&self, parent: Option<WindowId>) -> bool {
        self.fullscreen
            .is_some_and(|f| parent.is_none_or(|p| p != f.window))
    }

    /// A window showed its first content.
    fn mapped(
        &mut self,
        id: WindowId,
        size: Size,
        input: Vec<Rect>,
        limits: SizeLimits,
        parent: Option<WindowId>,
        out: &mut Vec<ClientInput>,
    ) {
        let initial = self
            .initial
            .iter()
            .position(|i| i.window == id)
            .map(|i| self.initial.remove(i));
        if self.opens_unfocused(parent)
            && let Some(fs) = self.fullscreen
        {
            // Behind the fullscreen window, centred in the view it returns to (FR-020).
            self.windows
                .insert_unfocused(id, size, input, fs.saved_camera.center);
            self.windows.set_limits(id, limits);
            self.windows.set_parent(id, parent);
            if initial.is_some() {
                // The state granted at `Created` no longer applies.
                self.commands.push(WindowCommand::Configure {
                    window: id,
                    size: None,
                    states: States::default(),
                });
            }
            return;
        }
        if self.fullscreen.is_some() {
            // A dialog of the fullscreen window: fullscreen ends first.
            self.exit_fullscreen(true);
        }
        let center = self.viewer.camera().center;
        self.windows.map(id, size, input, center);
        self.windows.set_limits(id, limits);
        self.windows.set_parent(id, parent);
        if let Some(init) = initial {
            // The initial configure already asked for the state.
            if init.maximized {
                self.maximize(id, false, false, out);
            }
            if init.fullscreen {
                self.enter_fullscreen(id, false, out);
            }
        }
    }

    /// Maximizes `id` (FR-013, research W7): the view jumps to the window at 100% and the
    /// window, anchored at its centre, is asked for the viewer size. `save`: remember its rect
    /// to restore (not for a window maximized before it was shown); `send`: send the configure.
    fn maximize(&mut self, id: WindowId, save: bool, send: bool, out: &mut Vec<ClientInput>) {
        let Some(w) = self.windows.get(id) else {
            return;
        };
        if matches!(w.mode, Mode::Maximized { .. }) {
            return;
        }
        let rect = w.rect();
        if let Some(fs) = self.fullscreen.filter(|f| f.window == id) {
            // Maximized under fullscreen: shown when fullscreen ends.
            self.windows.set_mode(
                id,
                Mode::Maximized {
                    saved: Some(fs.saved_rect),
                },
            );
            if send {
                self.configure_viewer_size(id);
            }
            return;
        }
        let center = rect.origin + rect.size.to_vector() / 2.0;
        self.windows.set_mode(
            id,
            Mode::Maximized {
                saved: save.then_some(rect),
            },
        );
        self.windows
            .set_anchor(id, Anchor::Centre { point: center });
        self.settling.retain(|w| *w != id);
        self.viewer.set_camera(Camera { center, zoom: 1.0 });
        self.focus_and_raise(id, out);
        if send {
            self.configure_viewer_size(id);
        }
        self.dirty = true;
    }

    /// Leaves maximize: back to the saved place and size, the view stays (FR-013).
    fn unmaximize(&mut self, id: WindowId) {
        let Some(Mode::Maximized { saved }) = self.windows.get(id).map(|w| w.mode) else {
            return;
        };
        self.windows.set_mode(id, Mode::Normal);
        if let Some(fs) = &mut self.fullscreen
            && fs.window == id
        {
            // Under fullscreen: leaving fullscreen will return to the normal rect.
            if let Some(rect) = saved {
                fs.saved_rect = rect;
            }
            self.configure_viewer_size(id);
            return;
        }
        self.restore(id, saved);
    }

    /// Puts `id` back at `saved` (or lets the application choose its size, keeping the
    /// centre, if there is none); the anchor returns to the top-left once it settles.
    fn restore(&mut self, id: WindowId, saved: Option<Rect>) {
        let states = self.states_of(id, false);
        match saved {
            Some(rect) => {
                self.windows.move_to(id, rect.origin);
                self.windows.set_anchor(id, Anchor::TopLeft);
                self.commands.push(WindowCommand::Configure {
                    window: id,
                    size: Some(rect.size),
                    states,
                });
            }
            None => {
                if let Some(r) = self.windows.get(id).map(Window::rect) {
                    let point = r.origin + r.size.to_vector() / 2.0;
                    self.windows.set_anchor(id, Anchor::Centre { point });
                }
                self.commands.push(WindowCommand::Configure {
                    window: id,
                    size: None,
                    states,
                });
            }
        }
        self.settling.retain(|w| *w != id);
        self.settling.push(id);
        self.dirty = true;
    }

    /// Fullscreen for `id` (FR-014, research W8): only it is shown, at 100%, centred.
    fn enter_fullscreen(&mut self, id: WindowId, send: bool, out: &mut Vec<ClientInput>) {
        if self.is_fullscreen(id) {
            return;
        }
        // One fullscreen window: the earlier one leaves first, so the saved view is always
        // the one from before any fullscreen.
        self.exit_fullscreen(true);
        let Some(rect) = self.windows.get(id).map(Window::rect) else {
            return;
        };
        self.fullscreen = Some(Fullscreen {
            window: id,
            saved_rect: rect,
            saved_camera: self.viewer.camera(),
        });
        let center = rect.origin + rect.size.to_vector() / 2.0;
        self.windows
            .set_anchor(id, Anchor::Centre { point: center });
        self.settling.retain(|w| *w != id);
        self.viewer.set_camera(Camera { center, zoom: 1.0 });
        self.focus_and_raise(id, out);
        if send {
            self.configure_viewer_size(id);
        }
        if let Some(p) = self.pointer_canvas() {
            self.retarget(p, false, out);
        }
        self.dirty = true;
    }

    /// Ends fullscreen: the window returns to its saved rect (or to maximized), and with
    /// `restore_camera` the view returns too.
    fn exit_fullscreen(&mut self, restore_camera: bool) {
        let Some(fs) = self.fullscreen.take() else {
            return;
        };
        if restore_camera {
            self.viewer.set_camera(fs.saved_camera);
        }
        self.dirty = true;
        let Some(mode) = self.windows.get(fs.window).map(|w| w.mode) else {
            return;
        };
        let rect = fs.saved_rect;
        match mode {
            Mode::Maximized { .. } => {
                let point = rect.origin + rect.size.to_vector() / 2.0;
                self.windows.move_to(fs.window, rect.origin);
                self.windows.set_anchor(fs.window, Anchor::Centre { point });
                self.configure_viewer_size(fs.window);
            }
            Mode::Normal => self.restore(fs.window, Some(rect)),
        }
    }

    /// A move of a maximized window restores its size first, keeping the grabbed point at the
    /// same relative x and the same y offset (spec Edge Cases); returns the new offset.
    fn unmaximize_for_move(&mut self, id: WindowId, offset: Vector) -> Vector {
        let Some(w) = self.windows.get(id) else {
            return offset;
        };
        let Mode::Maximized { saved } = w.mode else {
            return offset;
        };
        let width = w.size.w;
        self.windows.set_mode(id, Mode::Normal);
        let offset = match saved {
            Some(r) if width > 0.0 => Vector::new(offset.x * r.size.w / width, offset.y),
            _ => offset,
        };
        self.restore(id, saved);
        if let Some(p) = self.pointer_canvas() {
            self.windows.move_to(id, p - offset);
        }
        offset
    }

    /// An application asked to move its window: accepted only while a button pressed on that
    /// window is held (FR-001, FR-016, research W2); the grab becomes the move.
    fn move_requested(&mut self, id: WindowId, out: &mut Vec<ClientInput>) {
        let Some(grab) = self
            .router
            .grab
            .take_if(|g| g.window == id && !g.buttons.is_empty())
        else {
            return;
        };
        let (Some(p), Some(window)) = (self.pointer_canvas(), self.windows.get(id)) else {
            self.router.grab = Some(grab);
            return;
        };
        let offset = self.restore_for_move(id, p - window.origin);
        self.start_operation(Op::Move { window: id, offset }, grab.buttons, out);
    }

    /// An application asked to resize its window from `edges`: accepted like a move request
    /// (FR-005, FR-016); `Edges::NONE` means nothing.
    fn resize_requested(&mut self, id: WindowId, edges: Edges, out: &mut Vec<ClientInput>) {
        if !edges.is_valid() {
            return;
        }
        let Some(grab) = self
            .router
            .grab
            .take_if(|g| g.window == id && !g.buttons.is_empty())
        else {
            return;
        };
        let Some(op) = self.resize_op(id, edges) else {
            self.router.grab = Some(grab);
            return;
        };
        self.start_operation(op, grab.buttons, out);
    }

    /// A resize of `window` from `edges` starting at the pointer; anchors the opposite edges.
    /// A maximized window first returns to its saved rect, and the resize continues from it
    /// (FR-013).
    fn resize_op(&mut self, window: WindowId, edges: Edges) -> Option<Op> {
        let mut start_rect = self.windows.get(window)?.rect();
        let mut restored = false;
        if let Some(fs) = self.fullscreen.filter(|f| f.window == window) {
            // An application's resize request on its fullscreen window leaves fullscreen and
            // continues from the saved rect (FR-014).
            self.exit_fullscreen(true);
            start_rect = fs.saved_rect;
            restored = true;
        }
        let start_pointer = self.pointer_canvas()?;
        let w = self.windows.get(window)?;
        if let Mode::Maximized { saved } = w.mode {
            self.windows.set_mode(window, Mode::Normal);
            self.restore(window, saved);
            if let Some(rect) = saved {
                start_rect = rect;
            }
            restored = true;
        }
        self.settling.retain(|w| *w != window);
        self.windows.set_anchor(
            window,
            Anchor::Edges {
                fixed: Edges {
                    top: edges.bottom,
                    bottom: edges.top,
                    left: edges.right,
                    right: edges.left,
                },
                right: start_rect.end().x,
                bottom: start_rect.end().y,
            },
        );
        Some(Op::Resize {
            window,
            edges,
            start_pointer,
            start_rect,
            last: None,
            restored,
        })
    }

    /// The compositor resize gesture on `window`: edges from the 3 x 3 split (FR-006).
    fn start_resize(&mut self, window: WindowId, code: u32, out: &mut Vec<ClientInput>) {
        let (Some(p), Some(w)) = (self.pointer_canvas(), self.windows.get(window)) else {
            return;
        };
        let edges = pick_edges(w.rect(), p);
        self.focus_and_raise(window, out);
        if let Some(op) = self.resize_op(window, edges) {
            self.start_operation(op, vec![code], out);
        }
    }

    /// One focus-cycling step: focus and raise the next window in the history and bring it
    /// into view (FR-011, FR-012).
    fn cycle(&mut self, step: i32, out: &mut Vec<ClientInput>) {
        let before = self.windows.focus();
        let Some(id) = self.windows.cycle(step) else {
            return;
        };
        self.dirty = true;
        self.focus_changed(before, out);
        let Some(rect) = self.windows.get(id).map(Window::rect) else {
            return;
        };
        if let Some(mut camera) = reveal(self.viewer.camera(), self.viewer.size(), rect) {
            if matches!(
                self.windows.get(id).map(|w| w.mode),
                Some(Mode::Maximized { .. })
            ) {
                // A maximized window gets its maximize view (FR-012, FR-013).
                camera = Camera {
                    center: rect.origin + rect.size.to_vector() / 2.0,
                    zoom: 1.0,
                };
            }
            self.viewer.set_camera(camera);
            // A different window may be under the pointer now.
            if let Some(p) = self.pointer_canvas() {
                self.retarget(p, false, out);
            }
        }
    }

    /// The states to ask `window` for, besides `activated`.
    fn states_of(&self, window: WindowId, resizing: bool) -> States {
        States {
            maximized: matches!(
                self.windows.get(window).map(|w| w.mode),
                Some(Mode::Maximized { .. })
            ),
            fullscreen: self.is_fullscreen(window),
            resizing,
        }
    }

    /// The window accepting input at canvas point `p`; while a window is fullscreen, only it.
    fn hit(&self, p: Point) -> Option<(WindowId, Point)> {
        match self.fullscreen {
            Some(fs) => self
                .windows
                .hit_window(fs.window, p)
                .map(|local| (fs.window, local)),
            None => self.windows.hit(p),
        }
    }

    /// `size` within `window`'s limits and the largest displayable window (FR-008).
    fn clamp_size(&self, window: WindowId, size: Size) -> Size {
        let limits = self
            .windows
            .get(window)
            .map(|w| w.limits)
            .unwrap_or_default();
        limits.clamp(size, self.largest)
    }

    /// Starts an operation: the pointer leaves the window, which gets no pointer input until
    /// the operation ends (research W3).
    fn start_operation(&mut self, op: Op, buttons: Vec<u32>, out: &mut Vec<ClientInput>) {
        self.router.grab = None;
        self.leave(out);
        self.router.operation = Some(Operation { op, buttons });
    }

    /// The compositor move gesture on `window`: focus, raise, then move (FR-002, FR-004).
    fn start_move(&mut self, window: WindowId, code: u32, out: &mut Vec<ClientInput>) {
        let (Some(p), Some(w)) = (self.pointer_canvas(), self.windows.get(window)) else {
            return;
        };
        let offset = self.restore_for_move(window, p - w.origin);
        self.focus_and_raise(window, out);
        self.start_operation(Op::Move { window, offset }, vec![code], out);
    }

    /// A move of a fullscreen or maximized window first returns it to its earlier size, the
    /// grabbed point at the same relative x and the same y offset (spec Edge Cases, FR-014);
    /// returns the offset for the move.
    fn restore_for_move(&mut self, window: WindowId, mut offset: Vector) -> Vector {
        if let Some(fs) = self.fullscreen.filter(|f| f.window == window) {
            // The window still has its fullscreen size until the application answers, so
            // scale by the saved rect.
            let width = self.windows.get(window).map_or(0.0, |w| w.size.w);
            self.exit_fullscreen(true);
            if width > 0.0 {
                offset = Vector::new(offset.x * fs.saved_rect.size.w / width, offset.y);
            }
            if let Some(p) = self.pointer_canvas() {
                self.windows.move_to(window, p - offset);
            }
        }
        self.unmaximize_for_move(window, offset)
    }

    fn focus_and_raise(&mut self, window: WindowId, out: &mut Vec<ClientInput>) {
        let before = self.windows.focus();
        self.windows.raise_and_focus(window);
        self.dirty = true;
        self.focus_changed(before, out);
    }

    /// Applies the pointer position to the running operation.
    fn update_operation(&mut self) {
        let Some(operation) = &self.router.operation else {
            return;
        };
        let Some(p) = self.pointer_canvas() else {
            return;
        };
        match operation.op {
            Op::Move { window, offset } => {
                self.windows.move_to(window, p - offset);
                self.dirty = true;
            }
            Op::Resize {
                window,
                edges,
                start_pointer,
                start_rect,
                last,
                ..
            } => {
                let wanted = resize_size(start_rect, edges, p - start_pointer);
                let size = self.clamp_size(window, wanted);
                if last != Some(size) {
                    self.commands.push(WindowCommand::Configure {
                        window,
                        size: Some(size),
                        states: self.states_of(window, true),
                    });
                    if let Some(Operation {
                        op: Op::Resize { last, .. },
                        ..
                    }) = &mut self.router.operation
                    {
                        *last = Some(size);
                    }
                }
            }
        }
    }

    /// Ends the running operation; the pointer re-enters the window under it.
    fn end_operation(&mut self, out: &mut Vec<ClientInput>) {
        if self.finish_operation()
            && let Some(p) = self.pointer_canvas()
        {
            self.retarget(p, false, out);
        }
    }

    /// Ends the running operation, if any (also on a host pointer leave). A resize asks for
    /// its last size without `resizing` and waits for it to settle (research W4).
    fn finish_operation(&mut self) -> bool {
        let Some(operation) = self.router.operation.take() else {
            return false;
        };
        self.dirty = true;
        if let Op::Resize {
            window,
            last,
            restored,
            ..
        } = operation.op
        {
            match last {
                Some(size) => {
                    self.commands.push(WindowCommand::Configure {
                        window,
                        size: Some(size),
                        states: self.states_of(window, false),
                    });
                    self.settling.push(window);
                }
                // Restoring a maximized or fullscreen window sent a size: settle that one.
                None if restored => self.settling.push(window),
                None => self.windows.resize_settled(window),
            }
        }
        true
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

    /// Windows that at least partly intersect the view, back to front (FR-006); while a
    /// window is fullscreen, only it (FR-014).
    pub fn visible(&self) -> Vec<WindowId> {
        match self.fullscreen {
            Some(fs) => vec![fs.window],
            None => self.windows.visible(self.view_rect()),
        }
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
                // grab: it ends, and so does an operation, as on the last release.
                self.router.grab = None;
                self.finish_operation();
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
            InputEvent::PinchBegin if self.fullscreen.is_some() => {
                // Leaves fullscreen and is consumed (contracts/input.md).
                self.exit_fullscreen(true);
                Effect::default()
            }
            InputEvent::PinchBegin
            | InputEvent::PinchUpdate { .. }
            | InputEvent::PinchEnd { .. } => {
                let effect = self.viewer.handle(event);
                self.update_operation();
                effect
            }
            InputEvent::Key {
                keycode,
                binding_sym,
                pressed,
            } => self.key(event, keycode, binding_sym, pressed, &mut out),
            InputEvent::Modifiers { mods, raw } => {
                self.router.mods = mods;
                if !bindings::mod_held(mods) {
                    // Releasing Mod commits the focus order (FR-011).
                    self.windows.end_cycle();
                }
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
                // The Mod release may never be seen: commit the focus order now.
                self.windows.end_cycle();
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
        if self.router.operation.is_some() {
            // The window being moved or resized gets no pointer input (research W3).
            self.update_operation();
            return;
        }
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
        let hit = self.hit(p);
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
        if let Some(operation) = &mut self.router.operation {
            // Further presses are swallowed; the last release ends the operation.
            if pressed {
                operation.buttons.push(code);
            } else {
                operation.buttons.retain(|b| *b != code);
                if operation.buttons.is_empty() {
                    self.end_operation(out);
                }
            }
            return Effect::default();
        }
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
            let hit = self.pointer_canvas().and_then(|p| self.hit(p));
            let mod_held = bindings::mod_held(self.router.mods);
            if button == Button::Left
                && mod_held
                && self.router.mods.shift
                && let Some((window, _)) = hit
            {
                // Mod + Shift + left on a window moves it (FR-002); on a fullscreen window
                // it leaves fullscreen first.
                self.start_move(window, code, out);
                return Effect::default();
            }
            if self.fullscreen.is_some()
                && ((button == Button::Right && mod_held && hit.is_some())
                    || (button == Button::Left && (mod_held || hit.is_none())))
            {
                // A resize gesture, a pan press or a press on the uncovered background leaves
                // fullscreen and is consumed (contracts/input.md).
                self.exit_fullscreen(true);
                return Effect::default();
            }
            if button == Button::Right
                && mod_held
                && let Some((window, _)) = hit
            {
                // Mod + right (Shift ignored) on a window resizes it (FR-006).
                self.start_resize(window, code, out);
                return Effect::default();
            }
            if button == Button::Left && bindings::mod_held(self.router.mods) {
                // Mod + left pans anywhere and does not focus (FR-012).
                self.leave(out);
                return self.viewer.handle(event);
            }
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
        if self.router.operation.is_some() {
            // Mod + scroll zooms around the pointer; plain scrolling does nothing.
            if !bindings::mod_held(self.router.mods) {
                return Effect::default();
            }
            let effect = self.viewer.handle(event);
            self.update_operation();
            return effect;
        }
        let target = match &self.router.grab {
            Some(grab) => Some(grab.window),
            None => self
                .pointer_canvas()
                .and_then(|p| self.hit(p))
                .map(|(id, _)| id),
        };
        if self.fullscreen.is_some() && (bindings::mod_held(self.router.mods) || target.is_none()) {
            // Zooming or scrolling the canvas leaves fullscreen and is consumed.
            self.exit_fullscreen(true);
            return Effect::default();
        }
        if self.viewer.is_panning() || bindings::mod_held(self.router.mods) {
            return self.viewer.handle(event);
        }
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
            if let Some(action) = bindings::lookup(self.router.mods, sym, true) {
                self.router.consumed_keys.push(keycode);
                let operating = self.router.operation.is_some();
                return match action {
                    // Ignored during a move or resize (spec Edge Cases).
                    Action::NextWindow | Action::PreviousWindow | Action::ResetView
                        if operating =>
                    {
                        Effect::default()
                    }
                    Action::ResetView if self.fullscreen.is_some() => {
                        // Leaves fullscreen instead (contracts/input.md).
                        self.exit_fullscreen(true);
                        Effect::default()
                    }
                    Action::NextWindow | Action::PreviousWindow => {
                        // Out of fullscreen first; the view then goes straight to the next
                        // window, computed against the view fullscreen returned to.
                        self.exit_fullscreen(true);
                        self.cycle(if action == Action::NextWindow { 1 } else { -1 }, out);
                        Effect::default()
                    }
                    Action::CloseWindow => {
                        // The application decides; RubickWay never forces it (FR-010).
                        if let Some(window) = self.focused_window() {
                            self.commands.push(WindowCommand::Close { window });
                        }
                        Effect::default()
                    }
                    Action::ResetView | Action::Quit => self.viewer.handle(event),
                };
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
