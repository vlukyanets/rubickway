//! The shell: windows on the canvas, focus and input routing. Pure core (Principle IV): no
//! Wayland, GL or device types.
//!
//! The server reports windows and applications' requests as [`WindowEvent`]s, which the
//! application applies here; the shell never calls the server (client-windows research R7) and
//! answers with [`WindowCommand`]s instead (window-management research W1). Input is routed per
//! contracts/input.md: Mod + scroll, pinch and the background go to the viewer (canvas
//! navigation), the pointer bindings pan, move and resize (by default Mod + left moves and
//! Mod + right resizes a window, Mod+Shift + left pans), Mod+Tab cycles focus, everything else
//! goes to the window under the pointer or with keyboard focus, as [`ClientInput`]s. Maximize and fullscreen are decided here too (research W7, W8).

mod geometry;
mod route;
mod windows;

pub use geometry::*;
pub use route::ClientInput;
pub use windows::*;

use route::{Grab, Op, Operation, Router};

use crate::canvas::{Camera, Layout, LayoutEntry, Point, Rect, Size, Vector, ViewerId};
use crate::input::bindings::{Action, Bindings, PointerAction};
use crate::input::{Button, InputEvent, Keysym, ScrollSource};
use crate::viewer::Viewer;

/// What the application must do after an input event. Redraws are signalled through
/// [`Shell::take_dirty_viewers`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Effect {
    /// The user asked to quit.
    pub quit: bool,
    /// The user pressed spawn binding `n`: run its command (FR-014).
    pub spawn: Option<usize>,
}

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
    /// The application asked to go fullscreen (`on`) or leave it; `output`: the viewer of the
    /// output it named, if that output still is a display (FR-022).
    FullscreenRequested {
        id: WindowId,
        on: bool,
        output: Option<ViewerId>,
    },
    /// The application hid the pointer over its windows (`wl_pointer.set_cursor` without a
    /// surface) or showed it again (FR-008).
    CursorHidden { id: WindowId, hidden: bool },
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
    /// No pointer: the application under it hid it, and no gesture runs (FR-008).
    Hidden,
}

/// One display's viewer (research S14); its home view is the layout's.
#[derive(Debug, Clone)]
struct ViewerSlot {
    id: ViewerId,
    viewer: Viewer,
    /// Its fullscreen window, if any (research W8; one per viewer, FR-022).
    fullscreen: Option<Fullscreen>,
}

/// The viewers, the windows they show, and input routing.
#[derive(Debug, Clone)]
pub struct Shell {
    /// The shortcuts and Mod (research S15); every viewer has a copy.
    bindings: Bindings,
    /// One per display, in the order they were added.
    viewers: Vec<ViewerSlot>,
    /// The viewer used while no display exists; it shows nothing, and maximize and fullscreen
    /// are not granted (spec Edge Cases "No display at all").
    spare: Viewer,
    /// The row of displays and the pointer position (research S9).
    layout: Layout,
    /// The viewer under the pointer (FR-019).
    active: Option<ViewerId>,
    /// The viewer a pan started on, until the pan ends (FR-019).
    pan_viewer: Option<ViewerId>,
    /// Counts connections, for the row order of external displays (FR-018).
    next_seq: u64,
    windows: Windows,
    router: Router,
    commands: Vec<WindowCommand>,
    /// Windows whose resize ended; their anchor returns to the top-left once they apply the
    /// final configure (research W4).
    settling: Vec<WindowId>,
    /// The largest window size that can be displayed (research W6).
    largest: Size,
    /// The viewer each maximized window was maximized for (FR-022).
    maximized_on: Vec<(WindowId, ViewerId)>,
    /// Initial states answered at `Created`, entered at `Mapped` (research W13).
    initial: Vec<Initial>,
    /// Windows whose application hid the pointer (FR-008).
    cursor_hidden: Vec<WindowId>,
    dirty: bool,
}

