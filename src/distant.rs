//! Less work for windows far from the viewer, as games do with level of detail (FR-017, research
//! G4). Pure: nothing here knows of Wayland or GL. With nothing configured nothing changes.

use std::collections::HashMap;

/// The `distant-windows` block of the configuration. Every value is optional.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DistantWindows {
    /// Windows on a display at this canvas zoom or below are distant.
    pub below_zoom_percent: Option<u32>,
    /// How many times a second a distant window is asked for a new frame.
    pub max_fps: Option<u32>,
    /// The copy smoothing a distant image, as a percentage of the size it is drawn at.
    pub copy_detail_percent: Option<u32>,
}

impl DistantWindows {
    /// Whether windows are distant on a display at `zoom`.
    pub fn is_distant(&self, zoom: f64) -> bool {
        self.below_zoom_percent
            .is_some_and(|percent| zoom <= f64::from(percent) / 100.0)
    }

    /// The milliseconds between frames of a window on a display at `zoom`, if it is held back.
    pub fn frame_interval_ms(&self, zoom: f64) -> Option<u32> {
        if !self.is_distant(zoom) {
            return None;
        }
        self.max_fps.map(|fps| (1000 / fps.max(1)).max(1))
    }

    /// The detail of smoothing copies on a display at `zoom`, as a fraction of the drawn size.
    pub fn copy_detail(&self, zoom: f64) -> Option<f32> {
        if !self.is_distant(zoom) {
            return None;
        }
        self.copy_detail_percent
            .map(|percent| percent as f32 / 100.0)
    }

    /// The startup log line, if anything is configured (`contracts/cli.md`).
    pub fn describe(&self) -> Option<String> {
        let zoom = self.below_zoom_percent?;
        let mut parts = Vec::new();
        if let Some(fps) = self.max_fps {
            parts.push(format!("asked for at most {fps} frames a second"));
        }
        if let Some(detail) = self.copy_detail_percent {
            parts.push(format!(
                "smoothed with a copy at {detail}% of the size they are drawn at"
            ));
        }
        let line = format!("windows at {zoom}% zoom or below are distant");
        Some(if parts.is_empty() {
            line
        } else {
            format!("{line}: {}", parts.join(", "))
        })
    }
}

/// When each distant window last got its frame callbacks.
#[derive(Debug, Default)]
pub struct FramePace {
    last_ms: HashMap<u64, u32>,
}

impl FramePace {
    /// Whether `window` may have its frame callbacks at `now_ms`, given the interval. The clock may
    /// wrap.
    pub fn due(&mut self, window: u64, now_ms: u32, interval_ms: u32) -> bool {
        let is_due = self
            .last_ms
            .get(&window)
            .is_none_or(|last| now_ms.wrapping_sub(*last) >= interval_ms);
        if is_due {
            self.last_ms.insert(window, now_ms);
        }
        is_due
    }

    /// The window is gone.
    pub fn forget(&mut self, window: u64) {
        self.last_ms.remove(&window);
    }
}

