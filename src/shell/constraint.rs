//! Pointer constraints: what the server reports about an application's lock or confinement,
//! and the shell's own state for it (pointer-lock data-model, research L2-L10).

use crate::canvas::{Point, Rect, Size};

/// Beyond this many rectangles an area or input shape counts as its bounding box, so one motion
/// walks at most this many (research L10, FR-012).
pub(crate) const MAX_CONFINE_RECTS: usize = 256;

/// Where a constraint is in its life (data-model "ConstraintState"). "Suspended" is not a state:
/// it is `Active` while a pan, move or resize runs. "Released by the user" belongs to the window,
/// not to one constraint (`Shell::released`), so a new one asked for at once waits too (M11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConstraintState {
    /// Its conditions do not hold (research L5).
    Waiting,
    /// It applies.
    Active,
}

/// What the effective area was computed from, so it is computed again only when one changes.
#[derive(Debug, Clone, PartialEq)]
struct AreaKey {
    input_gen: u64,
    size: Size,
    view: Rect,
}

/// The shell's copy of a window's constraint.
#[derive(Debug, Clone)]
pub(crate) struct Constraint {
    pub info: ConstraintInfo,
    pub state: ConstraintState,
    /// Bumped when the window's input shape or the constraint's area changes.
    input_gen: u64,
    cache: Option<(AreaKey, Vec<Rect>)>,
}

impl Constraint {
    pub fn new(info: ConstraintInfo) -> Constraint {
        Constraint {
            info,
            state: ConstraintState::Waiting,
            input_gen: 0,
            cache: None,
        }
    }

    /// The window's input shape or the constraint's area changed.
    pub fn changed(&mut self) {
        self.input_gen += 1;
        self.cache = None;
    }

    /// The effective area for a window of `size` and `input` and a view of `view` (all in window
    /// coordinates), from the cache when nothing changed.
    pub fn area(&mut self, size: Size, input: &[Rect], view: Rect) -> &[Rect] {
        let key = AreaKey {
            input_gen: self.input_gen,
            size,
            view,
        };
        if self.cache.as_ref().is_none_or(|(k, _)| *k != key) {
            let area = effective_area(self.info.area.as_deref(), size, input, view);
            self.cache = Some((key, area));
        }
        self.cache.as_ref().map_or(&[], |(_, a)| a.as_slice())
    }
}

/// `rects`, or their bounding box when there are more than [`MAX_CONFINE_RECTS`].
fn bounded(rects: &[Rect]) -> Vec<Rect> {
    if rects.len() > MAX_CONFINE_RECTS {
        Rect::bounding(rects).into_iter().collect()
    } else {
        rects.to_vec()
    }
}

/// The part of a window where its constraint holds the pointer (data-model "Effective area"):
/// the constraint's `area` (or the whole window of `size`) intersected with the window's
/// `input` shape and with `view`, the part of the window shown on the pointer's viewer. All in
/// window coordinates. Empty: the constraint cannot apply.
pub(crate) fn effective_area(
    area: Option<&[Rect]>,
    size: Size,
    input: &[Rect],
    view: Rect,
) -> Vec<Rect> {
    let window = [Rect::new(Point::new(0.0, 0.0), size)];
    let area = bounded(area.unwrap_or(&window));
    let input = bounded(input);
    let mut out = Vec::new();
    for a in &area {
        let Some(a) = a.intersection(&view) else {
            continue;
        };
        out.extend(input.iter().filter_map(|i| i.intersection(&a)));
    }
    bounded(&out)
}

/// Whether `p` lies in one of `area`'s rectangles.
pub(crate) fn inside(area: &[Rect], p: Point) -> bool {
    area.iter().any(|r| r.contains(p))
}

/// How far inside a rectangle's far edges a clamped point stays, so it is still inside the
/// half-open rectangle (as the layout does at display edges).
const INSIDE: f64 = 1e-6;

/// `p` clamped into `r`.
fn clamp_into(r: &Rect, p: Point) -> Point {
    let end = r.end();
    Point::new(
        p.x.clamp(r.origin.x, (end.x - INSIDE).max(r.origin.x)),
        p.y.clamp(r.origin.y, (end.y - INSIDE).max(r.origin.y)),
    )
}

