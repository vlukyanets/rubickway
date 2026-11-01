//! The pointer gesture in progress (data-model.md, gesture state machine).

use crate::canvas::Point;

/// What the pointer is doing.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Gesture {
    #[default]
    Idle,
    /// A pan drag; `anchor` is the canvas point grabbed at the start.
    Panning { anchor: Point },
    /// A touchpad pinch; zoom is relative to the zoom at the begin.
    Pinching { zoom_at_begin: f64 },
}

impl Gesture {
    pub fn is_panning(self) -> bool {
        matches!(self, Gesture::Panning { .. })
    }
}
