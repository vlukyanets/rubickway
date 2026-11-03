//! Backend-neutral input: what backends produce and viewers consume.
//!
//! Positions are logical pixels relative to the viewer's top-left corner, y down.

pub mod bindings;
pub mod keyboard;

use crate::canvas::{Point, Vector};

pub use xkbcommon::xkb::Keysym;

/// One input event, already translated from the device representation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InputEvent {
    /// The pointer moved by `delta` logical pixels (relative, accelerated motion from a
    /// device). The shell turns it into per-viewer enter, motion and leave events through the
    /// layout (research S9).
    PointerMoved { delta: Vector },
    /// The pointer entered the viewer at `pos`.
    PointerEnter { pos: Point },
    /// The pointer moved to `pos` (may be outside the viewer during a drag).
    PointerMotion { pos: Point },
    /// The pointer left the viewer.
    PointerLeave,
    /// A pointer button changed state. `code` is its Linux input event code.
    PointerButton {
        button: Button,
        code: u32,
        pressed: bool,
    },
    /// One scroll frame. `delta` in logical pixels, positive `y` = scroll down, as libinput
    /// delivers it (natural scrolling off). `value120` is set for wheels: one notch is
    /// 120, high-resolution wheels report fractions of it.
    Scroll {
        source: ScrollSource,
        delta: Vector,
        value120: Option<Vector>,
    },
    /// A two-finger pinch started.
    PinchBegin,
    /// The pinch scale changed; `scale` is relative to the begin, always `> 0`.
    PinchUpdate { scale: f64 },
    /// The pinch ended or was cancelled.
    PinchEnd { cancelled: bool },
    /// A key changed state. `keycode` is the Linux evdev key code; `binding_sym` is the key's
    /// symbol in the first layout at the base level, so shortcuts work whatever layout is
    /// active. `time_ms` is the event time in milliseconds (wrapping), used for the two-press
    /// quit (FR-031).
    Key {
        keycode: u32,
        binding_sym: Keysym,
        pressed: bool,
        time_ms: u32,
    },
    /// The modifiers changed: `mods` are the effective ones, `raw` the masks as xkb reports
    /// them (forwarded to applications).
    Modifiers { mods: Modifiers, raw: RawModifiers },
    /// The viewer lost keyboard or pointer focus.
    FocusLost,
    /// The viewer got the keyboard (back).
    FocusGained,
}

/// Modifier and layout state as xkb masks, forwarded unchanged to applications.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RawModifiers {
    pub depressed: u32,
    pub latched: u32,
    pub locked: u32,
    pub group: u32,
}

/// A pointer button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
    Middle,
    /// Any other button, by its Linux input event code.
    Other(u32),
}

impl Button {
    /// The button for Linux input event code `code` (`BTN_LEFT` = 0x110, ...).
    pub fn from_code(code: u32) -> Button {
        match code {
            0x110 => Button::Left,
            0x111 => Button::Right,
            0x112 => Button::Middle,
            other => Button::Other(other),
        }
    }
}

/// What produced a scroll event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollSource {
    /// A mouse wheel, in notches.
    Wheel,
    /// Fingers on a touchpad.
    Finger,
    /// A continuous device such as a trackpoint; treated like `Finger`.
    Continuous,
}

/// Effective modifiers (depressed and latched; locked ones such as Caps Lock excluded).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub logo: bool,
}

impl Modifiers {
    /// No modifiers.
    pub const NONE: Modifiers = Modifiers {
        ctrl: false,
        alt: false,
        shift: false,
        logo: false,
    };
}
