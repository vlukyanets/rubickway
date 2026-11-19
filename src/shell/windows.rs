//! Windows on the canvas: placement, stacking, focus and hit-testing (data-model.md).
//!
//! Window coordinates are the application's logical pixels relative to the window geometry's
//! top-left; one canvas unit is one such pixel, so `local = canvas point - origin` at any zoom.

use crate::canvas::{Point, Rect, Size};

use super::geometry::{Edges, SizeLimits};
use super::popups::{Popup, PopupId};

/// A window's identity, assigned by the server when a toplevel first shows content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WindowId(pub u64);

/// The application a window belongs to: the server's client number (popups research P6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct AppId(pub u64);

/// Which point of a window stays fixed when the application commits a new size (research W4).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Anchor {
    /// The top-left corner stays (client-windows clarification 2).
    #[default]
    TopLeft,
    /// The `fixed` edges keep their canvas coordinates; `right` and `bottom` are the canvas
    /// coordinates of those edges, used when they are fixed and their opposite is dragged.
    Edges {
        fixed: Edges,
        right: f64,
        bottom: f64,
    },
    /// The centre stays at `point` (maximized and fullscreen windows).
    Centre { point: Point },
}

/// A window's state (fullscreen is tracked by the shell, research W8).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Mode {
    #[default]
    Normal,
    /// Maximized; `saved` is the rect to return to (`None` when maximized before it was
    /// first shown, research W7).
    Maximized { saved: Option<Rect> },
}

/// A window on the canvas.
#[derive(Debug, Clone, PartialEq)]
pub struct Window {
    pub id: WindowId,
    /// Canvas position of the geometry's top-left; changed by moves and anchors.
    pub origin: Point,
    /// Size in canvas units.
    pub size: Size,
    /// Where the window accepts pointer input, in window coordinates.
    pub input: Vec<Rect>,
    pub limits: SizeLimits,
    pub anchor: Anchor,
    pub mode: Mode,
    /// The parent window set by the application (dialogs).
    pub parent: Option<WindowId>,
    /// The application it belongs to.
    pub app: AppId,
    /// Its popups, in creation order (popups research P5).
    pub popups: Vec<Popup>,
}

impl Window {
    /// The window's rectangle on the canvas.
    pub fn rect(&self) -> Rect {
        Rect::new(self.origin, self.size)
    }

    pub fn popup(&self, id: PopupId) -> Option<&Popup> {
        self.popups.iter().find(|p| p.id == id)
    }

    /// Where popup `id`'s window geometry starts, in window coordinates: the positions along its
    /// parent chain (shown, else placed).
    pub fn popup_origin(&self, id: PopupId) -> Option<Point> {
        let (mut x, mut y) = (0.0, 0.0);
        let mut current = Some(id);
        for _ in 0..=self.popups.len() {
            let Some(c) = current else {
                return Some(Point::new(x, y));
            };
            let p = self.popup(c)?;
            let (px, py) = p.shown.as_ref().map_or((p.placed.x, p.placed.y), |s| s.at);
            x += f64::from(px);
            y += f64::from(py);
            current = p.parent;
        }
        None
    }

    /// Where popup `id` will be once its placements apply, in window coordinates: the newest
    /// placements along its parent chain. Placing a child goes from here (popups research P10).
    pub fn placed_origin(&self, id: PopupId) -> Option<Point> {
        let (mut x, mut y) = (0.0, 0.0);
        let mut current = Some(id);
        for _ in 0..=self.popups.len() {
            let Some(c) = current else {
                return Some(Point::new(x, y));
            };
            let p = self.popup(c)?;
            x += f64::from(p.placed.x);
            y += f64::from(p.placed.y);
            current = p.parent;
        }
        None
    }

    /// Each shown popup's rectangle and input shape, in window coordinates.
    fn shown_popups(&self) -> impl Iterator<Item = (Rect, &[Rect])> {
        self.popups.iter().filter_map(|p| {
            let shown = p.shown.as_ref()?;
            let origin = self.popup_origin(p.id)?;
            Some((Rect::new(origin, shown.size), shown.input.as_slice()))
        })
    }

