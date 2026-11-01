//! Windows on the canvas: placement, stacking, focus and hit-testing (data-model.md).
//!
//! Window coordinates are the application's logical pixels relative to the window geometry's
//! top-left; one canvas unit is one such pixel, so `local = canvas point - origin` at any zoom.

use crate::canvas::{Point, Rect, Size};

/// A window's identity, assigned by the server when a toplevel first shows content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WindowId(pub u64);

/// A window on the canvas.
#[derive(Debug, Clone, PartialEq)]
pub struct Window {
    pub id: WindowId,
    /// Canvas position of the geometry's top-left; set once on map, kept on resize.
    pub origin: Point,
    /// Size in canvas units.
    pub size: Size,
    /// Where the window accepts pointer input, in window coordinates.
    pub input: Vec<Rect>,
}

impl Window {
    /// The window's rectangle on the canvas.
    pub fn rect(&self) -> Rect {
        Rect::new(self.origin, self.size)
    }
}

/// All windows, back to front, and keyboard focus.
#[derive(Debug, Clone, Default)]
pub struct Windows {
    stack: Vec<Window>,
    focus: Option<WindowId>,
}

impl Windows {
    /// Adds a window on top, centred on `view_center`, and focuses it (FR-004, FR-008).
    pub fn map(&mut self, id: WindowId, size: Size, input: Vec<Rect>, view_center: Point) {
        self.stack.retain(|w| w.id != id);
        let origin = view_center - size.to_vector() / 2.0;
        self.stack.push(Window {
            id,
            origin,
            size,
            input,
        });
        self.focus = Some(id);
    }

    /// New size and input shape; the top-left stays (clarification 2). Returns whether the
    /// window exists.
    pub fn update(&mut self, id: WindowId, size: Size, input: Vec<Rect>) -> bool {
        match self.stack.iter_mut().find(|w| w.id == id) {
            Some(w) => {
                w.size = size;
                w.input = input;
                true
            }
            None => false,
        }
    }

    /// Removes a window; focus moves to the topmost remaining window (FR-014). Returns whether
    /// it existed.
    pub fn unmap(&mut self, id: WindowId) -> bool {
        let before = self.stack.len();
        self.stack.retain(|w| w.id != id);
        if self.focus == Some(id) {
            self.focus = self.stack.last().map(|w| w.id);
        }
        self.stack.len() != before
    }

    /// Moves a window to the top and focuses it (FR-009).
    pub fn raise_and_focus(&mut self, id: WindowId) {
        if let Some(i) = self.stack.iter().position(|w| w.id == id) {
            let w = self.stack.remove(i);
            self.stack.push(w);
            self.focus = Some(id);
        }
    }

    pub fn focus(&self) -> Option<WindowId> {
        self.focus
    }

    pub fn get(&self, id: WindowId) -> Option<&Window> {
        self.stack.iter().find(|w| w.id == id)
    }

    /// Windows back to front.
    pub fn iter(&self) -> impl Iterator<Item = &Window> {
        self.stack.iter()
    }

    /// The topmost window accepting input at canvas point `p`, and `p` in its coordinates.
    pub fn hit(&self, p: Point) -> Option<(WindowId, Point)> {
        self.stack.iter().rev().find_map(|w| {
            let local = Point::new(p.x - w.origin.x, p.y - w.origin.y);
            w.input
                .iter()
                .any(|r| r.contains(local))
                .then_some((w.id, local))
        })
    }

