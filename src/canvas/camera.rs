//! The camera of a viewer: which canvas point is at the viewer centre, and the zoom.
//!
//! Positions on screen are logical pixels with the origin at the viewer's top-left corner and
//! y pointing down; canvas positions are canvas units. At zoom 1.0 one canvas unit is one
//! logical pixel.

use super::{Point, Size, Vector};

/// Smallest zoom (10%).
pub const ZOOM_MIN: f64 = 0.1;
/// Largest zoom (1000%).
pub const ZOOM_MAX: f64 = 10.0;

/// What a viewer looks at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    /// The canvas point shown at the viewer centre.
    pub center: Point,
    /// Logical pixels per canvas unit, within `[ZOOM_MIN, ZOOM_MAX]`.
    pub zoom: f64,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            center: Point::new(0.0, 0.0),
            zoom: 1.0,
        }
    }
}

impl Camera {
    /// The canvas point under logical position `p` of a viewer of `size`.
    pub fn to_canvas(&self, p: Point, size: Size) -> Point {
        self.center + (p - size.center()) / self.zoom
    }

    /// The logical position of canvas point `p` in a viewer of `size`.
    pub fn to_logical(&self, p: Point, size: Size) -> Point {
        size.center() + (p - self.center) * self.zoom
    }

    /// Moves the camera so canvas point `anchor` is under logical position `p`. Drags anchor
    /// the point grabbed at the start, so long drags do not drift.
    pub fn pan_anchor(&mut self, anchor: Point, p: Point, size: Size) {
        self.center = anchor - (p - size.center()) / self.zoom;
    }

    /// Moves the canvas by `delta` logical pixels on screen (the centre moves the opposite
    /// way).
    pub fn pan_by(&mut self, delta: Vector) {
        self.center = self.center - delta / self.zoom;
    }

    /// Multiplies the zoom by `factor` around logical position `p`, clamped to the limits.
    /// Returns whether anything changed; at a limit the camera stays as it is.
    pub fn zoom_at(&mut self, factor: f64, p: Point, size: Size) -> bool {
        self.set_zoom_at(self.zoom * factor, p, size)
    }

    /// Sets the zoom (clamped) around logical position `p`, keeping the canvas point under
    /// `p` in place. Returns whether anything changed.
    pub fn set_zoom_at(&mut self, zoom: f64, p: Point, size: Size) -> bool {
        if !zoom.is_finite() {
            return false;
        }
        let zoom = zoom.clamp(ZOOM_MIN, ZOOM_MAX);
        if zoom == self.zoom {
            return false;
        }
        let under = self.to_canvas(p, size);
        self.zoom = zoom;
        self.pan_anchor(under, p, size);
        true
    }

