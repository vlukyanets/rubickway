//! The row of displays: where each viewer sits, where the pointer is, and each viewer's home
//! view (research S9, S14). Plain data, no device types.
//!
//! Layout positions are logical pixels with the origin at the top-left corner of the first
//! entry. Entries sit side by side with their top edges at `y = 0`.

use super::{Camera, Point, Size, Vector};

/// Identifies a viewer: the backend's output id, never reused while RubickWay runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ViewerId(pub u32);

/// One display in the row.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutEntry {
    pub id: ViewerId,
    /// Logical size (the display's mode at scale 1).
    pub size: Size,
    /// A built-in panel: placed before every external display (FR-018).
    pub internal: bool,
    /// Increases with every connection; orders external displays.
    pub connected_seq: u64,
}

/// How far inside an entry's far edge a clamped pointer stays, so it is still on the entry
/// (rectangles are half-open).
const INSIDE: f64 = 1e-6;

/// The row of displays, the pointer position and every display's home view.
#[derive(Debug, Clone, Default)]
pub struct Layout {
    /// In row order.
    entries: Vec<LayoutEntry>,
    pointer: Point,
    homes: Vec<(ViewerId, Camera)>,
}

impl Layout {
    /// The entries in row order.
    pub fn entries(&self) -> &[LayoutEntry] {
        &self.entries
    }

    /// The pointer position in layout coordinates.
    pub fn pointer(&self) -> Point {
        self.pointer
    }

    /// The top-left corner of entry `id`.
    pub fn origin(&self, id: ViewerId) -> Option<Point> {
        let mut x = 0.0;
        for e in &self.entries {
            if e.id == id {
                return Some(Point::new(x, 0.0));
            }
            x += e.size.w;
        }
        None
    }

    /// The home view of entry `id` (FR-020).
    pub fn home(&self, id: ViewerId) -> Option<Camera> {
        self.homes.iter().find(|(h, _)| *h == id).map(|(_, c)| *c)
    }

    /// Adds a display (an existing id is replaced). Its home view continues the rightmost
    /// display's home view; the first display's home is the canvas origin. The pointer keeps
    /// its position on the display it is on, or goes to the centre of the first display added.
    pub fn add(&mut self, entry: LayoutEntry) {
        let kept = self.locate(self.pointer);
        self.remove_entry(entry.id);
        let home = match self.rightmost_home() {
            Some((camera, size)) => Camera {
                center: Point::new(
                    camera.center.x + size.w / 2.0 + entry.size.w / 2.0,
                    camera.center.y - size.h / 2.0 + entry.size.h / 2.0,
                ),
                zoom: 1.0,
            },
            None => Camera::default(),
        };
        self.homes.push((entry.id, home));
        let at = self
            .entries
            .iter()
            .position(|e| order_key(&entry) < order_key(e))
            .unwrap_or(self.entries.len());
        self.entries.insert(at, entry);
        self.restore_pointer(kept);
    }

    /// Removes a display. A pointer on it moves to the centre of the first display; a pointer
    /// on another display keeps its position there (FR-021).
    pub fn remove(&mut self, id: ViewerId) {
        let kept = self.locate(self.pointer).filter(|(on, _)| *on != id);
        self.remove_entry(id);
        self.restore_pointer(kept);
    }

    /// Gives every display the home view of the whole row centred on the canvas origin by its
    /// bounding box (FR-020), and puts the pointer in the centre of the leftmost display
    /// (FR-009); used once the displays present at startup are known.
    pub fn recentre_homes(&mut self) {
        self.restore_pointer(None);
        let width: f64 = self.entries.iter().map(|e| e.size.w).sum();
        let height = self.entries.iter().map(|e| e.size.h).fold(0.0, f64::max);
        let mut x = -width / 2.0;
        let top = -height / 2.0;
        self.homes = self
            .entries
            .iter()
            .map(|e| {
                let center = Point::new(x + e.size.w / 2.0, top + e.size.h / 2.0);
                x += e.size.w;
                (e.id, Camera { center, zoom: 1.0 })
            })
            .collect();
    }