/// Where a pointer moving from `from` to `to` ends up when confined to `area` (research L8), all
/// in window coordinates: `to` if it is inside; else `to` clamped into the rectangle holding
/// `from`, so the pointer slides along its edge; else (the area changed under the pointer) the
/// point of the area nearest to `to`. O(rectangles of `area`).
pub(crate) fn confine(area: &[Rect], from: Point, to: Point) -> Point {
    if inside(area, to) {
        return to;
    }
    if let Some(r) = area.iter().find(|r| r.contains(from)) {
        return clamp_into(r, to);
    }
    area.iter()
        .map(|r| clamp_into(r, to))
        .min_by(|a, b| {
            let da = (a.x - to.x).powi(2) + (a.y - to.y).powi(2);
            let db = (b.x - to.x).powi(2) + (b.y - to.y).powi(2);
            da.total_cmp(&db)
        })
        .unwrap_or(to)
}

/// Identifies one constraint object for its whole life; never reused (research L3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ConstraintToken(pub u64);

/// Whether the pointer is held still or kept inside an area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConstraintKind {
    Lock,
    Confine,
}

/// A window's constraint as the server reports it (`WindowEvent::ConstraintChanged`).
#[derive(Debug, Clone, PartialEq)]
pub struct ConstraintInfo {
    pub token: ConstraintToken,
    pub kind: ConstraintKind,
    /// `false`: single-use (`oneshot`); `true`: lasting (`persistent`).
    pub persistent: bool,
    /// Window coordinates; `None`: the whole window. Any number of rectangles - beyond
    /// `MAX_CONFINE_RECTS` the shell uses their bounding box (research L10).
    pub area: Option<Vec<Rect>>,
    /// Locks only: the latest committed release point, window coordinates.
    pub hint: Option<Point>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::Size;

    fn r(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect::new(Point::new(x, y), Size::new(w, h))
    }

    /// A 200x100 window accepting input everywhere, fully in view.
    const SIZE: Size = Size { w: 200.0, h: 100.0 };

    fn full() -> Vec<Rect> {
        vec![r(0.0, 0.0, 200.0, 100.0)]
    }

    fn view() -> Rect {
        r(-400.0, -350.0, 1000.0, 800.0)
    }

    #[test]
    fn without_an_area_it_is_the_window_in_view() {
        assert_eq!(effective_area(None, SIZE, &full(), view()), full());
    }

    #[test]
    fn the_area_is_cut_to_the_input_shape_and_the_view() {
        let area = [r(50.0, 20.0, 500.0, 30.0)];
        assert_eq!(
            effective_area(Some(&area), SIZE, &full(), view()),
            vec![r(50.0, 20.0, 150.0, 30.0)]
        );
        let input = [r(0.0, 0.0, 100.0, 100.0), r(150.0, 0.0, 50.0, 100.0)];
        assert_eq!(
            effective_area(Some(&area), SIZE, &input, view()),
            vec![r(50.0, 20.0, 50.0, 30.0), r(150.0, 20.0, 50.0, 30.0)]
        );
        // Only the right half of the window is on this viewer.
        let half = r(100.0, -1000.0, 1000.0, 2000.0);
        assert_eq!(
            effective_area(None, SIZE, &full(), half),
            vec![r(100.0, 0.0, 100.0, 100.0)]
        );
    }

    #[test]
    fn an_empty_or_disjoint_area_gives_nothing() {
        assert!(effective_area(Some(&[]), SIZE, &full(), view()).is_empty());
        let outside = [r(300.0, 0.0, 10.0, 10.0)];
        assert!(effective_area(Some(&outside), SIZE, &full(), view()).is_empty());
        let away = r(5000.0, 5000.0, 10.0, 10.0);
        assert!(effective_area(None, SIZE, &full(), away).is_empty());
    }

