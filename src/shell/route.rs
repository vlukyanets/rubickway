//! Input for applications: what the shell tells the server to deliver (data-model.md), and the
//! pointer and keyboard state needed to route it.

use crate::canvas::{Point, Vector};
use crate::input::{Modifiers, RawModifiers, ScrollSource};

use super::WindowId;

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
    /// Scroll values as the host delivered them.
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

/// Routing state.
#[derive(Debug, Clone, Default)]
pub(crate) struct Router {
    /// The window the pointer is in (entered), if any.
    pub entered: Option<WindowId>,
    pub grab: Option<Grab>,
    /// Keys pressed and delivered to the focused window.
    pub held_keys: Vec<u32>,
    /// Keys whose press triggered a compositor shortcut; their release is swallowed too.
    pub consumed_keys: Vec<u32>,
    /// Whether the host gave the viewer the keyboard.
    pub viewer_focused: bool,
    /// Effective modifiers (for Mod gestures and shortcuts).
    pub mods: Modifiers,
}
