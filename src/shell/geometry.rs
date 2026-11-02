//! Pure size, edge and view maths for window management (research W2, W3, W6, W9).

use crate::canvas::{Camera, Point, Rect, Size, Vector, ZOOM_MAX, ZOOM_MIN};

/// Window edges being dragged in a resize (backend-neutral `xdg_toplevel.resize_edge`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Edges {
    pub top: bool,
    pub bottom: bool,
    pub left: bool,
    pub right: bool,
}

impl Edges {
    pub const NONE: Edges = Edges {
        top: false,
        bottom: false,
        left: false,
        right: false,
    };

    /// The edges for an `xdg_toplevel.resize_edge` value; `None` for values outside the enum.
    pub fn from_resize_edge(value: u32) -> Option<Edges> {
        let (top, bottom, left, right) = match value {
            0 => (false, false, false, false),
            1 => (true, false, false, false),
            2 => (false, true, false, false),
            4 => (false, false, true, false),
            5 => (true, false, true, false),
            6 => (false, true, true, false),
            8 => (false, false, false, true),
            9 => (true, false, false, true),
            10 => (false, true, false, true),
            _ => return None,
        };
        Some(Edges {
            top,
            bottom,
            left,
            right,
        })
    }

    /// One edge, or two adjacent ones (a corner).
    pub fn is_valid(self) -> bool {
        let vertical = usize::from(self.top) + usize::from(self.bottom);
        let horizontal = usize::from(self.left) + usize::from(self.right);
        vertical <= 1 && horizontal <= 1 && vertical + horizontal >= 1
    }
}

/// An application's size limits in canvas units; 0 means none (min) or unlimited (max).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SizeLimits {
    pub min: Size,
    pub max: Size,
}

impl SizeLimits {
    /// `size` within the limits, at least 1 x 1 and never above `largest` (FR-008): the
    /// minimum wins over a smaller maximum, `largest` wins over everything (research W6).
    pub fn clamp(&self, size: Size, largest: Size) -> Size {
        Size::new(
            clamp_axis(size.w, self.min.w, self.max.w, largest.w),
            clamp_axis(size.h, self.min.h, self.max.h, largest.h),
        )
    }
}

/// The edges a compositor resize started at `p` drags (FR-006): `rect` split 3 x 3 with
/// half-open cells (a boundary point belongs to the cell further right or down); corner cells
/// give corners, edge cells edges, the centre cell the nearest corner (ties: bottom / right).
pub fn pick_edges(rect: Rect, p: Point) -> Edges {
    let local = p - rect.origin;
    let (w, h) = (rect.size.w, rect.size.h);
    let column = third(local.x, w);
    let row = third(local.y, h);
    let (left, right) = match column {
        0 => (true, false),
        2 => (false, true),
        _ if row == 1 => (local.x < w / 2.0, local.x >= w / 2.0),
        _ => (false, false),
    };
    let (top, bottom) = match row {
        0 => (true, false),
        2 => (false, true),
        _ if column == 1 => (local.y < h / 2.0, local.y >= h / 2.0),
        _ => (false, false),
    };
    Edges {
        top,
        bottom,
        left,
        right,
    }
}

/// Which third of `[0, len]` holds `v`: 0, 1 or 2.
fn third(v: f64, len: f64) -> u8 {
    if v < len / 3.0 {
        0
    } else if v < 2.0 * len / 3.0 {
        1
    } else {
        2
    }
}

/// The size of `start` with its `edges` dragged by `delta` (canvas units, research W3).
pub fn resize_size(start: Rect, edges: Edges, delta: Vector) -> Size {
    let mut w = start.size.w;
    let mut h = start.size.h;
    if edges.right {
        w += delta.x;
    } else if edges.left {
        w -= delta.x;
    }
    if edges.bottom {
        h += delta.y;
    } else if edges.top {
        h -= delta.y;
    }
    Size::new(w, h)
}

/// The camera that brings `rect` into a view of `view_size` (FR-012): `None` when it is fully
/// inside already; else centred on it, keeping the zoom if it fits, otherwise zoomed out so it
/// fills at most 90% of the view in each direction (a 5% margin per side), never below
/// `ZOOM_MIN`.
pub fn reveal(camera: Camera, view_size: Size, rect: Rect) -> Option<Camera> {
    let top_left = camera.to_canvas(Point::new(0.0, 0.0), view_size);
    let view = Rect::new(
        top_left,
        Size::new(view_size.w / camera.zoom, view_size.h / camera.zoom),
    );
    let end = rect.end();
    let view_end = view.end();
    if rect.origin.x >= view.origin.x
        && rect.origin.y >= view.origin.y
        && end.x <= view_end.x
        && end.y <= view_end.y
    {
        return None;
    }
    let fits = rect.size.w * camera.zoom <= view_size.w && rect.size.h * camera.zoom <= view_size.h;
    let zoom = if fits {
        camera.zoom
    } else {
        let fit_w = 0.9 * view_size.w / rect.size.w.max(f64::MIN_POSITIVE);
        let fit_h = 0.9 * view_size.h / rect.size.h.max(f64::MIN_POSITIVE);
        camera.zoom.min(fit_w).min(fit_h).clamp(ZOOM_MIN, ZOOM_MAX)
    };
    Some(Camera {
        center: rect.origin + rect.size.to_vector() / 2.0,
        zoom,
    })
}

