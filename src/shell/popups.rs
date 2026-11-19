//! Popups on the canvas (popups data-model "Shell"): menus, lists, submenus and tooltips of an
//! application, attached to a window or to another popup. A popup is part of its window: its
//! position is relative to its parent, so it moves with the window, and it is drawn at the
//! window's zoom (FR-019).

use crate::canvas::{Point, Rect, Size, ViewerId};

use super::positioner::{IRect, PopupRules};

/// A popup's identity, assigned by the server at the popup's initial commit; never reused within
/// a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PopupId(pub u64);

/// Where the application shows a popup, after acking a placement.
#[derive(Debug, Clone, PartialEq)]
pub struct Shown {
    /// Relative to the parent's window geometry.
    pub at: (i32, i32),
    pub size: Size,
    /// Where it accepts pointer input, in popup coordinates.
    pub input: Vec<Rect>,
}

/// A popup the shell knows (data-model "Popup").
#[derive(Debug, Clone, PartialEq)]
pub struct Popup {
    pub id: PopupId,
    /// `None`: the window itself.
    pub parent: Option<PopupId>,
    pub rules: PopupRules,
    /// It has an accepted grab.
    pub grab: bool,
    /// The latest placement sent, relative to the parent.
    pub placed: IRect,
    /// `None` until the application shows it.
    pub shown: Option<Shown>,
}

/// The canvas area a popup is kept inside (research P4): the view of the viewer under the
/// pointer if it shows part of `window`, otherwise of the viewer showing the largest part of it,
/// otherwise none. `views`: each viewer's canvas rectangle.
pub(crate) fn constraint_view(
    views: &[(ViewerId, Rect)],
    pointer: Option<ViewerId>,
    window: Rect,
) -> Option<Rect> {
    let shown = |view: &Rect| {
        view.intersection(&window)
            .map(|r| r.size.w * r.size.h)
            .filter(|a| *a > 0.0)
    };
    if let Some((_, view)) = views
        .iter()
        .find(|(id, view)| Some(*id) == pointer && shown(view).is_some())
    {
        return Some(*view);
    }
    views
        .iter()
        .filter_map(|(_, view)| shown(view).map(|a| (a, *view)))
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, view)| view)
}

/// `view` (canvas) in the coordinates of a parent whose window geometry starts at canvas point
/// `origin`, rounded inwards to whole pixels.
pub(crate) fn area_in(view: Rect, origin: Point) -> IRect {
    let x0 = (view.origin.x - origin.x).ceil();
    let y0 = (view.origin.y - origin.y).ceil();
    let x1 = (view.origin.x + view.size.w - origin.x).floor();
    let y1 = (view.origin.y + view.size.h - origin.y).floor();
    IRect::new(
        to_i32(x0),
        to_i32(y0),
        to_i32((x1 - x0).max(0.0)),
        to_i32((y1 - y0).max(0.0)),
    )
}

/// A whole-number `f64` as `i32`, saturating (and 0 for NaN).
fn to_i32(v: f64) -> i32 {
    if v.is_nan() {
        return 0;
    }
    // Clamped into the i32 range first, so the conversion is exact.
    v.clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::{Camera, Size};

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect::new(Point::new(x, y), Size::new(w, h))
    }

    #[test]
    fn the_viewer_under_the_pointer_wins_when_it_shows_the_window() {
        let views = [
            (ViewerId(1), rect(0.0, 0.0, 1000.0, 800.0)),
            (ViewerId(2), rect(1000.0, 0.0, 1000.0, 800.0)),
        ];
        // The window is mostly on viewer 1, but the pointer is on viewer 2.
        let window = rect(100.0, 100.0, 1000.0, 300.0);
        assert_eq!(
            constraint_view(&views, Some(ViewerId(2)), window),
            Some(views[1].1)
        );
        assert_eq!(
            constraint_view(&views, Some(ViewerId(1)), window),
            Some(views[0].1)
        );
    }

    #[test]
    fn otherwise_the_viewer_showing_most_of_the_window() {
        let views = [
            (ViewerId(1), rect(0.0, 0.0, 1000.0, 800.0)),
            (ViewerId(2), rect(1000.0, 0.0, 1000.0, 800.0)),
            (ViewerId(3), rect(5000.0, 0.0, 1000.0, 800.0)),
        ];
        let window = rect(800.0, 100.0, 1000.0, 300.0);
        // The pointer is on viewer 3, which does not show the window.
        assert_eq!(
            constraint_view(&views, Some(ViewerId(3)), window),
            Some(views[1].1)
        );
        assert_eq!(constraint_view(&views, None, window), Some(views[1].1));
    }

    #[test]
    fn no_viewer_shows_the_window() {
        let views = [(ViewerId(1), rect(0.0, 0.0, 1000.0, 800.0))];
        assert_eq!(
            constraint_view(&views, Some(ViewerId(1)), rect(5000.0, 0.0, 10.0, 10.0)),
            None
        );
        assert_eq!(constraint_view(&[], None, rect(0.0, 0.0, 10.0, 10.0)), None);
    }

    #[test]
    fn the_area_is_in_the_parents_coordinates_rounded_inwards() {
        // A window at a fractional canvas origin, with a parent popup at (40, 30) in it.
        let origin = Point::new(100.25 + 40.0, 50.5 + 30.0);
        let view = rect(0.0, 0.0, 1000.0, 800.0);
        // x: -140.25 .. 859.75 -> -140 .. 859; y: -80.5 .. 719.5 -> -80 .. 719.
        assert_eq!(area_in(view, origin), IRect::new(-140, -80, 999, 799));
    }

    #[test]
    fn at_zoom_half_the_area_is_twice_as_large() {
        let size = Size::new(1000.0, 800.0);
        let view_of = |zoom: f64| {
            let camera = Camera {
                center: Point::new(0.0, 0.0),
                zoom,
            };
            let top_left = camera.to_canvas(Point::new(0.0, 0.0), size);
            Rect::new(top_left, Size::new(size.w / zoom, size.h / zoom))
        };
        let full = area_in(view_of(1.0), Point::new(0.0, 0.0));
        let half = area_in(view_of(0.5), Point::new(0.0, 0.0));
        assert_eq!((full.w, full.h), (1000, 800));
        assert_eq!((half.w, half.h), (2000, 1600));
        assert_eq!((half.x, half.y), (2 * full.x, 2 * full.y));
    }
}