    /// The window's rectangle together with its shown popups' (canvas).
    pub fn extent(&self) -> Rect {
        let v = self.origin - Point::new(0.0, 0.0);
        let mut rects = vec![self.rect()];
        rects.extend(self.shown_popups().map(|(r, _)| r.translate(v)));
        Rect::bounding(&rects).unwrap_or_else(|| self.rect())
    }

    /// Whether the window or one of its shown popups accepts input at window point `local`.
    fn accepts(&self, local: Point) -> bool {
        self.input.iter().any(|r| r.contains(local))
            || self.shown_popups().any(|(rect, input)| {
                let inside = Point::new(local.x - rect.origin.x, local.y - rect.origin.y);
                input.iter().any(|r| r.contains(inside))
            })
    }
}

/// All windows, back to front, keyboard focus and the focus history (research W9).
#[derive(Debug, Clone, Default)]
pub struct Windows {
    stack: Vec<Window>,
    focus: Option<WindowId>,
    /// Every window once, most recently focused first.
    mru: Vec<WindowId>,
    /// While cycling: the index in `mru` reached so far.
    cycle: Option<usize>,
}

impl Windows {
    /// Adds a window on top, centred on `view_center`, and focuses it (FR-004, FR-008).
    pub fn map(&mut self, id: WindowId, size: Size, input: Vec<Rect>, view_center: Point) {
        self.stack.retain(|w| w.id != id);
        self.mru.retain(|w| *w != id);
        self.mru.insert(0, id);
        self.cycle = None;
        let origin = view_center - size.to_vector() / 2.0;
        self.stack.push(Window {
            id,
            origin,
            size,
            input,
            limits: SizeLimits::default(),
            anchor: Anchor::TopLeft,
            mode: Mode::Normal,
            parent: None,
            app: AppId::default(),
            popups: Vec::new(),
        });
        self.focus = Some(id);
    }

    /// New size and input shape; the origin follows the window's anchor. Returns whether the
    /// window exists.
    pub fn update(&mut self, id: WindowId, size: Size, input: Vec<Rect>) -> bool {
        match self.get_mut(id) {
            Some(w) => {
                match w.anchor {
                    Anchor::TopLeft => {}
                    Anchor::Edges {
                        fixed,
                        right,
                        bottom,
                    } => {
                        if fixed.right && !fixed.left {
                            w.origin.x = right - size.w;
                        }
                        if fixed.bottom && !fixed.top {
                            w.origin.y = bottom - size.h;
                        }
                    }
                    Anchor::Centre { point } => {
                        w.origin = point - size.to_vector() / 2.0;
                    }
                }
                w.size = size;
                w.input = input;
                true
            }
            None => false,
        }
    }

    /// Moves a window's top-left to `origin`. Returns whether the window exists.
    pub fn move_to(&mut self, id: WindowId, origin: Point) -> bool {
        match self.get_mut(id) {
            Some(w) => {
                w.origin = origin;
                true
            }
            None => false,
        }
    }

    pub fn set_anchor(&mut self, id: WindowId, anchor: Anchor) {
        if let Some(w) = self.get_mut(id) {
            w.anchor = anchor;
        }
    }

    /// The application applied the configure that ended a resize: later size changes keep
    /// the top-left again (US2/AC4, research W4).
    pub fn resize_settled(&mut self, id: WindowId) {
        if let Some(w) = self.get_mut(id)
            && matches!(w.anchor, Anchor::Edges { .. })
        {
            w.anchor = Anchor::TopLeft;
        }
    }

    pub fn set_mode(&mut self, id: WindowId, mode: Mode) {
        if let Some(w) = self.get_mut(id) {
            w.mode = mode;
        }
    }

    /// Adds a window on top without focus, centred on `center`, second in the focus history
    /// (a window opening while another is fullscreen, research W14).
    pub fn insert_unfocused(&mut self, id: WindowId, size: Size, input: Vec<Rect>, center: Point) {
        let focus = self.focus;
        let mru = self.mru.clone();
        self.map(id, size, input, center);
        self.focus = focus.filter(|f| *f != id);
        self.mru = mru;
        self.mru.retain(|w| *w != id);
        let at = self.mru.len().min(1);
        self.mru.insert(at, id);
    }