    /// Back to the canvas origin at 100%.
    pub fn reset(&mut self) {
        *self = Camera::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::{Point, Size};

    const SIZE: Size = Size {
        w: 1280.0,
        h: 800.0,
    };

    fn close(a: Point, b: Point) -> bool {
        (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9
    }

    #[test]
    fn default_is_origin_at_100_percent() {
        let camera = Camera::default();
        assert_eq!(camera.center, Point::new(0.0, 0.0));
        assert_eq!(camera.zoom, 1.0);
    }

    #[test]
    fn viewer_centre_shows_camera_centre() {
        let camera = Camera {
            center: Point::new(10.0, -20.0),
            zoom: 2.0,
        };
        assert!(close(camera.to_canvas(SIZE.center(), SIZE), camera.center));
        assert!(close(camera.to_logical(camera.center, SIZE), SIZE.center()));
    }

    #[test]
    fn conversions_follow_the_formulas() {
        let camera = Camera {
            center: Point::new(100.0, 50.0),
            zoom: 2.0,
        };
        // canvas = center + (p - size/2) / zoom
        assert!(close(
            camera.to_canvas(Point::new(0.0, 0.0), SIZE),
            Point::new(-220.0, -150.0)
        ));
        // logical = (p - center) * zoom + size/2
        assert!(close(
            camera.to_logical(Point::new(0.0, 0.0), SIZE),
            Point::new(440.0, 300.0)
        ));
    }

    #[test]
    fn round_trips_at_several_zooms() {
        for zoom in [0.1, 1.0, 10.0] {
            let camera = Camera {
                center: Point::new(-3.5, 7.25),
                zoom,
            };
            for p in [
                Point::new(0.0, 0.0),
                Point::new(1279.0, 799.0),
                Point::new(-50.0, 900.0),
            ] {
                let back = camera.to_logical(camera.to_canvas(p, SIZE), SIZE);
                assert!(close(back, p), "zoom {zoom}: {p:?} -> {back:?}");
            }
        }
    }
}

#[cfg(test)]
mod pan_zoom_tests {
    use super::*;
    use crate::canvas::{Point, Size, Vector};
    use proptest::prelude::*;

    const SIZE: Size = Size {
        w: 1280.0,
        h: 800.0,
    };

    fn close(a: Point, b: Point, eps: f64) -> bool {
        (a.x - b.x).abs() < eps && (a.y - b.y).abs() < eps
    }

    fn coord() -> impl Strategy<Value = f64> {
        -1e6..1e6f64
    }

    fn screen() -> impl Strategy<Value = Point> {
        (-2000.0..3000.0f64, -2000.0..3000.0f64).prop_map(|(x, y)| Point::new(x, y))
    }

    fn zoom() -> impl Strategy<Value = f64> {
        ZOOM_MIN..=ZOOM_MAX
    }

    proptest! {
        /// SC-002: after anchoring, the anchor is under the pointer.
        #[test]
        fn pan_anchor_puts_anchor_under_pointer(
            cx in coord(), cy in coord(), z in zoom(),
            ax in coord(), ay in coord(), p in screen(),
        ) {
            let mut camera = Camera { center: Point::new(cx, cy), zoom: z };
            let anchor = Point::new(ax, ay);
            camera.pan_anchor(anchor, p, SIZE);
            prop_assert!(close(camera.to_logical(anchor, SIZE), p, 1e-6));
        }

        /// SC-003: zooming keeps the canvas point under the pointer in place.
        #[test]
        fn zoom_at_keeps_point_under_pointer(
            cx in coord(), cy in coord(), z in zoom(),
            factor in 0.5..2.0f64, p in screen(),
        ) {
            let mut camera = Camera { center: Point::new(cx, cy), zoom: z };
            let before = camera.to_canvas(p, SIZE);
            let changed = camera.zoom_at(factor, p, SIZE);
            prop_assert!((ZOOM_MIN..=ZOOM_MAX).contains(&camera.zoom));
            if changed {
                let after = camera.to_canvas(p, SIZE);
                prop_assert!(close(before, after, 1e-6 * camera.zoom.max(1.0)));
            }
        }

        /// FR-007: zoom stays within the limits, and at a limit nothing moves.
        #[test]
        fn zoom_is_clamped(cx in coord(), cy in coord(), p in screen()) {
            for (limit, factor) in [(ZOOM_MAX, 1.5), (ZOOM_MIN, 0.5)] {
                let mut camera = Camera { center: Point::new(cx, cy), zoom: limit };
                let before = camera;
                prop_assert!(!camera.zoom_at(factor, p, SIZE));
                prop_assert_eq!(camera, before);
            }
        }
    }

    #[test]
    fn zoom_past_a_limit_stops_at_the_limit() {
        let mut camera = Camera {
            center: Point::new(0.0, 0.0),
            zoom: 9.5,
        };
        assert!(camera.zoom_at(1.1, SIZE.center(), SIZE));
        assert_eq!(camera.zoom, ZOOM_MAX);
    }

    #[test]
    fn set_zoom_at_is_absolute_and_clamped() {
        let mut camera = Camera::default();
        let p = Point::new(100.0, 100.0);
        let before = camera.to_canvas(p, SIZE);
        assert!(camera.set_zoom_at(2.0, p, SIZE));
        assert_eq!(camera.zoom, 2.0);
        assert!(close(camera.to_canvas(p, SIZE), before, 1e-9));
        camera.set_zoom_at(100.0, p, SIZE);
        assert_eq!(camera.zoom, ZOOM_MAX);
    }

    #[test]
    fn reset_returns_to_the_default_view() {
        let mut camera = Camera::default();
        camera.pan_by(Vector::new(1234.5, -987.0));
        camera.zoom_at(3.7, Point::new(10.0, 20.0), SIZE);
        camera.reset();
        assert_eq!(camera, Camera::default());
    }

    #[test]
    fn pan_by_moves_centre_against_the_delta() {
        let mut camera = Camera {
            center: Point::new(10.0, 10.0),
            zoom: 2.0,
        };
        camera.pan_by(Vector::new(4.0, -6.0));
        assert_eq!(camera.center, Point::new(8.0, 13.0));
    }
}