    #[test]
    fn more_than_256_rectangles_count_as_their_bounding_box() {
        let many: Vec<Rect> = (0..300).map(|i| r(f64::from(i), 0.0, 1.0, 1.0)).collect();
        assert_eq!(
            effective_area(Some(&many), SIZE, &full(), view()),
            vec![r(0.0, 0.0, 200.0, 1.0)]
        );
        let input: Vec<Rect> = (0..300)
            .map(|i| r(0.0, f64::from(i) / 4.0, 1.0, 0.2))
            .collect();
        assert_eq!(
            effective_area(None, SIZE, &input, view()),
            vec![r(0.0, 0.0, 1.0, 299.0 / 4.0 + 0.2)]
        );
        // 20 strips across 20 strips: 400 pieces, more than 256, so their bounding box.
        let rows: Vec<Rect> = (0..20)
            .map(|i| r(0.0, f64::from(2 * i), 200.0, 1.0))
            .collect();
        let columns: Vec<Rect> = (0..20)
            .map(|j| r(f64::from(10 * j), 0.0, 1.0, 100.0))
            .collect();
        assert_eq!(
            effective_area(Some(&rows), SIZE, &columns, view()),
            vec![r(0.0, 0.0, 191.0, 39.0)]
        );
    }

    #[test]
    fn areas_at_the_i32_limits_stay_finite() {
        let min = f64::from(i32::MIN);
        let max = f64::from(i32::MAX);
        let huge = [r(min, min, max, max), r(0.0, 0.0, max, max)];
        let area = effective_area(Some(&huge), SIZE, &full(), view());
        assert_eq!(area, full());
        assert!(
            area.iter()
                .all(|a| a.origin.x.is_finite() && a.size.w.is_finite())
        );
    }

    // --- confine (research L8) ---------------------------------------------------------------

    fn p(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    fn close(a: Point, b: Point) -> bool {
        (a.x - b.x).abs() < 1e-3 && (a.y - b.y).abs() < 1e-3
    }

    #[test]
    fn a_point_inside_the_area_is_kept() {
        let area = [r(0.0, 0.0, 100.0, 100.0)];
        assert_eq!(confine(&area, p(50.0, 50.0), p(70.0, 20.0)), p(70.0, 20.0));
    }

    #[test]
    fn a_move_out_stops_at_the_edge_and_slides_along_it() {
        let area = [r(0.0, 0.0, 100.0, 100.0)];
        let to = confine(&area, p(50.0, 50.0), p(150.0, 80.0));
        assert!(close(to, p(100.0, 80.0)), "{to:?}");
        assert!(
            inside(&area, to),
            "kept inside the half-open rectangle: {to:?}"
        );
        let corner = confine(&area, p(50.0, 50.0), p(-20.0, -30.0));
        assert_eq!(corner, p(0.0, 0.0));
    }

    #[test]
    fn a_move_into_a_touching_rectangle_is_allowed() {
        let area = [r(0.0, 0.0, 100.0, 100.0), r(100.0, 0.0, 100.0, 50.0)];
        assert_eq!(
            confine(&area, p(50.0, 20.0), p(150.0, 20.0)),
            p(150.0, 20.0)
        );
        // Beyond both: it stays in the rectangle it came from.
        let to = confine(&area, p(50.0, 80.0), p(150.0, 80.0));
        assert!(close(to, p(100.0, 80.0)), "{to:?}");
    }

    #[test]
    fn from_outside_the_area_the_nearest_point_is_taken() {
        let area = [r(0.0, 0.0, 10.0, 10.0), r(100.0, 100.0, 10.0, 10.0)];
        let to = confine(&area, p(50.0, 50.0), p(60.0, 5.0));
        assert!(close(to, p(10.0, 5.0)), "{to:?}");
        assert!(inside(&area, to));
        let to = confine(&area, p(50.0, 50.0), p(90.0, 95.0));
        assert!(close(to, p(100.0, 100.0)), "{to:?}");
    }

    proptest::proptest! {
        #[test]
        fn from_inside_the_result_is_always_inside(
            rects in proptest::collection::vec(
                (-1000.0..1000.0f64, -1000.0..1000.0f64, 1.0..500.0f64, 1.0..500.0f64),
                1..8,
            ),
            pick in 0usize..8,
            u in 0.0..1.0f64,
            v in 0.0..1.0f64,
            dx in -3000.0..3000.0f64,
            dy in -3000.0..3000.0f64,
        ) {
            let area: Vec<Rect> = rects.iter().map(|&(x, y, w, h)| r(x, y, w, h)).collect();
            let start = area[pick % area.len()];
            let from = p(start.origin.x + u * start.size.w, start.origin.y + v * start.size.h);
            let to = confine(&area, from, p(from.x + dx, from.y + dy));
            proptest::prop_assert!(inside(&area, to), "{:?} from {:?}", to, from);
        }
    }
}