    /// `p` (canvas) in `id`'s coordinates if `id` (or one of its popups) accepts input there.
    pub fn hit_window(&self, id: WindowId, p: Point) -> Option<Point> {
        let w = self.get(id)?;
        let local = Point::new(p.x - w.origin.x, p.y - w.origin.y);
        w.accepts(local).then_some(local)
    }

    /// Adds a popup to its window.
    pub fn add_popup(&mut self, window: WindowId, popup: Popup) {
        if let Some(w) = self.get_mut(window) {
            w.popups.push(popup);
        }
    }

    pub fn popup_mut(&mut self, window: WindowId, id: PopupId) -> Option<&mut Popup> {
        self.get_mut(window)?.popups.iter_mut().find(|p| p.id == id)
    }

    /// The window popup `id` belongs to.
    pub fn window_of_popup(&self, id: PopupId) -> Option<WindowId> {
        self.stack
            .iter()
            .find(|w| w.popup(id).is_some())
            .map(|w| w.id)
    }

    /// Removes popup `id` and the popups above it; returns their ids, the topmost first.
    pub fn remove_popup(&mut self, window: WindowId, id: PopupId) -> Vec<PopupId> {
        let Some(w) = self.get_mut(window) else {
            return Vec::new();
        };
        let mut gone = vec![id];
        let mut i = 0;
        while i < gone.len() {
            let parent = gone[i];
            gone.extend(
                w.popups
                    .iter()
                    .filter(|p| p.parent == Some(parent))
                    .map(|p| p.id),
            );
            i += 1;
        }
        gone.retain(|g| w.popup(*g).is_some());
        w.popups.retain(|p| !gone.contains(&p.id));
        gone.sort_unstable_by(|a, b| b.cmp(a));
        gone
    }

    /// Removes all popups of `window`; returns their ids, the topmost first.
    pub fn take_popups(&mut self, window: WindowId) -> Vec<PopupId> {
        let Some(w) = self.get_mut(window) else {
            return Vec::new();
        };
        let mut gone: Vec<PopupId> = w.popups.drain(..).map(|p| p.id).collect();
        gone.sort_unstable_by(|a, b| b.cmp(a));
        gone
    }

    pub fn set_limits(&mut self, id: WindowId, limits: SizeLimits) {
        if let Some(w) = self.get_mut(id) {
            w.limits = limits;
        }
    }

    pub fn set_parent(&mut self, id: WindowId, parent: Option<WindowId>) {
        if let Some(w) = self.get_mut(id) {
            w.parent = parent;
        }
    }

    pub fn set_app(&mut self, id: WindowId, app: AppId) {
        if let Some(w) = self.get_mut(id) {
            w.app = app;
        }
    }

    fn get_mut(&mut self, id: WindowId) -> Option<&mut Window> {
        self.stack.iter_mut().find(|w| w.id == id)
    }

    /// Removes a window; focus moves to the topmost remaining window (FR-014). Returns whether
    /// it existed.
    pub fn unmap(&mut self, id: WindowId) -> bool {
        let before = self.stack.len();
        self.stack.retain(|w| w.id != id);
        self.mru.retain(|w| *w != id);
        self.cycle = None;
        if self.focus == Some(id) {
            self.focus = self.stack.last().map(|w| w.id);
            if let Some(f) = self.focus {
                self.mark_used(f);
            }
        }
        self.stack.len() != before
    }

    /// Moves a window to the top and focuses it (FR-009); it becomes the most recently used.
    pub fn raise_and_focus(&mut self, id: WindowId) {
        if self.raise(id) {
            self.focus = Some(id);
            self.cycle = None;
            self.mark_used(id);
        }
    }

    /// The focus history, most recently focused first.
    pub fn mru(&self) -> &[WindowId] {
        &self.mru
    }

    /// One focus-cycling step (`step` 1: next, -1: previous) from the window reached so far
    /// (the front of the history when a cycle starts): focuses and raises that window without
    /// reordering the history (FR-011). `None` without windows.
    pub fn cycle(&mut self, step: i32) -> Option<WindowId> {
        let len = i64::try_from(self.mru.len()).ok().filter(|n| *n > 0)?;
        let from = i64::try_from(self.cycle.unwrap_or(0)).ok()?;
        let index = usize::try_from((from + i64::from(step)).rem_euclid(len)).ok()?;
        let id = *self.mru.get(index)?;
        self.cycle = Some(index);
        self.raise(id);
        self.focus = Some(id);
        Some(id)
    }