fn clamp_axis(value: f64, min: f64, max: f64, largest: f64) -> f64 {
    let low = min.max(1.0);
    let mut v = value.max(low);
    if max > 0.0 {
        v = v.min(max.max(low));
    }
    v.min(largest.max(1.0))
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn edges(top: bool, bottom: bool, left: bool, right: bool) -> Edges {
        Edges {
            top,
            bottom,
            left,
            right,
        }
    }

    #[test]
    fn resize_edge_values_map_to_edges() {
        let cases = [
            (1, edges(true, false, false, false)),
            (2, edges(false, true, false, false)),
            (4, edges(false, false, true, false)),
            (5, edges(true, false, true, false)),
            (6, edges(false, true, true, false)),
            (8, edges(false, false, false, true)),
            (9, edges(true, false, false, true)),
            (10, edges(false, true, false, true)),
        ];
        for (value, expected) in cases {
            assert_eq!(Edges::from_resize_edge(value), Some(expected), "{value}");
            assert!(expected.is_valid(), "{value}");
        }
    }

    #[test]
    fn resize_edge_none_is_known_but_not_valid() {
        assert_eq!(Edges::from_resize_edge(0), Some(Edges::NONE));
        assert!(!Edges::NONE.is_valid());
    }

    #[test]
    fn unknown_resize_edge_values_are_rejected() {
        for value in [3, 7, 11, 12, 13, 14, 15, 16, 255, u32::MAX] {
            assert_eq!(Edges::from_resize_edge(value), None, "{value}");
        }
    }

    #[test]
    fn opposite_or_more_than_two_edges_are_invalid() {
        assert!(!edges(true, true, false, false).is_valid());
        assert!(!edges(false, false, true, true).is_valid());
        assert!(!edges(true, false, true, true).is_valid());
        assert!(!edges(true, true, true, true).is_valid());
    }

    const UNLIMITED: Size = Size {
        w: f64::INFINITY,
        h: f64::INFINITY,
    };

    fn limits(min: (f64, f64), max: (f64, f64)) -> SizeLimits {
        SizeLimits {
            min: Size::new(min.0, min.1),
            max: Size::new(max.0, max.1),
        }
    }

    #[test]
    fn no_limits_keep_the_size_but_at_least_one() {
        let l = SizeLimits::default();
        assert_eq!(
            l.clamp(Size::new(300.0, 200.0), UNLIMITED),
            Size::new(300.0, 200.0)
        );
        assert_eq!(l.clamp(Size::new(0.0, 0.5), UNLIMITED), Size::new(1.0, 1.0));
    }

    #[test]
    fn min_and_max_bound_the_size() {
        let l = limits((100.0, 50.0), (400.0, 300.0));
        assert_eq!(
            l.clamp(Size::new(10.0, 10.0), UNLIMITED),
            Size::new(100.0, 50.0)
        );
        assert_eq!(
            l.clamp(Size::new(900.0, 900.0), UNLIMITED),
            Size::new(400.0, 300.0)
        );
        assert_eq!(
            l.clamp(Size::new(200.0, 100.0), UNLIMITED),
            Size::new(200.0, 100.0)
        );
    }

    #[test]
    fn zero_max_is_unlimited_per_component() {
        let l = limits((0.0, 0.0), (0.0, 300.0));
        assert_eq!(
            l.clamp(Size::new(5000.0, 5000.0), UNLIMITED),
            Size::new(5000.0, 300.0)
        );
    }

    #[test]
    fn a_max_below_min_counts_as_min() {
        let l = limits((200.0, 200.0), (100.0, 100.0));
        assert_eq!(
            l.clamp(Size::new(50.0, 500.0), UNLIMITED),
            Size::new(200.0, 200.0)
        );
    }

    #[test]
    fn the_largest_window_wins_over_min() {
        let l = limits((5000.0, 100.0), (0.0, 0.0));
        let largest = Size::new(4096.0, 4096.0);
        assert_eq!(
            l.clamp(Size::new(6000.0, 6000.0), largest),
            Size::new(4096.0, 4096.0)
        );
        assert_eq!(
            l.clamp(Size::new(10.0, 10.0), largest),
            Size::new(4096.0, 100.0)
        );
    }

    fn rect() -> Rect {
        // 300 x 150 at (100, 50): thirds at x 200 / 300, y 100 / 150.
        Rect::new(Point::new(100.0, 50.0), Size::new(300.0, 150.0))
    }

    #[test]
    fn the_outer_cells_pick_edges_and_corners() {
        let r = rect();
        let cases = [
            ((110.0, 60.0), edges(true, false, true, false)),
            ((250.0, 60.0), edges(true, false, false, false)),
            ((390.0, 60.0), edges(true, false, false, true)),
            ((110.0, 120.0), edges(false, false, true, false)),
            ((390.0, 120.0), edges(false, false, false, true)),
            ((110.0, 190.0), edges(false, true, true, false)),
            ((250.0, 190.0), edges(false, true, false, false)),
            ((390.0, 190.0), edges(false, true, false, true)),
        ];
        for ((x, y), expected) in cases {
            assert_eq!(pick_edges(r, Point::new(x, y)), expected, "({x}, {y})");
        }
    }

    #[test]
    fn cell_boundaries_belong_to_the_cell_right_or_below() {
        let r = rect();
        // x = 200 is the first boundary: middle column; y = 100: middle row -> centre cell.
        assert_eq!(
            pick_edges(r, Point::new(200.0, 60.0)),
            edges(true, false, false, false)
        );
        // x = 300 is the second boundary: right column.
        assert_eq!(
            pick_edges(r, Point::new(300.0, 120.0)),
            edges(false, false, false, true)
        );
        // y = 150 is the second boundary: bottom row.
        assert_eq!(
            pick_edges(r, Point::new(250.0, 150.0)),
            edges(false, true, false, false)
        );
    }

    #[test]
    fn the_centre_cell_picks_the_nearest_corner_ties_bottom_right() {
        let r = rect();
        assert_eq!(
            pick_edges(r, Point::new(220.0, 110.0)),
            edges(true, false, true, false)
        );
        assert_eq!(
            pick_edges(r, Point::new(280.0, 140.0)),
            edges(false, true, false, true)
        );
        // The exact centre (250, 125).
        assert_eq!(
            pick_edges(r, Point::new(250.0, 125.0)),
            edges(false, true, false, true)
        );
    }

    #[test]
    fn resize_size_grows_the_dragged_edges() {
        let r = rect();
        let d = Vector::new(30.0, -20.0);
        assert_eq!(
            resize_size(r, edges(false, true, false, true), d),
            Size::new(330.0, 130.0)
        );
        assert_eq!(
            resize_size(r, edges(true, false, true, false), d),
            Size::new(270.0, 170.0)
        );
        assert_eq!(
            resize_size(r, edges(false, false, true, false), d),
            Size::new(270.0, 150.0)
        );
        assert_eq!(
            resize_size(r, edges(true, false, false, false), d),
            Size::new(300.0, 170.0)
        );
    }

    const VIEW: Size = Size {
        w: 1000.0,
        h: 800.0,
    };

    fn camera(x: f64, y: f64, zoom: f64) -> Camera {
        Camera {
            center: Point::new(x, y),
            zoom,
        }
    }

    #[test]
    fn a_window_fully_in_view_needs_no_camera_change() {
        // The view at zoom 1 centred on 0 covers (-500, -400)..(500, 400).
        let r = Rect::new(Point::new(300.0, 200.0), Size::new(200.0, 200.0));
        assert_eq!(reveal(camera(0.0, 0.0, 1.0), VIEW, r), None);
    }

    #[test]
    fn a_window_that_fits_is_centred_at_the_same_zoom() {
        let r = Rect::new(Point::new(5000.0, -3000.0), Size::new(400.0, 300.0));
        assert_eq!(
            reveal(camera(0.0, 0.0, 2.0), VIEW, r),
            Some(camera(5200.0, -2850.0, 2.0))
        );
        // Partly visible counts as not fully inside.
        let r = Rect::new(Point::new(450.0, 0.0), Size::new(100.0, 100.0));
        assert_eq!(
            reveal(camera(0.0, 0.0, 1.0), VIEW, r),
            Some(camera(500.0, 50.0, 1.0))
        );
    }

    #[test]
    fn a_window_too_big_zooms_out_with_a_margin() {
        // 2000 x 400 at zoom 1 does not fit 1000 wide: 90% of 1000 / 2000 = 0.45.
        let r = Rect::new(Point::new(-1000.0, 1000.0), Size::new(2000.0, 400.0));
        let c = reveal(camera(0.0, 0.0, 1.0), VIEW, r).expect("a camera");
        assert_eq!(c.center, Point::new(0.0, 1200.0));
        assert!((c.zoom - 0.45).abs() < 1e-12);
    }

    #[test]
    fn zooming_out_stops_at_the_minimum() {
        let r = Rect::new(Point::new(0.0, 0.0), Size::new(100_000.0, 100.0));
        let c = reveal(camera(0.0, 0.0, 1.0), VIEW, r).expect("a camera");
        assert_eq!(c.zoom, ZOOM_MIN);
        assert_eq!(c.center, Point::new(50_000.0, 50.0));
    }

    proptest! {
        #[test]
        fn clamp_is_always_between_one_and_largest(
            w in -1e6f64..1e6, h in -1e6f64..1e6,
            min_w in 0.0f64..1e5, min_h in 0.0f64..1e5,
            max_w in 0.0f64..1e5, max_h in 0.0f64..1e5,
            lw in 1.0f64..1e5, lh in 1.0f64..1e5,
        ) {
            let l = limits((min_w, min_h), (max_w, max_h));
            let s = l.clamp(Size { w, h }, Size::new(lw, lh));
            prop_assert!(s.w >= 1.0 && s.h >= 1.0);
            prop_assert!(s.w <= lw && s.h <= lh);
        }
    }
}
