//! Input for applications: what the shell tells the server to deliver (data-model.md), and the
//! pointer and keyboard state needed to route it.

use crate::canvas::{Point, Rect, Size, Vector};
use crate::input::{Modifiers, RawModifiers, ScrollSource};

use super::{Edges, WindowId};

/// Input for an application's window, in window coordinates (backend-neutral).
#[derive(Debug, Clone, PartialEq)]
pub enum ClientInput {
    /// The pointer is now over (or grabbed by) this window.
    PointerEnter {
        window: WindowId,
        pos: Point,
    },
    PointerLeave {
        window: WindowId,
    },
    PointerMotion {
        window: WindowId,
        pos: Point,
    },
    /// `code` is the Linux input event code.
    PointerButton {
        window: WindowId,
        code: u32,
        pressed: bool,
    },
    /// Scroll values as the input device delivered them.
    Scroll {
        window: WindowId,
        source: ScrollSource,
        delta: Vector,
        value120: Option<Vector>,
    },
    /// Ends one group of pointer events.
    PointerFrame {
        window: WindowId,
    },
    /// Keyboard focus changed (`None`: no window, or the viewer lost the keyboard).
    KeyboardFocus(Option<WindowId>),
    /// `keycode` is the Linux evdev key code.
    Key {
        window: WindowId,
        keycode: u32,
        pressed: bool,
    },
    Modifiers {
        window: WindowId,
        raw: RawModifiers,
    },
}

/// A button pressed on a window: it gets all pointer input until the last button is released.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Grab {
    pub window: WindowId,
    pub buttons: Vec<u32>,
}

/// An interactive operation on a window (research W3).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Op {
    /// The window's origin follows the pointer: `origin = pointer - offset` (canvas).
    Move { window: WindowId, offset: Vector },
    /// The `edges` follow the pointer: the wanted size is `start_rect` grown by the pointer's
    /// canvas movement since `start_pointer`; `last` is the size asked for last; `restored`:
    /// the window left maximize or fullscreen when the resize started (a size was sent).
    Resize {
        window: WindowId,
        edges: Edges,
        start_pointer: Point,
        start_rect: Rect,
        last: Option<Size>,
        restored: bool,
    },
}

impl Op {
    pub fn window(&self) -> WindowId {
        match self {
            Op::Move { window, .. } | Op::Resize { window, .. } => *window,
        }
    }
}

/// An operation in progress and the buttons held for it; it ends on the last release.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Operation {
    pub op: Op,
    pub buttons: Vec<u32>,
}

/// Routing state.
#[derive(Debug, Clone, Default)]
pub(crate) struct Router {
    /// The window the pointer is in (entered), if any.
    pub entered: Option<WindowId>,
    pub grab: Option<Grab>,
    pub operation: Option<Operation>,
    /// The button that started the running pan; only its release ends it (FR-032).
    pub pan_button: Option<u32>,
    /// Keys pressed and delivered to the focused window.
    pub held_keys: Vec<u32>,
    /// Keys whose press triggered a compositor shortcut; their release is swallowed too.
    pub consumed_keys: Vec<u32>,
    /// Whether RubickWay has the keyboard (not paused on another console).
    pub viewer_focused: bool,
    /// Effective modifiers (for Mod gestures and shortcuts).
    pub mods: Modifiers,
    /// The keyboard's modifier masks as last reported, also while no window has the keyboard:
    /// a window getting the keyboard is told them first, so it never sees stale ones.
    pub raw: RawModifiers,
    /// The first press of the two-press quit, if one is pending (FR-031).
    pub quit: QuitConfirm,
}

/// The two-press quit (FR-031, data-model "QuitConfirm").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum QuitConfirm {
    #[default]
    Idle,
    /// A first press at `since` (event time, ms, wrapping).
    Pending { since: u32 },
}

/// How long after the first press a second one still quits.
const QUIT_WINDOW_MS: u32 = 2000;

impl QuitConfirm {
    /// A quit press at `t`: returns whether RubickWay quits now.
    pub fn press(&mut self, t: u32) -> bool {
        match *self {
            QuitConfirm::Pending { since } if t.wrapping_sub(since) <= QUIT_WINDOW_MS => {
                *self = QuitConfirm::Idle;
                true
            }
            _ => {
                *self = QuitConfirm::Pending { since: t };
                false
            }
        }
    }

    /// Another key was pressed, or the keyboard went away: a pending quit is cancelled.
    pub fn cancel(&mut self) {
        *self = QuitConfirm::Idle;
    }
}