    /// Moves the pointer by `delta`: x is clamped to the row, the display whose horizontal
    /// range contains it is the one the pointer is on, and y is clamped to that display's
    /// height (so crossing to a shorter display clamps the height, FR-018).
    pub fn move_by(&mut self, delta: Vector) {
        self.pointer = self.clamp(self.pointer + delta);
    }

    /// Puts the pointer at `point`, clamped like [`Layout::move_by`] (for absolute positions).
    pub fn move_to(&mut self, point: Point) {
        self.pointer = self.clamp(point);
    }

    /// The display containing `point`, and the point relative to its top-left corner.
    pub fn locate(&self, point: Point) -> Option<(ViewerId, Point)> {
        let mut x = 0.0;
        for e in &self.entries {
            let rel = Point::new(point.x - x, point.y);
            if rel.x >= 0.0 && rel.x < e.size.w && rel.y >= 0.0 && rel.y < e.size.h {
                return Some((e.id, rel));
            }
            x += e.size.w;
        }
        None
    }

    /// `point` relative to the top-left corner of display `id`, also outside it.
    pub fn relative_to(&self, id: ViewerId, point: Point) -> Option<Point> {
        self.origin(id)
            .map(|o| Point::new(point.x - o.x, point.y - o.y))
    }

    fn remove_entry(&mut self, id: ViewerId) {
        self.entries.retain(|e| e.id != id);
        self.homes.retain(|(h, _)| *h != id);
    }

    /// The home view and size of the rightmost display that has a home.
    fn rightmost_home(&self) -> Option<(Camera, Size)> {
        self.entries
            .iter()
            .rev()
            .find_map(|e| self.home(e.id).map(|c| (c, e.size)))
    }

    /// Puts the pointer back at `kept` (a display and a position on it), or at the centre of
    /// the first display.
    fn restore_pointer(&mut self, kept: Option<(ViewerId, Point)>) {
        let back = kept.and_then(|(id, rel)| {
            self.origin(id)
                .map(|o| Point::new(o.x + rel.x, o.y + rel.y))
        });
        self.pointer = match (back, self.entries.first()) {
            (Some(p), _) => self.clamp(p),
            (None, Some(first)) => first.size.center(),
            (None, None) => Point::default(),
        };
    }

    fn clamp(&self, p: Point) -> Point {
        let width: f64 = self.entries.iter().map(|e| e.size.w).sum();
        let Some(last) = self.entries.last() else {
            return p;
        };
        let x = p.x.clamp(0.0, (width - INSIDE).max(0.0));
        // The entry whose horizontal range contains x; the last one if rounding put x past it.
        let mut left = 0.0;
        let mut height = last.size.h;
        for e in &self.entries {
            if x < left + e.size.w {
                height = e.size.h;
                break;
            }
            left += e.size.w;
        }
        Point::new(x, p.y.clamp(0.0, (height - INSIDE).max(0.0)))
    }
}