    /// Ends a cycle: the window reached becomes the most recently used.
    pub fn end_cycle(&mut self) {
        if let Some(index) = self.cycle.take()
            && let Some(id) = self.mru.get(index).copied()
        {
            self.mark_used(id);
        }
    }

    /// Moves a window to the top of the stack; returns whether it exists.
    fn raise(&mut self, id: WindowId) -> bool {
        match self.stack.iter().position(|w| w.id == id) {
            Some(i) => {
                let w = self.stack.remove(i);
                self.stack.push(w);
                true
            }
            None => false,
        }
    }

    /// Moves `id` to the front of the focus history.
    fn mark_used(&mut self, id: WindowId) {
        self.mru.retain(|w| *w != id);
        self.mru.insert(0, id);
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

    /// The topmost window accepting input at canvas point `p` (with its popups), and `p` in its
    /// coordinates.
    pub fn hit(&self, p: Point) -> Option<(WindowId, Point)> {
        self.stack.iter().rev().find_map(|w| {
            let local = Point::new(p.x - w.origin.x, p.y - w.origin.y);
            w.accepts(local).then_some((w.id, local))
        })
    }

    /// Windows that at least partly intersect `view` (canvas rectangle) with their popups, back
    /// to front; windows covered by others count (FR-006).
    pub fn visible(&self, view: Rect) -> Vec<WindowId> {
        self.stack
            .iter()
            .filter(|w| w.extent().intersects(&view))
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

    #[test]
    fn new_windows_have_default_window_management_state() {
        let mut w = Windows::default();
        let size = Size::new(100.0, 100.0);
        w.map(id(1), size, full(size), Point::default());
        let win = w.get(id(1)).unwrap();
        assert_eq!(win.anchor, Anchor::TopLeft);
        assert_eq!(win.mode, Mode::Normal);
        assert_eq!(win.limits, SizeLimits::default());
        assert_eq!(win.parent, None);
    }

    #[test]
    fn limits_and_parent_are_stored() {
        let mut w = Windows::default();
        let size = Size::new(100.0, 100.0);
        w.map(id(1), size, full(size), Point::default());
        w.map(id(2), size, full(size), Point::default());
        let limits = SizeLimits {
            min: Size::new(10.0, 20.0),
            max: Size::new(0.0, 500.0),
        };
        w.set_limits(id(2), limits);
        w.set_parent(id(2), Some(id(1)));
        assert_eq!(w.get(id(2)).unwrap().limits, limits);
        assert_eq!(w.get(id(2)).unwrap().parent, Some(id(1)));
    }

    #[test]
    fn move_to_moves_a_window() {
        let mut w = Windows::default();
        let size = Size::new(100.0, 100.0);
        w.map(id(1), size, full(size), Point::default());
        assert!(w.move_to(id(1), Point::new(300.0, -40.0)));
        assert_eq!(w.get(id(1)).unwrap().origin, Point::new(300.0, -40.0));
        assert!(!w.move_to(id(9), Point::default()));
    }

    #[test]
    fn edges_anchor_keeps_the_far_edges_when_left_and_top_are_dragged() {
        let mut w = Windows::default();
        let size = Size::new(100.0, 100.0);
        w.map(id(1), size, full(size), Point::new(50.0, 50.0)); // origin (0, 0)
        // Dragging the top-left corner: right and bottom stay fixed at 100.
        w.set_anchor(
            id(1),
            Anchor::Edges {
                fixed: Edges {
                    top: false,
                    bottom: true,
                    left: false,
                    right: true,
                },
                right: 100.0,
                bottom: 100.0,
            },
        );
        let bigger = Size::new(160.0, 130.0);
        w.update(id(1), bigger, full(bigger));
        let win = w.get(id(1)).unwrap();
        assert_eq!(win.origin, Point::new(-60.0, -30.0));
        assert_eq!(win.rect().end(), Point::new(100.0, 100.0));
    }

    #[test]
    fn edges_anchor_keeps_the_origin_when_right_and_bottom_are_dragged() {
        let mut w = Windows::default();
        let size = Size::new(100.0, 100.0);
        w.map(id(1), size, full(size), Point::new(50.0, 50.0));
        w.set_anchor(
            id(1),
            Anchor::Edges {
                fixed: Edges {
                    top: true,
                    bottom: false,
                    left: true,
                    right: false,
                },
                right: 100.0,
                bottom: 100.0,
            },
        );
        let bigger = Size::new(160.0, 130.0);
        w.update(id(1), bigger, full(bigger));
        assert_eq!(w.get(id(1)).unwrap().origin, Point::new(0.0, 0.0));
    }

    #[test]
    fn centre_anchor_keeps_the_centre() {
        let mut w = Windows::default();
        let size = Size::new(100.0, 100.0);
        w.map(id(1), size, full(size), Point::default());
        w.set_anchor(
            id(1),
            Anchor::Centre {
                point: Point::new(500.0, 400.0),
            },
        );
        let bigger = Size::new(300.0, 200.0);
        w.update(id(1), bigger, full(bigger));
        assert_eq!(w.get(id(1)).unwrap().origin, Point::new(350.0, 300.0));
    }

    /// The anchor that keeps the edges opposite to `dragged` (research W4).
    fn opposite_anchor(dragged: Edges, rect: Rect) -> Anchor {
        Anchor::Edges {
            fixed: Edges {
                top: dragged.bottom,
                bottom: dragged.top,
                left: dragged.right,
                right: dragged.left,
            },
            right: rect.end().x,
            bottom: rect.end().y,
        }
    }

    #[test]
    fn resize_settled_returns_to_top_left() {
        let mut w = Windows::default();
        let size = Size::new(100.0, 100.0);
        w.map(id(1), size, full(size), Point::new(50.0, 50.0));
        let rect = w.get(id(1)).unwrap().rect();
        let dragged = Edges {
            top: true,
            bottom: false,
            left: true,
            right: false,
        };
        w.set_anchor(id(1), opposite_anchor(dragged, rect));
        let bigger = Size::new(150.0, 150.0);
        w.update(id(1), bigger, full(bigger));
        let origin = w.get(id(1)).unwrap().origin;
        assert_eq!(origin, Point::new(-50.0, -50.0));
        w.resize_settled(id(1));
        assert_eq!(w.get(id(1)).unwrap().anchor, Anchor::TopLeft);
        let smaller = Size::new(80.0, 80.0);
        w.update(id(1), smaller, full(smaller));
        assert_eq!(w.get(id(1)).unwrap().origin, origin);
    }

    fn three() -> Windows {
        let mut w = Windows::default();
        let size = Size::new(100.0, 100.0);
        for n in 1..=3 {
            w.map(id(n), size, full(size), Point::default());
        }
        w
    }

    #[test]
    fn the_focus_history_puts_the_latest_focus_first() {
        let mut w = three();
        assert_eq!(w.mru(), &[id(3), id(2), id(1)]);
        w.raise_and_focus(id(1));
        assert_eq!(w.mru(), &[id(1), id(3), id(2)]);
        w.unmap(id(3));
        assert_eq!(w.mru(), &[id(1), id(2)]);
    }

    #[test]
    fn cycling_walks_the_history_without_reordering_until_it_ends() {
        let mut w = three();
        assert_eq!(w.cycle(1), Some(id(2)));
        assert_eq!(w.focus(), Some(id(2)));
        assert_eq!(w.iter().last().map(|win| win.id), Some(id(2)), "raised");
        assert_eq!(w.mru(), &[id(3), id(2), id(1)]);
        assert_eq!(w.cycle(1), Some(id(1)));
        assert_eq!(w.cycle(1), Some(id(3)), "wraps around");
        assert_eq!(w.cycle(-1), Some(id(1)));
        w.end_cycle();
        assert_eq!(w.mru(), &[id(1), id(3), id(2)]);
        // A new cycle starts from the front: one step goes back to the previous window.
        assert_eq!(w.cycle(1), Some(id(3)));
        w.end_cycle();
        assert_eq!(w.mru(), &[id(3), id(1), id(2)]);
    }

    #[test]
    fn cycling_backwards_starts_at_the_oldest() {
        let mut w = three();
        assert_eq!(w.cycle(-1), Some(id(1)));
        w.end_cycle();
        assert_eq!(w.mru(), &[id(1), id(3), id(2)]);
    }

    #[test]
    fn cycling_with_no_windows_does_nothing() {
        let mut w = Windows::default();
        assert_eq!(w.cycle(1), None);
        w.end_cycle();
        assert_eq!(w.focus(), None);
    }

    proptest! {
        #[test]
        fn the_history_is_a_permutation_of_the_stack(
            ops in prop::collection::vec((0u8..5, 1u64..6), 0..40),
        ) {
            let mut w = Windows::default();
            let size = Size::new(10.0, 10.0);
            for (op, n) in ops {
                match op {
                    0 => w.map(id(n), size, full(size), Point::default()),
                    1 => { w.unmap(id(n)); }
                    2 => w.raise_and_focus(id(n)),
                    3 => { w.cycle(if n % 2 == 0 { 1 } else { -1 }); }
                    _ => w.end_cycle(),
                }
                let mut stack: Vec<_> = w.iter().map(|win| win.id).collect();
                let mut mru = w.mru().to_vec();
                stack.sort();
                mru.sort();
                prop_assert_eq!(stack, mru);
            }
        }

        #[test]
        fn the_opposite_edges_stay_whatever_size_arrives(
            ox in -1e5f64..1e5, oy in -1e5f64..1e5,
            w0 in 1.0f64..4000.0, h0 in 1.0f64..4000.0,
            edge in prop::sample::select(vec![1u32, 2, 4, 5, 6, 8, 9, 10]),
            sizes in prop::collection::vec((1.0f64..8000.0, 1.0f64..8000.0), 1..8),
        ) {
            let mut w = Windows::default();
            let size = Size::new(w0, h0);
            w.map(id(1), size, full(size), Point::new(ox + w0 / 2.0, oy + h0 / 2.0));
            let start = w.get(id(1)).unwrap().rect();
            let dragged = Edges::from_resize_edge(edge).unwrap();
            w.set_anchor(id(1), opposite_anchor(dragged, start));
            for (sw, sh) in sizes {
                let s = Size::new(sw, sh);
                w.update(id(1), s, full(s));
                let r = w.get(id(1)).unwrap().rect();
                if !dragged.left {
                    prop_assert!((r.origin.x - start.origin.x).abs() <= 1.0);
                } else {
                    prop_assert!((r.end().x - start.end().x).abs() <= 1.0);
                }
                if !dragged.top {
                    prop_assert!((r.origin.y - start.origin.y).abs() <= 1.0);
                } else {
                    prop_assert!((r.end().y - start.end().y).abs() <= 1.0);
                }
            }
        }

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

    // --- popups (popups tasks T013) ---------------------------------------------------------

    use crate::shell::popups::Shown;
    use crate::shell::positioner::{IRect, PopupRules};

    /// A shown `w x h` popup at `at` relative to its parent.
    fn popup(n: u64, parent: Option<u64>, at: (i32, i32), w: f64, h: f64) -> Popup {
        Popup {
            id: PopupId(n),
            parent: parent.map(PopupId),
            rules: PopupRules::default(),
            grab: false,
            placed: IRect::new(at.0, at.1, w as i32, h as i32),
            shown: Some(Shown {
                at,
                size: Size::new(w, h),
                input: full(Size::new(w, h)),
            }),
        }
    }

    /// Window 1, 100 x 100 at canvas (0, 0).
    fn one_window() -> Windows {
        let mut w = Windows::default();
        let size = Size::new(100.0, 100.0);
        w.map(id(1), size, full(size), Point::new(50.0, 50.0));
        w
    }

    #[test]
    fn the_extent_covers_the_window_and_its_shown_popups() {
        let mut w = one_window();
        let rect = w.get(id(1)).unwrap().rect();
        assert_eq!(w.get(id(1)).unwrap().extent(), rect);
        // A menu sticking out to the right, and a submenu of it further right.
        w.add_popup(id(1), popup(1, None, (90, 10), 50.0, 30.0));
        w.add_popup(id(1), popup(2, Some(1), (50, 0), 40.0, 20.0));
        assert_eq!(
            w.get(id(1)).unwrap().extent(),
            Rect::new(Point::new(0.0, 0.0), Size::new(180.0, 100.0))
        );
        // A popup not shown yet does not count.
        let mut hidden = popup(3, None, (0, 200), 10.0, 10.0);
        hidden.shown = None;
        w.add_popup(id(1), hidden);
        assert_eq!(
            w.get(id(1)).unwrap().extent(),
            Rect::new(Point::new(0.0, 0.0), Size::new(180.0, 100.0))
        );
    }

    #[test]
    fn a_window_whose_popup_alone_is_in_view_is_visible() {
        let mut w = one_window();
        let view = Rect::new(Point::new(120.0, 0.0), Size::new(100.0, 100.0));
        assert!(w.visible(view).is_empty());
        w.add_popup(id(1), popup(1, None, (90, 10), 50.0, 30.0));
        assert_eq!(w.visible(view), vec![id(1)]);
    }

    #[test]
    fn a_popup_outside_the_window_is_hit_as_its_window() {
        let mut w = one_window();
        w.add_popup(id(1), popup(1, None, (90, 10), 50.0, 30.0));
        assert_eq!(
            w.hit(Point::new(130.0, 20.0)),
            Some((id(1), Point::new(130.0, 20.0)))
        );
        assert_eq!(
            w.hit_window(id(1), Point::new(130.0, 20.0)),
            Some(Point::new(130.0, 20.0))
        );
        assert_eq!(w.hit(Point::new(130.0, 50.0)), None);
    }

    #[test]
    fn a_popup_accepts_input_over_a_window_part_that_does_not() {
        let mut w = Windows::default();
        let size = Size::new(100.0, 100.0);
        // The window accepts input only in its left half.
        w.map(
            id(1),
            size,
            vec![Rect::new(Point::new(0.0, 0.0), Size::new(50.0, 100.0))],
            Point::new(50.0, 50.0),
        );
        assert_eq!(w.hit(Point::new(70.0, 20.0)), None);
        w.add_popup(id(1), popup(1, None, (60, 10), 30.0, 30.0));
        assert_eq!(
            w.hit(Point::new(70.0, 20.0)),
            Some((id(1), Point::new(70.0, 20.0)))
        );
    }

    #[test]
    fn a_higher_window_covers_a_lower_windows_popup() {
        let mut w = one_window();
        w.add_popup(id(1), popup(1, None, (90, 10), 50.0, 30.0));
        let size = Size::new(100.0, 100.0);
        // Window 2 on top, covering x 100..200.
        w.map(id(2), size, full(size), Point::new(150.0, 50.0));
        assert_eq!(
            w.hit(Point::new(130.0, 20.0)),
            Some((id(2), Point::new(30.0, 20.0)))
        );
        // Window 1's own part of the popup, over window 1, is still window 1.
        assert_eq!(
            w.hit(Point::new(95.0, 20.0)),
            Some((id(1), Point::new(95.0, 20.0)))
        );
    }

    #[test]
    fn removing_a_popup_removes_the_popups_above_it_topmost_first() {
        let mut w = one_window();
        w.add_popup(id(1), popup(1, None, (0, 0), 10.0, 10.0));
        w.add_popup(id(1), popup(2, Some(1), (0, 0), 10.0, 10.0));
        w.add_popup(id(1), popup(3, Some(2), (0, 0), 10.0, 10.0));
        w.add_popup(id(1), popup(4, None, (0, 0), 10.0, 10.0));
        assert_eq!(w.window_of_popup(PopupId(3)), Some(id(1)));
        assert_eq!(
            w.remove_popup(id(1), PopupId(2)),
            vec![PopupId(3), PopupId(2)]
        );
        assert_eq!(w.take_popups(id(1)), vec![PopupId(4), PopupId(1)]);
        assert!(w.get(id(1)).unwrap().popups.is_empty());
    }
}