/// The size of a smoothing copy: `detail` of the drawn size, whole pixels, never above the image's
/// size and never zero. Drawn sizes are given along the image's own axes.
pub fn copy_size(image: (u32, u32), drawn: (f32, f32), detail: Option<f32>) -> (u32, u32) {
    let Some(detail) = detail else {
        return image;
    };
    let side =
        |image: u32, drawn: f32| ((drawn.max(0.0) * detail).ceil() as u32).clamp(1, image.max(1));
    (side(image.0, drawn.0), side(image.1, drawn.1))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: DistantWindows = DistantWindows {
        below_zoom_percent: Some(50),
        max_fps: Some(20),
        copy_detail_percent: Some(100),
    };

    #[test]
    fn at_or_below_the_zoom_a_window_is_distant() {
        assert!(ALL.is_distant(0.25));
        assert!(ALL.is_distant(0.5));
        assert!(!ALL.is_distant(0.51));
        assert!(!ALL.is_distant(1.0));
    }

    #[test]
    fn with_nothing_configured_nothing_is_distant() {
        let none = DistantWindows::default();
        assert!(!none.is_distant(0.01));
        assert_eq!(none.frame_interval_ms(0.01), None);
        assert_eq!(none.copy_detail(0.01), None);
        assert_eq!(none.describe(), None);
    }

    #[test]
    fn each_parameter_works_on_its_own() {
        let only_fps = DistantWindows {
            max_fps: Some(20),
            copy_detail_percent: None,
            ..ALL
        };
        assert_eq!(only_fps.frame_interval_ms(0.25), Some(50));
        assert_eq!(only_fps.copy_detail(0.25), None);
        let only_detail = DistantWindows {
            max_fps: None,
            copy_detail_percent: Some(200),
            ..ALL
        };
        assert_eq!(only_detail.frame_interval_ms(0.25), None);
        assert_eq!(only_detail.copy_detail(0.25), Some(2.0));
    }

    #[test]
    fn above_the_zoom_no_parameter_applies() {
        assert_eq!(ALL.frame_interval_ms(1.0), None);
        assert_eq!(ALL.copy_detail(1.0), None);
    }

    #[test]
    fn without_the_zoom_no_parameter_applies() {
        let no_zoom = DistantWindows {
            below_zoom_percent: None,
            ..ALL
        };
        assert_eq!(no_zoom.frame_interval_ms(0.01), None);
        assert_eq!(no_zoom.copy_detail(0.01), None);
    }

    #[test]
    fn the_log_line_names_only_what_is_set() {
        assert_eq!(
            ALL.describe().as_deref(),
            Some(
                "windows at 50% zoom or below are distant: asked for at most 20 frames a second, \
                 smoothed with a copy at 100% of the size they are drawn at"
            )
        );
        let only_fps = DistantWindows {
            copy_detail_percent: None,
            ..ALL
        };
        assert_eq!(
            only_fps.describe().as_deref(),
            Some("windows at 50% zoom or below are distant: asked for at most 20 frames a second")
        );
    }

    #[test]
    fn a_window_is_let_through_once_per_interval() {
        let mut pace = FramePace::default();
        let w = 1;
        assert!(pace.due(w, 1000, 50), "the first time");
        assert!(!pace.due(w, 1017, 50));
        assert!(!pace.due(w, 1049, 50));
        assert!(pace.due(w, 1050, 50));
        assert!(!pace.due(w, 1067, 50));
    }

    #[test]
    fn windows_have_their_own_pace() {
        let mut pace = FramePace::default();
        assert!(pace.due(1, 1000, 50));
        assert!(pace.due(2, 1010, 50));
        assert!(!pace.due(1, 1020, 50));
    }

    #[test]
    fn the_clock_may_wrap() {
        let mut pace = FramePace::default();
        let w = 1;
        assert!(pace.due(w, u32::MAX - 10, 50));
        assert!(!pace.due(w, 5, 50));
        assert!(pace.due(w, 40, 50));
    }

    #[test]
    fn a_forgotten_window_starts_anew() {
        let mut pace = FramePace::default();
        let w = 1;
        assert!(pace.due(w, 1000, 50));
        pace.forget(w);
        assert!(pace.due(w, 1001, 50));
    }

    #[test]
    fn without_a_detail_the_copy_has_the_size_of_the_image() {
        assert_eq!(copy_size((1000, 500), (100.0, 50.0), None), (1000, 500));
    }

    #[test]
    fn the_copy_follows_the_drawn_size_and_detail() {
        assert_eq!(copy_size((1000, 500), (100.0, 50.0), Some(1.0)), (100, 50));
        assert_eq!(copy_size((1000, 500), (100.0, 50.0), Some(2.0)), (200, 100));
        assert_eq!(copy_size((1000, 500), (100.0, 50.0), Some(0.5)), (50, 25));
    }

    #[test]
    fn the_copy_is_never_above_the_image_or_empty() {
        assert_eq!(
            copy_size((1000, 500), (900.0, 450.0), Some(4.0)),
            (1000, 500)
        );
        assert_eq!(copy_size((1000, 500), (0.1, 0.1), Some(0.25)), (1, 1));
        assert_eq!(copy_size((1000, 500), (0.0, -5.0), Some(1.0)), (1, 1));
    }

    #[test]
    fn a_fractional_size_rounds_up() {
        assert_eq!(copy_size((1000, 500), (100.5, 50.2), Some(1.0)), (101, 51));
    }
}
