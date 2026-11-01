//! Backend-neutral input: what backends produce and viewers consume.
//!
//! Positions are logical pixels relative to the viewer's top-left corner, y down.

pub mod bindings;
pub mod keyboard;

use crate::canvas::{Point, Vector};

pub use xkbcommon::xkb::Keysym;

/// One input event, already translated from the host or device representation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InputEvent {
    /// The pointer entered the viewer at `pos`.
    PointerEnter { pos: Point },
    /// The pointer moved to `pos` (may be outside the viewer during a drag).
    PointerMotion { pos: Point },
    /// The pointer left the viewer.
    PointerLeave,
    /// A pointer button changed state.
    PointerButton { button: Button, pressed: bool },
    /// One scroll frame. `delta` in logical pixels, positive `y` = scroll down, as delivered by
    /// the host (natural scrolling already applied). `value120` is set for wheels: one notch is
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
    /// A key changed state. `binding_sym` is the key's symbol in the first layout at the base
    /// level, so shortcuts work whatever layout is active.
    Key { binding_sym: Keysym, pressed: bool },
    /// The effective modifiers changed.
    Modifiers { mods: Modifiers },
    /// The viewer lost keyboard or pointer focus.
    FocusLost,
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
