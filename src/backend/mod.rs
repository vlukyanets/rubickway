//! Host backends. They speak `InputEvent`s and window events; nothing above them knows which
//! backend is running.

pub mod wayland_host;

use crate::canvas::Size;
use crate::input::InputEvent;

/// An event from a backend to the rest of the compositor.
#[derive(Debug, Clone, PartialEq)]
pub enum BackendEvent {
    /// Input for the viewer.
    Input(InputEvent),
    /// The host decided the viewer's logical size. A zero component means "your choice".
    Configure { size: Size },
    /// The host's preferred scale for the viewer changed.
    Scale(f64),
    /// The host is ready for the next frame.
    FrameDone,
    /// The host asked to close the viewer window.
    CloseRequested,
    /// The host's keymap (xkb text v1), forwarded to applications.
    Keymap(String),
    /// The host's key repeat settings: `rate` keys per second (0 = no repeat), `delay` ms.
    RepeatInfo { rate: i32, delay: i32 },
}