/// Row order: internal displays first, then by connection.
fn order_key(e: &LayoutEntry) -> (bool, u64) {
    (!e.internal, e.connected_seq)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::{Camera, Point, Size, Vector};
    use proptest::prelude::*;

    const LAPTOP: Size = Size {
        w: 1920.0,
        h: 1080.0,
    };
    const MONITOR: Size = Size {
        w: 2560.0,
        h: 1440.0,
    };

    fn entry(id: u32, size: Size, internal: bool, seq: u64) -> LayoutEntry {
        LayoutEntry {
            id: ViewerId(id),
            size,
            internal,
            connected_seq: seq,
        }
    }

    fn two() -> Layout {
        let mut l = Layout::default();
        l.add(entry(1, LAPTOP, true, 0));
        l.add(entry(2, MONITOR, false, 1));
        l
    }

    #[test]
    fn internal_entries_come_first_then_connection_order() {
        let mut l = Layout::default();
        l.add(entry(5, MONITOR, false, 3));
        l.add(entry(7, MONITOR, false, 1));
        l.add(entry(9, LAPTOP, true, 9));
        let ids: Vec<u32> = l.entries().iter().map(|e| e.id.0).collect();
        assert_eq!(ids, vec![9, 7, 5]);
        assert_eq!(l.origin(ViewerId(9)), Some(Point::new(0.0, 0.0)));
        assert_eq!(l.origin(ViewerId(7)), Some(Point::new(1920.0, 0.0)));
        assert_eq!(l.origin(ViewerId(5)), Some(Point::new(4480.0, 0.0)));
    }

    #[test]
    fn the_first_entry_puts_the_pointer_at_its_centre() {
        let mut l = Layout::default();
        assert_eq!(l.locate(l.pointer()), None);
        l.add(entry(1, LAPTOP, true, 0));
        assert_eq!(l.pointer(), Point::new(960.0, 540.0));
        assert_eq!(
            l.locate(l.pointer()),
            Some((ViewerId(1), Point::new(960.0, 540.0)))
        );
    }

    #[test]
    fn motion_stays_inside_one_entry_and_stops_at_outer_edges() {
        let mut l = Layout::default();
        l.add(entry(1, LAPTOP, true, 0));
        l.move_by(Vector::new(-5000.0, -5000.0));
        assert_eq!(l.pointer(), Point::new(0.0, 0.0));
        l.move_by(Vector::new(5000.0, 5000.0));
        let (id, p) = l.locate(l.pointer()).expect("inside");
        assert_eq!(id, ViewerId(1));
        assert!(p.x < 1920.0 && p.x > 1919.0, "{p:?}");
        assert!(p.y < 1080.0 && p.y > 1079.0, "{p:?}");
    }

    #[test]
    fn motion_crosses_a_shared_edge() {
        let mut l = two();
        // From the laptop centre to the right, 100 px past its right edge.
        l.move_by(Vector::new(1060.0, 0.0));
        assert_eq!(
            l.locate(l.pointer()),
            Some((ViewerId(2), Point::new(100.0, 540.0)))
        );
    }

    #[test]
    fn crossing_to_a_shorter_entry_clamps_the_height() {
        let mut l = Layout::default();
        l.add(entry(1, MONITOR, true, 0));
        l.add(entry(2, LAPTOP, false, 1));
        // Bottom of the monitor (y 1400), then to the right, 100 px past the shared edge, at a
        // height the laptop does not have.
        l.move_by(Vector::new(0.0, 680.0));
        l.move_by(Vector::new(1380.0, 0.0));
        let (id, p) = l.locate(l.pointer()).expect("inside");
        assert_eq!(id, ViewerId(2));
        assert!((p.x - 100.0).abs() < 1e-6, "{p:?}");
        assert!(p.y < 1080.0 && p.y > 1079.0, "{p:?}");
    }

    #[test]
    fn relative_to_works_outside_the_entry() {
        let l = two();
        assert_eq!(
            l.relative_to(ViewerId(2), Point::new(100.0, 50.0)),
            Some(Point::new(-1820.0, 50.0))
        );
        assert_eq!(l.relative_to(ViewerId(3), Point::new(0.0, 0.0)), None);
    }

    #[test]
    fn removing_the_pointers_entry_moves_it_to_the_first_centre() {
        let mut l = two();
        l.move_by(Vector::new(2000.0, 0.0));
        assert_eq!(l.locate(l.pointer()).map(|(id, _)| id), Some(ViewerId(2)));
        l.remove(ViewerId(2));
        assert_eq!(l.pointer(), Point::new(960.0, 540.0));
    }

    #[test]
    fn re_flow_keeps_the_pointer_on_its_entry() {
        let mut l = Layout::default();
        l.add(entry(2, MONITOR, false, 1));
        l.move_by(Vector::new(100.0, 0.0));
        let before = l.locate(l.pointer());
        // An internal display appears and goes first: the monitor moves right.
        l.add(entry(1, LAPTOP, true, 2));
        assert_eq!(l.locate(l.pointer()), before);
        l.remove(ViewerId(1));
        assert_eq!(l.locate(l.pointer()), before);
    }

    #[test]
    fn no_entries_locates_nothing() {
        let mut l = two();
        l.remove(ViewerId(1));
        l.remove(ViewerId(2));
        assert_eq!(l.locate(l.pointer()), None);
        l.move_by(Vector::new(10.0, 10.0));
        assert_eq!(l.locate(l.pointer()), None);
    }

    #[test]
    fn the_pointer_starts_on_the_leftmost_display_whatever_came_first() {
        let mut l = Layout::default();
        // The card lists the external connector before the built-in one.
        l.add(entry(2, MONITOR, false, 1));
        l.add(entry(1, LAPTOP, true, 2));
        assert_eq!(l.locate(l.pointer()).map(|(id, _)| id), Some(ViewerId(2)));
        l.recentre_homes();
        assert_eq!(l.pointer(), Point::new(960.0, 540.0));
        assert_eq!(l.locate(l.pointer()).map(|(id, _)| id), Some(ViewerId(1)));
    }

    #[test]
    fn one_entry_is_home_at_the_origin() {
        let mut l = Layout::default();
        l.add(entry(1, LAPTOP, true, 0));
        l.recentre_homes();
        assert_eq!(l.home(ViewerId(1)), Some(Camera::default()));
    }

    #[test]
    fn a_row_is_centred_on_the_origin_by_its_bounding_box() {
        let mut l = two();
        l.recentre_homes();
        // Box 4480 x 1440, top-left (-2240, -720).
        let laptop = l.home(ViewerId(1)).expect("home");
        let monitor = l.home(ViewerId(2)).expect("home");
        assert_eq!(laptop.zoom, 1.0);
        assert_eq!(laptop.center, Point::new(-2240.0 + 960.0, -720.0 + 540.0));
        assert_eq!(monitor.center, Point::new(-2240.0 + 1920.0 + 1280.0, 0.0));
        // The shorter display's home view sits higher.
        assert!(laptop.center.y < monitor.center.y);
    }

    #[test]
    fn a_later_entry_continues_the_rightmost_home_view() {
        let mut l = Layout::default();
        l.add(entry(1, LAPTOP, true, 0));
        l.recentre_homes();
        // The second home is computed when added, from the first home, whatever the cameras do.
        l.add(entry(2, MONITOR, false, 1));
        let laptop = l.home(ViewerId(1)).expect("home");
        let monitor = l.home(ViewerId(2)).expect("home");
        assert_eq!(laptop, Camera::default());
        // Left edge at x 960, top edge at y -540.
        assert_eq!(monitor.center, Point::new(960.0 + 1280.0, -540.0 + 720.0));
        assert_eq!(monitor.zoom, 1.0);
    }

    #[derive(Debug, Clone)]
    enum Op {
        Add(u32, bool, f64, f64),
        Remove(u32),
        Move(f64, f64),
    }

    fn op() -> impl Strategy<Value = Op> {
        prop_oneof![
            (0u32..6, any::<bool>(), 100.0f64..4000.0, 100.0f64..3000.0)
                .prop_map(|(id, i, w, h)| Op::Add(id, i, w, h)),
            (0u32..6).prop_map(Op::Remove),
            (-5000.0f64..5000.0, -5000.0f64..5000.0).prop_map(|(x, y)| Op::Move(x, y)),
        ]
    }

    proptest! {
        #[test]
        fn the_pointer_is_always_on_an_entry(ops in prop::collection::vec(op(), 1..40)) {
            let mut l = Layout::default();
            for (seq, op) in ops.into_iter().enumerate() {
                match op {
                    Op::Add(id, internal, w, h) => {
                        if l.origin(ViewerId(id)).is_none() {
                            l.add(entry(id, Size::new(w, h), internal, seq as u64));
                        }
                    }
                    Op::Remove(id) => l.remove(ViewerId(id)),
                    Op::Move(x, y) => l.move_by(Vector::new(x, y)),
                }
                if !l.entries().is_empty() {
                    prop_assert!(l.locate(l.pointer()).is_some(), "{:?}", l);
                }
            }
        }
    }
}