/// The canvas rectangle a viewer shows.
fn view_rect_of(viewer: &Viewer) -> Rect {
    let size = viewer.size();
    let camera = viewer.camera();
    let top_left = camera.to_canvas(Point::new(0.0, 0.0), size);
    Rect::new(
        top_left,
        Size::new(size.w / camera.zoom, size.h / camera.zoom),
    )
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
    /// A shell with the default shortcuts (Mod = Super, no spawns); see
    /// [`Shell::with_bindings`].
    pub fn new() -> Shell {
        Shell::with_bindings(Bindings::default())
    }

    /// A shell whose shortcuts and Mod come from `bindings` (FR-005, FR-014).
    pub fn with_bindings(bindings: Bindings) -> Shell {
        let mut spare = Viewer::new();
        spare.set_bindings(bindings.clone());
        Shell {
            bindings,
            viewers: Vec::new(),
            spare,
            layout: Layout::default(),
            active: None,
            pan_viewer: None,
            next_seq: 0,
            windows: Windows::default(),
            router: Router::default(),
            commands: Vec::new(),
            settling: Vec::new(),
            largest: Size {
                w: f64::INFINITY,
                h: f64::INFINITY,
            },
            maximized_on: Vec::new(),
            initial: Vec::new(),
            cursor_hidden: Vec::new(),
            dirty: true,
        }
    }

    /// The active viewer's fullscreen window, if any.
    pub fn fullscreen(&self) -> Option<WindowId> {
        self.active_fs().map(|f| f.window)
    }

    /// Viewer `id`'s fullscreen window, if any (FR-022).
    pub fn fullscreen_in(&self, id: ViewerId) -> Option<WindowId> {
        self.slot(id).and_then(|s| s.fullscreen).map(|f| f.window)
    }

    fn slot(&self, id: ViewerId) -> Option<&ViewerSlot> {
        self.viewers.iter().find(|s| s.id == id)
    }

    fn slot_mut(&mut self, id: ViewerId) -> Option<&mut ViewerSlot> {
        self.viewers.iter_mut().find(|s| s.id == id)
    }

    fn active_fs(&self) -> Option<Fullscreen> {
        self.active
            .and_then(|id| self.slot(id))
            .and_then(|s| s.fullscreen)
    }

    /// The viewer where `window` is fullscreen, and that fullscreen state.
    fn fs_of(&self, window: WindowId) -> Option<(ViewerId, Fullscreen)> {
        self.viewers.iter().find_map(|s| {
            s.fullscreen
                .filter(|f| f.window == window)
                .map(|f| (s.id, f))
        })
    }

    fn set_camera_of(&mut self, id: ViewerId, camera: Camera) {
        if let Some(slot) = self.slot_mut(id) {
            slot.viewer.set_camera(camera);
        }
    }

    /// The viewer maximize and fullscreen use for `window` (FR-022): the active viewer if it
    /// shows part of the window, else the one showing the largest part, else the active one;
    /// `None` without displays.
    fn window_viewer(&self, window: WindowId) -> Option<ViewerId> {
        let rect = self.windows.get(window)?.rect();
        let active = self.active?;
        let area = |id: ViewerId| {
            self.slot(id)
                .and_then(|s| view_rect_of(&s.viewer).intersection(&rect))
                .map_or(0.0, |r| r.size.w * r.size.h)
        };
        if area(active) > 0.0 {
            return Some(active);
        }
        let best = self
            .viewers
            .iter()
            .map(|s| (s.id, area(s.id)))
            .filter(|(_, a)| *a > 0.0)
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id);
        Some(best.unwrap_or(active))
    }

    /// The viewer whose size a maximized or fullscreen `window` gets.
    fn sizing_viewer(&self, window: WindowId) -> Option<ViewerId> {
        self.fs_of(window)
            .map(|(id, _)| id)
            .or_else(|| {
                self.maximized_on
                    .iter()
                    .find(|(w, v)| *w == window && self.slot(*v).is_some())
                    .map(|(_, v)| *v)
            })
            .or_else(|| self.window_viewer(window))
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
            None if self.av().is_panning() => CursorShape::Grabbing,
            None if self
                .router
                .entered
                .is_some_and(|w| self.cursor_hidden.contains(&w)) =>
            {
                CursorShape::Hidden
            }
            None => CursorShape::Default,
        }
    }

    /// Viewer `id` got a new size: its maximized and fullscreen windows follow it, and so do
    /// initial states granted for the active viewer.
    fn viewer_resized(&mut self, id: ViewerId) {
        let follow: Vec<WindowId> = self
            .windows
            .iter()
            .filter(|w| {
                (matches!(w.mode, Mode::Maximized { .. }) || self.is_fullscreen(w.id))
                    && self.sizing_viewer(w.id) == Some(id)
            })
            .map(|w| w.id)
            .collect();
        for window in follow {
            self.configure_viewer_size(window);
        }
        if self.active != Some(id) {
            return;
        }
        for init in self.initial.clone() {
            self.commands.push(WindowCommand::Configure {
                window: init.window,
                size: Some(init.limits.clamp(self.av().size(), self.largest)),
                states: States {
                    maximized: init.maximized,
                    fullscreen: init.fullscreen,
                    resizing: false,
                },
            });
        }
    }

    fn is_fullscreen(&self, id: WindowId) -> bool {
        self.fs_of(id).is_some()
    }

    /// Asks a maximized or fullscreen window for its viewer's size.
    fn configure_viewer_size(&mut self, id: WindowId) {
        let viewer_size = self
            .sizing_viewer(id)
            .and_then(|v| self.slot(v))
            .map_or_else(|| self.av().size(), |s| s.viewer.size());
        let size = self.clamp_size(id, viewer_size);
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

    /// The active viewer (the one under the pointer), or the spare viewer while there is no
    /// display-backed one.
    pub fn viewer(&self) -> &Viewer {
        self.av()
    }

    /// The active viewer, mutable (see [`Shell::viewer`]).
    pub fn viewer_mut(&mut self) -> &mut Viewer {
        self.av_mut()
    }

    /// The viewer of display `id`.
    pub fn viewer_of(&self, id: ViewerId) -> Option<&Viewer> {
        self.viewers.iter().find(|s| s.id == id).map(|s| &s.viewer)
    }

    /// The display-backed viewers, in the order they were added.
    pub fn viewers(&self) -> impl Iterator<Item = (ViewerId, &Viewer)> {
        self.viewers.iter().map(|s| (s.id, &s.viewer))
    }

    /// The active viewer's id (FR-019).
    pub fn active(&self) -> Option<ViewerId> {
        self.active
    }

    /// The display the pointer is on and its position there (where to draw it).
    pub fn pointer(&self) -> Option<(ViewerId, Point)> {
        self.layout.locate(self.layout.pointer())
    }

    /// The layout of the displays (positions for `wl_output`).
    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    /// Adds the viewer of a display of logical `size`; `internal` for a built-in panel
    /// (FR-018). Its camera starts at its home view (FR-020). Adding an existing id resizes it.
    pub fn add_viewer(&mut self, id: ViewerId, size: Size, internal: bool) {
        let connected_seq = match self.layout.entries().iter().find(|e| e.id == id) {
            Some(e) => e.connected_seq,
            None => {
                self.next_seq += 1;
                self.next_seq
            }
        };
        let existing = self.viewers.iter().any(|s| s.id == id);
        self.layout.add(LayoutEntry {
            id,
            size,
            internal,
            connected_seq,
        });
        if !existing {
            let mut viewer = Viewer::new();
            viewer.set_bindings(self.bindings.clone());
            viewer.set_camera(self.layout.home(id).unwrap_or_default());
            self.viewers.push(ViewerSlot {
                id,
                viewer,
                fullscreen: None,
            });
        }
        let mut resized = false;
        if let Some(slot) = self.slot_mut(id) {
            let before = slot.viewer.size();
            slot.viewer.configure(size);
            resized = existing && slot.viewer.size() != before;
        }
        self.sync_active();
        if resized {
            self.viewer_resized(id);
        }
        self.dirty = true;
    }

    /// Removes the viewer of a display that went away (FR-021): its fullscreen window gets
    /// its previous place and size back, maximized windows stay as they are; with the pointer
    /// on it, a pan, move or resize ends as on a broken grab, and the pointer moves to the
    /// centre of the leftmost display. Returns input for applications (a pointer leave).
    pub fn remove_viewer(&mut self, id: ViewerId) -> Vec<ClientInput> {
        let mut out = Vec::new();
        if self.slot(id).is_none() {
            return out;
        }
        self.exit_fullscreen_on(id, false);
        if self.active == Some(id) {
            self.router.grab = None;
            self.finish_operation();
            self.leave(&mut out);
        }
        self.viewers.retain(|s| s.id != id);
        self.layout.remove(id);
        if self.pan_viewer == Some(id) {
            self.pan_viewer = None;
        }
        self.sync_active();
        self.dirty = true;
        out
    }

    /// Gives every viewer the home view of the whole row and moves its camera there, with the
    /// pointer in the centre of the leftmost display; called once the displays present at
    /// startup are known (FR-009, FR-020).
    pub fn place_home_views(&mut self) {
        self.layout.recentre_homes();
        for slot in &mut self.viewers {
            slot.viewer
                .set_camera(self.layout.home(slot.id).unwrap_or_default());
        }
        self.sync_active();
    }

    /// The active viewer follows the pointer's display, and knows where the pointer is on it
    /// (the pointer exists from the start, so a press before any motion still pans). Windows
    /// learn about the pointer with its next motion.
    fn sync_active(&mut self) {
        let pointer = self.pointer();
        self.active = pointer.map(|(id, _)| id);
        if let Some((id, pos)) = pointer
            && let Some(slot) = self.viewers.iter_mut().find(|s| s.id == id)
        {
            slot.viewer.handle(&InputEvent::PointerEnter { pos });
        }
    }

    fn av(&self) -> &Viewer {
        self.active
            .and_then(|id| self.viewers.iter().find(|s| s.id == id))
            .map_or(&self.spare, |s| &s.viewer)
    }

    fn av_mut(&mut self) -> &mut Viewer {
        match self
            .active
            .and_then(|id| self.viewers.iter().position(|s| s.id == id))
        {
            Some(i) => &mut self.viewers[i].viewer,
            None => &mut self.spare,
        }
    }

    /// The active viewer's home view (the canvas origin at 100% without displays).
    fn active_home(&self) -> Camera {
        self.active
            .and_then(|id| self.layout.home(id))
            .unwrap_or_default()
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
            // Only the focused window may change these states (FR-019), and only while there
            // is a display (spec Edge Cases "No display at all").
            WindowEvent::MaximizeRequested { id, on }
                if self.windows.focus() == Some(id) && !self.viewers.is_empty() =>
            {
                if on {
                    self.maximize(id, true, true, &mut out);
                } else {
                    self.unmaximize(id);
                }
            }
            WindowEvent::FullscreenRequested { id, on, output }
                if self.windows.focus() == Some(id) && !self.viewers.is_empty() =>
            {
                if on {
                    self.enter_fullscreen(id, output, true, &mut out);
                } else if let Some((viewer, _)) = self.fs_of(id) {
                    self.exit_fullscreen_on(viewer, true);
                }
            }
            WindowEvent::MaximizeRequested { .. } | WindowEvent::FullscreenRequested { .. } => {}
            WindowEvent::CursorHidden { id, hidden } => {
                let before = self.cursor_hidden.contains(&id);
                if hidden && !before {
                    self.cursor_hidden.push(id);
                } else if !hidden {
                    self.cursor_hidden.retain(|w| *w != id);
                }
            }
            WindowEvent::Unmapped { id } => {
                self.windows.unmap(id);
                self.cursor_hidden.retain(|w| *w != id);
                self.settling.retain(|w| *w != id);
                self.initial.retain(|i| i.window != id);
                self.maximized_on.retain(|(w, _)| *w != id);
                if let Some((viewer, fs)) = self.fs_of(id) {
                    // That viewer returns to where it was before fullscreen.
                    if let Some(slot) = self.slot_mut(viewer) {
                        slot.fullscreen = None;
                        slot.viewer.set_camera(fs.saved_camera);
                    }
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
        // Without a display the states are not granted (spec Edge Cases).
        if unfocused || !(maximized || fullscreen) || self.viewers.is_empty() {
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
            size: Some(limits.clamp(self.av().size(), self.largest)),
            states: States {
                maximized,
                fullscreen,
                resizing: false,
            },
        });
    }

    /// Whether a new window with `parent` opens without focus: while another window is
    /// fullscreen on the active viewer, unless it is that window's dialog (FR-020, FR-022).
    fn opens_unfocused(&self, parent: Option<WindowId>) -> bool {
        self.active_fs()
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
            && let Some(fs) = self.active_fs()
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
        if self.active_fs().is_some() {
            // A dialog of the fullscreen window: fullscreen ends first.
            self.exit_fullscreen(true);
        }
        // Centred in the active viewer; without a display, on the canvas origin (FR-022).
        let center = match self.active {
            Some(_) => self.av().camera().center,
            None => Point::new(0.0, 0.0),
        };
        self.windows.map(id, size, input, center);
        self.windows.set_limits(id, limits);
        self.windows.set_parent(id, parent);
        if let Some(init) = initial {
            // The initial configure already asked for the state.
            if init.maximized {
                self.maximize(id, false, false, out);
            }
            if init.fullscreen {
                self.enter_fullscreen(id, None, false, out);
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
        let Some(viewer) = self.window_viewer(id) else {
            return;
        };
        if let Some((fs_viewer, fs)) = self.fs_of(id) {
            // Maximized under fullscreen: shown when fullscreen ends.
            self.windows.set_mode(
                id,
                Mode::Maximized {
                    saved: Some(fs.saved_rect),
                },
            );
            self.maximized_on.retain(|(w, _)| *w != id);
            self.maximized_on.push((id, fs_viewer));
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
        self.maximized_on.retain(|(w, _)| *w != id);
        self.maximized_on.push((id, viewer));
        self.windows
            .set_anchor(id, Anchor::Centre { point: center });
        self.settling.retain(|w| *w != id);
        // Only that viewer's camera moves (FR-022).
        self.set_camera_of(viewer, Camera { center, zoom: 1.0 });
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
        self.maximized_on.retain(|(w, _)| *w != id);
        if let Some((viewer, _)) = self.fs_of(id) {
            // Under fullscreen: leaving fullscreen will return to the normal rect.
            if let Some(rect) = saved
                && let Some(fs) = self.slot_mut(viewer).and_then(|s| s.fullscreen.as_mut())
            {
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

    /// Fullscreen for `id` (FR-014, research W8) on the viewer of `output` if that is a display,
    /// else on the window's viewer (FR-022): only it is shown there, at 100%, centred.
    fn enter_fullscreen(
        &mut self,
        id: WindowId,
        output: Option<ViewerId>,
        send: bool,
        out: &mut Vec<ClientInput>,
    ) {
        let Some(viewer) = output
            .filter(|v| self.slot(*v).is_some())
            .or_else(|| self.window_viewer(id))
        else {
            return;
        };
        match self.fs_of(id) {
            Some((on, _)) if on == viewer => return,
            // Fullscreen elsewhere: it moves to this viewer.
            Some((on, _)) => self.exit_fullscreen_on(on, true),
            None => {}
        }
        // One fullscreen window per viewer: the earlier one leaves first, so the saved view is
        // always the one from before any fullscreen.
        self.exit_fullscreen_on(viewer, true);
        let Some(rect) = self.windows.get(id).map(Window::rect) else {
            return;
        };
        let center = rect.origin + rect.size.to_vector() / 2.0;
        if let Some(slot) = self.slot_mut(viewer) {
            slot.fullscreen = Some(Fullscreen {
                window: id,
                saved_rect: rect,
                saved_camera: slot.viewer.camera(),
            });
            slot.viewer.set_camera(Camera { center, zoom: 1.0 });
        }
        self.windows
            .set_anchor(id, Anchor::Centre { point: center });
        self.settling.retain(|w| *w != id);
        self.focus_and_raise(id, out);
        if send {
            self.configure_viewer_size(id);
        }
        if self.active == Some(viewer)
            && let Some(p) = self.pointer_canvas()
        {
            self.retarget(p, false, out);
        }
        self.dirty = true;
    }

    /// Ends the active viewer's fullscreen (see [`Shell::exit_fullscreen_on`]).
    fn exit_fullscreen(&mut self, restore_camera: bool) {
        if let Some(id) = self.active {
            self.exit_fullscreen_on(id, restore_camera);
        }
    }

    /// Ends viewer `id`'s fullscreen: the window returns to its saved rect (or to maximized),
    /// and with `restore_camera` that viewer's view returns too.
    fn exit_fullscreen_on(&mut self, id: ViewerId, restore_camera: bool) {
        let Some(slot) = self.slot_mut(id) else {
            return;
        };
        let Some(fs) = slot.fullscreen.take() else {
            return;
        };
        if restore_camera {
            slot.viewer.set_camera(fs.saved_camera);
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
        if let Some((viewer, fs)) = self.fs_of(window) {
            // An application's resize request on its fullscreen window leaves fullscreen and
            // continues from the saved rect (FR-014).
            self.exit_fullscreen_on(viewer, true);
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
    /// into the active viewer, unless it is already fully inside some viewer (FR-011, FR-012,
    /// FR-022).
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
        let shown = self.is_fullscreen(id)
            || self.viewers.iter().any(|s| {
                Some(s.id) != self.active
                    && reveal(s.viewer.camera(), s.viewer.size(), rect).is_none()
            });
        if self.active.is_none() || shown {
            return;
        }
        if let Some(mut camera) = reveal(self.av().camera(), self.av().size(), rect) {
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
            self.av_mut().set_camera(camera);
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

    /// The window accepting input at canvas point `p`; while a window is fullscreen on the
    /// active viewer, only it.
    fn hit(&self, p: Point) -> Option<(WindowId, Point)> {
        match self.active_fs() {
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
        if let Some((viewer, fs)) = self.fs_of(window) {
            // The window still has its fullscreen size until the application answers, so
            // scale by the saved rect.
            let width = self.windows.get(window).map_or(0.0, |w| w.size.w);
            self.exit_fullscreen_on(viewer, true);
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

    /// Ends the running operation, if any (also on a pointer leave). A resize asks for
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
        view_rect_of(self.av())
    }

    /// Windows shown on display `id`, back to front; while a window is fullscreen there, only
    /// it.
    pub fn visible_in(&self, id: ViewerId) -> Vec<WindowId> {
        match self.slot(id) {
            Some(ViewerSlot {
                fullscreen: Some(fs),
                ..
            }) => vec![fs.window],
            Some(slot) => self.windows.visible(view_rect_of(&slot.viewer)),
            None => Vec::new(),
        }
    }

    /// Windows shown on any display, back to front (they get frame callbacks).
    pub fn visible_any(&self) -> Vec<WindowId> {
        let shown: Vec<Vec<WindowId>> =
            self.viewers.iter().map(|s| self.visible_in(s.id)).collect();
        self.windows
            .iter()
            .map(|w| w.id)
            .filter(|id| shown.iter().any(|v| v.contains(id)))
            .collect()
    }

    /// The displays that need a new frame (their view or the windows changed); clears the
    /// flags.
    pub fn take_dirty_viewers(&mut self) -> Vec<ViewerId> {
        let all = std::mem::take(&mut self.dirty);
        self.viewers
            .iter_mut()
            .filter_map(|s| (s.viewer.take_dirty() || all).then_some(s.id))
            .collect()
    }

    /// Windows that at least partly intersect the view, back to front (FR-006); while a
    /// window is fullscreen, only it (FR-014).
    pub fn visible(&self) -> Vec<WindowId> {
        match self.active_fs() {
            Some(fs) => vec![fs.window],
            None => self.windows.visible(self.view_rect()),
        }
    }

    /// Whether a new frame is needed (the view or the windows changed); clears the flag.
    pub fn take_dirty(&mut self) -> bool {
        let viewer = self.av_mut().take_dirty();
        std::mem::take(&mut self.dirty) || viewer
    }

    /// Handles one input event: canvas navigation or input for an application.
    pub fn handle(&mut self, event: &InputEvent) -> (Effect, Vec<ClientInput>) {
        let mut out = Vec::new();
        let panned = self.pan_viewer.is_some();
        let effect = match *event {
            InputEvent::PointerEnter { pos } | InputEvent::PointerMotion { pos } => {
                // An absolute position on the active viewer: keep the layout's pointer in step.
                if let Some(origin) = self.active.and_then(|id| self.layout.origin(id)) {
                    self.layout
                        .move_to(Point::new(origin.x + pos.x, origin.y + pos.y));
                }
                self.pointer_moved(event, pos, &mut out);
                Effect::default()
            }
            InputEvent::PointerLeave => {
                // A leave breaks the grab (the console was switched away, research S23): it
                // ends, and so does an operation, as on the last release.
                self.router.grab = None;
                self.finish_operation();
                self.leave(&mut out);
                self.av_mut().handle(event);
                Effect::default()
            }
            InputEvent::PointerButton {
                button,
                code,
                pressed,
            } => self.button(button, code, pressed, &mut out),
            InputEvent::Scroll {
                source,
                delta,
                value120,
            } => {
                self.scroll(event, source, delta, value120, &mut out);
                Effect::default()
            }
            InputEvent::PinchBegin if self.active_fs().is_some() => {
                // Leaves fullscreen and is consumed (contracts/input.md).
                self.exit_fullscreen(true);
                Effect::default()
            }
            InputEvent::PinchBegin
            | InputEvent::PinchUpdate { .. }
            | InputEvent::PinchEnd { .. } => {
                self.av_mut().handle(event);
                self.update_operation();
                Effect::default()
            }
            InputEvent::Key {
                keycode,
                binding_sym,
                pressed,
                time_ms,
            } => self.key(keycode, binding_sym, pressed, time_ms, &mut out),
            InputEvent::PointerMoved { delta } => {
                self.relative_motion(delta, &mut out);
                Effect::default()
            }
            InputEvent::Modifiers { mods, raw } => {
                self.router.mods = mods;
                self.router.raw = raw;
                if !self.bindings.mod_held(mods) {
                    // Releasing Mod commits the focus order (FR-011).
                    self.windows.end_cycle();
                }
                if let Some(window) = self.focused_window() {
                    out.push(ClientInput::Modifiers { window, raw });
                }
                self.av_mut().handle(event);
                Effect::default()
            }
            InputEvent::FocusLost => {
                // A console switch or lost keyboard cancels a pending quit (FR-031).
                self.router.quit.cancel();
                if self.router.viewer_focused && self.windows.focus().is_some() {
                    out.push(ClientInput::KeyboardFocus(None));
                }
                self.router.viewer_focused = false;
                self.router.grab = None;
                // The Mod release may never be seen: commit the focus order now.
                self.windows.end_cycle();
                self.router.held_keys.clear();
                self.router.consumed_keys.clear();
                self.av_mut().handle(event);
                Effect::default()
            }
            InputEvent::FocusGained => {
                if !self.router.viewer_focused
                    && let Some(window) = self.windows.focus()
                {
                    self.give_keyboard(window, &mut out);
                }
                self.router.viewer_focused = true;
                self.av_mut().handle(event);
                Effect::default()
            }
        };
        // A pan keeps acting on the viewer it started on until it ends (FR-019).
        self.pan_viewer = if self.av().is_panning() {
            self.pan_viewer.or(self.active)
        } else {
            // Also ended by the viewer itself (a pointer leave, the keyboard lost).
            self.router.pan_button = None;
            None
        };
        if panned && self.pan_viewer.is_none() {
            self.follow_pointer(&mut out);
        }
        (effect, out)
    }

    /// Once a pan is over, the viewer under the pointer is the active one again, without
    /// waiting for the next motion (FR-019). Returns whether it changed (and the pointer was
    /// re-targeted on the new viewer).
    fn follow_pointer(&mut self, out: &mut Vec<ClientInput>) -> bool {
        let on = self.pointer().map(|(id, _)| id);
        if on.is_none() || on == self.active {
            return false;
        }
        if let Some(slot) = self
            .active
            .and_then(|id| self.viewers.iter_mut().find(|s| s.id == id))
        {
            slot.viewer.handle(&InputEvent::PointerLeave);
        }
        self.sync_active();
        if let Some(p) = self.pointer_canvas() {
            self.retarget(p, false, out);
        }
        true
    }

    /// Relative pointer motion: the layout moves the pointer, and the viewer it is on (or the
    /// one a pan started on) gets the position relative to its top-left corner (research S9).
    fn relative_motion(&mut self, delta: Vector, out: &mut Vec<ClientInput>) {
        self.layout.move_by(delta);
        let pointer = self.layout.pointer();
        let target = self
            .pan_viewer
            .or_else(|| self.layout.locate(pointer).map(|(id, _)| id));
        let Some(target) = target else {
            return;
        };
        if self.pan_viewer.is_none() {
            self.active = Some(target);
        }
        let Some(pos) = self.layout.relative_to(target, pointer) else {
            return;
        };
        self.pointer_moved(&InputEvent::PointerMotion { pos }, pos, out);
    }

    /// The focused window, if the viewer has the keyboard.
    fn focused_window(&self) -> Option<WindowId> {
        self.windows.focus().filter(|_| self.router.viewer_focused)
    }

    fn focus_changed(&self, before: Option<WindowId>, out: &mut Vec<ClientInput>) {
        let now = self.windows.focus();
        if now == before || !self.router.viewer_focused {
            return;
        }
        match now {
            Some(window) => self.give_keyboard(window, out),
            None => out.push(ClientInput::KeyboardFocus(None)),
        }
    }

    /// Gives `window` the keyboard, telling it the current modifiers first: they may have
    /// changed while no window had the keyboard (a Mod released after Mod+Q closed the last
    /// window), and `wl_keyboard.enter` carries them.
    fn give_keyboard(&self, window: WindowId, out: &mut Vec<ClientInput>) {
        out.push(ClientInput::Modifiers {
            window,
            raw: self.router.raw,
        });
        out.push(ClientInput::KeyboardFocus(Some(window)));
    }

    /// The pointer position on the canvas, if it is over the viewer.
    fn pointer_canvas(&self) -> Option<Point> {
        let pos = self.av().pointer()?;
        Some(self.av().camera().to_canvas(pos, self.av().size()))
    }

    /// `p` (canvas) in the coordinates of `window`.
    fn local(&self, window: WindowId, p: Point) -> Option<Point> {
        let w = self.windows.get(window)?;
        Some(Point::new(p.x - w.origin.x, p.y - w.origin.y))
    }

    fn pointer_moved(&mut self, event: &InputEvent, pos: Point, out: &mut Vec<ClientInput>) {
        self.av_mut().handle(event);
        if self.router.operation.is_some() {
            // The window being moved or resized gets no pointer input (research W3).
            self.update_operation();
            return;
        }
        if self.av().is_panning() {
            // A canvas pan: applications get nothing (FR-012).
            return;
        }
        let canvas = self.av().camera().to_canvas(pos, self.av().size());
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
        if let Some(pan) = self.router.pan_button {
            // During a pan other buttons do nothing; the release of its own button ends it.
            if !pressed && pan == code {
                self.end_pan(out);
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
            // The pointer bindings (FR-032, contracts/config.md "Pointer combinations").
            match (self.bindings.pointer_lookup(self.router.mods, button), hit) {
                (Some(PointerAction::MoveWindow), Some((window, _))) => {
                    // Focus, raise, move (FR-002); a fullscreen or maximized window gets its
                    // earlier size first.
                    self.start_move(window, code, out);
                    return Effect::default();
                }
                (Some(PointerAction::ResizeWindow), Some((window, _))) => {
                    if self.active_fs().is_some() {
                        // Leaves fullscreen and is consumed (contracts/input.md).
                        self.exit_fullscreen(true);
                    } else {
                        self.start_resize(window, code, out);
                    }
                    return Effect::default();
                }
                (Some(PointerAction::ResizeWindow), None) => return Effect::default(),
                // Over the background a move pans; a pan pans anywhere, without focus.
                (Some(PointerAction::MoveWindow | PointerAction::PanCanvas), _) => {
                    self.pan_or_leave_fullscreen(code, out);
                    return Effect::default();
                }
                (None, _) => {}
            }
            // An ordinary click.
            let Some((window, local)) = hit else {
                // The background: left pans (or leaves fullscreen), other buttons do nothing;
                // focus stays (FR-013).
                if button == Button::Left {
                    self.pan_or_leave_fullscreen(code, out);
                }
                return Effect::default();
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
        }
        Effect::default()
    }

    /// A pan press: with a fullscreen window on the active viewer it leaves fullscreen and is
    /// consumed (contracts/input.md); otherwise the active viewer pans until `code` is released.
    fn pan_or_leave_fullscreen(&mut self, code: u32, out: &mut Vec<ClientInput>) {
        if self.active_fs().is_some() {
            self.exit_fullscreen(true);
            return;
        }
        // Applications get nothing during a pan (FR-012).
        self.leave(out);
        if self.av_mut().begin_pan() {
            self.router.pan_button = Some(code);
        }
    }

    /// The pan's button was released: the pointer re-enters the window under it.
    fn end_pan(&mut self, out: &mut Vec<ClientInput>) {
        self.av_mut().end_pan();
        self.router.pan_button = None;
        self.pan_viewer = None;
        if !self.follow_pointer(out)
            && let Some(p) = self.pointer_canvas()
        {
            self.retarget(p, false, out);
        }
    }

    fn scroll(
        &mut self,
        event: &InputEvent,
        source: ScrollSource,
        delta: Vector,
        value120: Option<Vector>,
        out: &mut Vec<ClientInput>,
    ) {
        if self.router.operation.is_some() {
            // Mod + scroll zooms around the pointer; plain scrolling does nothing.
            if self.bindings.mod_held(self.router.mods) {
                self.av_mut().handle(event);
                self.update_operation();
            }
            return;
        }
        let target = match &self.router.grab {
            Some(grab) => Some(grab.window),
            None => self
                .pointer_canvas()
                .and_then(|p| self.hit(p))
                .map(|(id, _)| id),
        };
        if self.active_fs().is_some()
            && (self.bindings.mod_held(self.router.mods) || target.is_none())
        {
            // Zooming or scrolling the canvas leaves fullscreen and is consumed.
            self.exit_fullscreen(true);
            return;
        }
        if self.av().is_panning() || self.bindings.mod_held(self.router.mods) {
            self.av_mut().handle(event);
            return;
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
            }
            None => self.av_mut().handle(event),
        }
    }

    fn key(
        &mut self,
        keycode: u32,
        sym: Keysym,
        pressed: bool,
        time_ms: u32,
        out: &mut Vec<ClientInput>,
    ) -> Effect {
        if pressed {
            let action = self.bindings.lookup(self.router.mods, sym, true);
            // Any other key press cancels a pending quit; modifier keys do not (FR-031).
            if action != Some(Action::Quit) && !sym.is_modifier_key() {
                self.router.quit.cancel();
            }
            if let Some(action) = action {
                self.router.consumed_keys.push(keycode);
                let operating = self.router.operation.is_some();
                return match action {
                    // Ignored during a move or resize (spec Edge Cases).
                    Action::NextWindow | Action::PreviousWindow | Action::ResetView
                        if operating =>
                    {
                        Effect::default()
                    }
                    Action::ResetView if self.active_fs().is_some() => {
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
                    Action::ResetView => {
                        // The active viewer's home view (FR-019, FR-020).
                        let home = self.active_home();
                        self.av_mut().set_camera(home);
                        Effect::default()
                    }
                    // Quitting takes two presses within 2 s, everywhere (FR-031).
                    Action::Quit => Effect {
                        quit: self.router.quit.press(time_ms),
                        ..Effect::default()
                    },
                    // `App` starts the command (FR-014); the key is consumed with its release.
                    Action::Spawn(n) => Effect {
                        spawn: Some(n),
                        ..Effect::default()
                    },
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