    /// Windows that at least partly intersect `view` (canvas rectangle), back to front;
    /// windows covered by others count (FR-006).
    pub fn visible(&self, view: Rect) -> Vec<WindowId> {
        self.stack
            .iter()
            .filter(|w| w.rect().intersects(&view))
            .map(|w| w.id)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::canvas::{Point, Rect, Size, Vector};

    fn full(size: Size) -> Vec<Rect> {
        vec![Rect::new(Point::new(0.0, 0.0), size)]
    }

    fn id(n: u64) -> WindowId {
        WindowId(n)
    }

    #[test]
    fn map_centres_the_window_on_the_view_and_focuses_it() {
        let mut w = Windows::default();
        let size = Size::new(200.0, 100.0);
        w.map(id(1), size, full(size), Point::new(50.0, -20.0));
        let win = w.get(id(1)).unwrap();
        assert_eq!(win.origin, Point::new(-50.0, -70.0));
        assert_eq!(win.size, size);
        assert_eq!(w.focus(), Some(id(1)));
    }

    #[test]
    fn new_windows_go_on_top() {
        let mut w = Windows::default();
        let size = Size::new(100.0, 100.0);
        w.map(id(1), size, full(size), Point::default());
        w.map(id(2), size, full(size), Point::default());
        let order: Vec<_> = w.iter().map(|win| win.id).collect();
        assert_eq!(order, vec![id(1), id(2)]);
        assert_eq!(w.focus(), Some(id(2)));
    }

    #[test]
    fn update_keeps_the_top_left() {
        let mut w = Windows::default();
        let size = Size::new(100.0, 100.0);
        w.map(id(1), size, full(size), Point::default());
        let before = w.get(id(1)).unwrap().origin;
        let bigger = Size::new(300.0, 250.0);
        assert!(w.update(id(1), bigger, full(bigger)));
        let win = w.get(id(1)).unwrap();
        assert_eq!(win.origin, before);
        assert_eq!(win.size, bigger);
    }

    #[test]
    fn unmap_removes_and_moves_focus_to_the_topmost() {
        let mut w = Windows::default();
        let size = Size::new(100.0, 100.0);
        for n in 1..=3 {
            w.map(id(n), size, full(size), Point::default());
        }
        assert!(w.unmap(id(3)));
        assert!(w.get(id(3)).is_none());
        assert_eq!(w.focus(), Some(id(2)));
        assert!(w.unmap(id(1)));
        assert_eq!(w.focus(), Some(id(2)));
        assert!(w.unmap(id(2)));
        assert_eq!(w.focus(), None);
        assert!(!w.unmap(id(2)));
    }

    #[test]
    fn hit_finds_the_topmost_window_and_local_point() {
        let mut w = Windows::default();
        let size = Size::new(100.0, 100.0);
        w.map(id(1), size, full(size), Point::new(50.0, 50.0));
        w.map(id(2), size, full(size), Point::new(100.0, 100.0));
        // Window 1 covers [0, 100), window 2 covers [50, 150).
        assert_eq!(
            w.hit(Point::new(75.0, 75.0)),
            Some((id(2), Point::new(25.0, 25.0)))
        );
        assert_eq!(
            w.hit(Point::new(10.0, 20.0)),
            Some((id(1), Point::new(10.0, 20.0)))
        );
        assert_eq!(w.hit(Point::new(-1.0, 0.0)), None);
    }

    #[test]
    fn hit_respects_input_rects() {
        let mut w = Windows::default();
        let size = Size::new(100.0, 100.0);
        w.map(id(1), size, full(size), Point::new(50.0, 50.0));
        // Window 2 on top accepts input only in its left half.
        let half = vec![Rect::new(Point::new(0.0, 0.0), Size::new(50.0, 100.0))];
        w.map(id(2), size, half, Point::new(50.0, 50.0));
        assert_eq!(w.hit(Point::new(25.0, 50.0)).map(|h| h.0), Some(id(2)));
        assert_eq!(w.hit(Point::new(75.0, 50.0)).map(|h| h.0), Some(id(1)));
        w.update(id(2), size, Vec::new());
        assert_eq!(w.hit(Point::new(25.0, 50.0)).map(|h| h.0), Some(id(1)));
    }

    #[test]
    fn raise_and_focus() {
        let mut w = Windows::default();
        let size = Size::new(100.0, 100.0);
        w.map(id(1), size, full(size), Point::default());
        w.map(id(2), size, full(size), Point::default());
        w.raise_and_focus(id(1));
        let order: Vec<_> = w.iter().map(|win| win.id).collect();
        assert_eq!(order, vec![id(2), id(1)]);
        assert_eq!(w.focus(), Some(id(1)));
    }

    #[test]
    fn visible_lists_windows_touching_the_view_back_to_front() {
        let mut w = Windows::default();
        let size = Size::new(100.0, 100.0);
        w.map(id(1), size, full(size), Point::new(0.0, 0.0));
        w.map(id(2), size, full(size), Point::new(0.0, 0.0)); // covers window 1 fully
        w.map(id(3), size, full(size), Point::new(1000.0, 0.0));
        w.map(id(4), size, full(size), Point::new(140.0, 0.0)); // partly in view
        let view = Rect::new(Point::new(-100.0, -100.0), Size::new(200.0, 200.0));
        assert_eq!(w.visible(view), vec![id(1), id(2), id(4)]);
    }

    proptest! {
        #[test]
        fn placement_centres_any_window_anywhere(
            cx in -1e6f64..1e6, cy in -1e6f64..1e6,
            ww in 1.0f64..4000.0, wh in 1.0f64..4000.0,
        ) {
            let mut w = Windows::default();
            let size = Size::new(ww, wh);
            w.map(id(1), size, full(size), Point::new(cx, cy));
            let win = w.get(id(1)).unwrap();
            let centre = win.origin + size.to_vector() / 2.0;
            prop_assert!((centre.x - cx).abs() < 1e-6 && (centre.y - cy).abs() < 1e-6);
        }

        #[test]
        fn local_points_are_canvas_minus_origin(
            cx in -1e6f64..1e6, cy in -1e6f64..1e6,
            fx in 0.0f64..1.0, fy in 0.0f64..1.0,
        ) {
            let mut w = Windows::default();
            let size = Size::new(640.0, 480.0);
            w.map(id(1), size, full(size), Point::new(cx, cy));
            let origin = w.get(id(1)).unwrap().origin;
            let p = origin + Vector::new(fx * 639.0, fy * 479.0);
            let (hit, local) = w.hit(p).unwrap();
            prop_assert_eq!(hit, id(1));
            prop_assert!((local.x - fx * 639.0).abs() < 1e-6);
            prop_assert!((local.y - fy * 479.0).abs() < 1e-6);
        }
    }
}
