//! The hardware backend and the watchdog. The backend speaks `InputEvent`s and display events;
//! nothing above it knows about devices.

pub mod drm;
pub mod watchdog;

use crate::canvas::ViewerId;
use crate::input::InputEvent;

/// An event from the backend to the rest of the compositor.
#[derive(Debug, Clone, PartialEq)]
pub enum BackendEvent {
    /// Input for the viewers.
    Input(InputEvent),
    /// The keymap (xkb text v1), forwarded to applications.
    Keymap(String),
    /// Key repeat settings for applications: `rate` keys per second (0 = no repeat), `delay`
    /// ms.
    RepeatInfo { rate: i32, delay: i32 },
    /// A display is connected and becomes a viewer (research S13).
    OutputAdded { id: ViewerId, info: OutputInfo },
    /// A display went away.
    OutputRemoved { id: ViewerId },
    /// Display `id` showed the last frame drawn for it (page flip), at `time_ms` on the frame
    /// clock.
    Presented { id: ViewerId, time_ms: u32 },
    /// The session lost the devices (console switched away, research S12).
    Paused,
    /// The devices are back.
    Resumed,
}

/// A display as applications and the log see it (FR-015).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputInfo {
    /// The connector name, e.g. `eDP-1`, `HDMI-A-1`.
    pub name: String,
    /// From the EDID; `Unknown` when the display does not report it.
    pub make: String,
    pub model: String,
    /// Physical size in millimetres; 0 when unknown.
    pub physical_mm: (u32, u32),
    pub mode: Mode,
    /// A built-in panel (eDP, LVDS, DSI).
    pub internal: bool,
}

/// A display mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mode {
    pub width: u32,
    pub height: u32,
    /// Refresh rate in millihertz.
    pub refresh_mhz: u32,
}
