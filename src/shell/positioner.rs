//! Popup placement by the rules of an `xdg_positioner` (popups research P3). Pure: integer
//! coordinates relative to the parent's window geometry, no Wayland types.
//!
//! A popup is first put at the anchor point (on the anchor rectangle, plus the offset) and grown in
//! the gravity's direction. If it then sticks out of the constraint area on an axis, the
//! adjustments the application allows are tried in the protocol's order: flip, slide, resize. The
//! arithmetic runs in `i64` and saturates back to `i32` (Principle II).

use std::ops::BitOr;

use crate::canvas::{Point, Rect, Size};

/// An integer rectangle in protocol coordinates (logical pixels).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct IRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl IRect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> IRect {
        IRect { x, y, w, h }
    }

    /// The same rectangle in `f64` coordinates.
    pub fn to_rect(self) -> Rect {
        Rect::new(
            Point::new(f64::from(self.x), f64::from(self.y)),
            Size::new(f64::from(self.w), f64::from(self.h)),
        )
    }
}

/// An `xdg_positioner` anchor or gravity: an edge, a corner, or none (the centre).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum Anchor8 {
    #[default]
    None,
    Top,
    Bottom,
    Left,
    Right,
    TopLeft,
    BottomLeft,
    TopRight,
    BottomRight,
}

/// Where on an axis: the start (left or top), the middle, or the end (right or bottom).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Start,
    Middle,
    End,
}

impl Side {
    fn flipped(self) -> Side {
        match self {
            Side::Start => Side::End,
            Side::Middle => Side::Middle,
            Side::End => Side::Start,
        }
    }
}

impl Anchor8 {
    /// All values, in protocol order (0-8).
    pub const ALL: [Anchor8; 9] = [
        Anchor8::None,
        Anchor8::Top,
        Anchor8::Bottom,
        Anchor8::Left,
        Anchor8::Right,
        Anchor8::TopLeft,
        Anchor8::BottomLeft,
        Anchor8::TopRight,
        Anchor8::BottomRight,
    ];

    /// The value for an `xdg_positioner.anchor` or `gravity` number; `None` outside 0-8.
    pub fn from_u32(value: u32) -> Option<Anchor8> {
        usize::try_from(value)
            .ok()
            .and_then(|i| Anchor8::ALL.get(i).copied())
    }

    fn x(self) -> Side {
        match self {
            Anchor8::Left | Anchor8::TopLeft | Anchor8::BottomLeft => Side::Start,
            Anchor8::Right | Anchor8::TopRight | Anchor8::BottomRight => Side::End,
            Anchor8::None | Anchor8::Top | Anchor8::Bottom => Side::Middle,
        }
    }

    fn y(self) -> Side {
        match self {
            Anchor8::Top | Anchor8::TopLeft | Anchor8::TopRight => Side::Start,
            Anchor8::Bottom | Anchor8::BottomLeft | Anchor8::BottomRight => Side::End,
            Anchor8::None | Anchor8::Left | Anchor8::Right => Side::Middle,
        }
    }

    fn from_sides(x: Side, y: Side) -> Anchor8 {
        match (x, y) {
            (Side::Middle, Side::Middle) => Anchor8::None,
            (Side::Middle, Side::Start) => Anchor8::Top,
            (Side::Middle, Side::End) => Anchor8::Bottom,
            (Side::Start, Side::Middle) => Anchor8::Left,
            (Side::End, Side::Middle) => Anchor8::Right,
            (Side::Start, Side::Start) => Anchor8::TopLeft,
            (Side::Start, Side::End) => Anchor8::BottomLeft,
            (Side::End, Side::Start) => Anchor8::TopRight,
            (Side::End, Side::End) => Anchor8::BottomRight,
        }
    }

    /// Left and right swapped.
    pub fn flip_x(self) -> Anchor8 {
        Anchor8::from_sides(self.x().flipped(), self.y())
    }

    /// Top and bottom swapped.
    pub fn flip_y(self) -> Anchor8 {
        Anchor8::from_sides(self.x(), self.y().flipped())
    }
}

/// The `xdg_positioner.constraint_adjustment` bits an application allows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Adjust(u32);

impl Adjust {
    pub const NONE: Adjust = Adjust(0);
    pub const SLIDE_X: Adjust = Adjust(1);
    pub const SLIDE_Y: Adjust = Adjust(2);
    pub const FLIP_X: Adjust = Adjust(4);
    pub const FLIP_Y: Adjust = Adjust(8);
    pub const RESIZE_X: Adjust = Adjust(16);
    pub const RESIZE_Y: Adjust = Adjust(32);
    pub const ALL: Adjust = Adjust(0x3f);

    /// The set for a protocol value; `None` if it has bits outside the protocol's six.
    pub fn from_bits(bits: u32) -> Option<Adjust> {
        (bits & !Adjust::ALL.0 == 0).then_some(Adjust(bits))
    }

    pub fn bits(self) -> u32 {
        self.0
    }

    /// Whether every bit of `other` is allowed.
    pub fn contains(self, other: Adjust) -> bool {
        self.0 & other.0 == other.0
    }
}

impl BitOr for Adjust {
    type Output = Adjust;

    fn bitor(self, rhs: Adjust) -> Adjust {
        Adjust(self.0 | rhs.0)
    }
}

/// A copy of an `xdg_positioner`'s rules (data-model "PopupRules").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PopupRules {
    /// At least 1 x 1 (checked by the server).
    pub size: (i32, i32),
    /// Relative to the parent's window geometry; width and height at least 0.
    pub anchor_rect: IRect,
    pub anchor: Anchor8,
    pub gravity: Anchor8,
    pub adjust: Adjust,
    pub offset: (i32, i32),
    pub reactive: bool,
    /// `set_parent_size`: kept for the record; placement does not depend on the parent's size
    /// (research P10).
    pub parent_size: Option<(i32, i32)>,
}

impl Default for PopupRules {
    fn default() -> Self {
        PopupRules {
            size: (1, 1),
            anchor_rect: IRect::default(),
            anchor: Anchor8::None,
            gravity: Anchor8::None,
            adjust: Adjust::NONE,
            offset: (0, 0),
            reactive: false,
            parent_size: None,
        }
    }
}

/// The popup's rectangle for `rules`, kept inside `area` as far as the allowed adjustments go
/// (research P3). Both are relative to the parent's window geometry; `area: None` places the
/// popup exactly as asked.
pub fn place(rules: &PopupRules, area: Option<IRect>) -> IRect {
    let size = (
        i64::from(rules.size.0.max(1)),
        i64::from(rules.size.1.max(1)),
    );
    let at = |anchor: Anchor8, gravity: Anchor8| {
        let a = rules.anchor_rect;
        (
            Span::grown(
                Span::new(a.x, a.w),
                anchor.x(),
                gravity.x(),
                rules.offset.0,
                size.0,
            ),
            Span::grown(
                Span::new(a.y, a.h),
                anchor.y(),
                gravity.y(),
                rules.offset.1,
                size.1,
            ),
        )
    };
    let (mut x, mut y) = at(rules.anchor, rules.gravity);
    if let Some(area) = area {
        let adjust = rules.adjust;
        let flipped_x = at(rules.anchor.flip_x(), rules.gravity.flip_x()).0;
        let flipped_y = at(rules.anchor.flip_y(), rules.gravity.flip_y()).1;
        x = x.unconstrained(
            Span::new(area.x, area.w),
            adjust.contains(Adjust::FLIP_X).then_some(flipped_x),
            adjust.contains(Adjust::SLIDE_X),
            adjust.contains(Adjust::RESIZE_X),
        );
        y = y.unconstrained(
            Span::new(area.y, area.h),
            adjust.contains(Adjust::FLIP_Y).then_some(flipped_y),
            adjust.contains(Adjust::SLIDE_Y),
            adjust.contains(Adjust::RESIZE_Y),
        );
    }
    IRect::new(
        saturate(x.start),
        saturate(y.start),
        saturate(x.len).max(1),
        saturate(y.len).max(1),
    )
}

/// `v` clamped into `i32`.
fn saturate(v: i64) -> i32 {
    i32::try_from(v.clamp(i64::from(i32::MIN), i64::from(i32::MAX))).unwrap_or(0)
}

/// One axis of a rectangle, in `i64` so sums of protocol values cannot overflow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Span {
    start: i64,
    len: i64,
}

impl Span {
    /// From a protocol start and length; a negative length counts as 0.
    fn new(start: i32, len: i32) -> Span {
        Span {
            start: i64::from(start),
            len: i64::from(len.max(0)),
        }
    }

    fn end(self) -> i64 {
        self.start + self.len
    }

    /// A popup of `len` grown from the `anchor` side of `anchor_span` (plus `offset`) in the
    /// `gravity` direction: towards the start, towards the end, or centred.
    fn grown(anchor_span: Span, anchor: Side, gravity: Side, offset: i32, len: i64) -> Span {
        let point = match anchor {
            Side::Start => anchor_span.start,
            Side::Middle => anchor_span.start + anchor_span.len / 2,
            Side::End => anchor_span.end(),
        } + i64::from(offset);
        let start = match gravity {
            Side::Start => point - len,
            Side::Middle => point - len / 2,
            Side::End => point,
        };
        Span { start, len }
    }

    fn inside(self, area: Span) -> bool {
        self.start >= area.start && self.end() <= area.end()
    }

    /// The span after the allowed adjustments, in the protocol's order: the flipped span if it
    /// fits; then slid back inside (a span longer than the area goes to the area's start); then
    /// cut to the area, unless nothing would be left.
    fn unconstrained(self, area: Span, flipped: Option<Span>, slide: bool, resize: bool) -> Span {
        if self.inside(area) {
            return self;
        }
        if let Some(f) = flipped.filter(|f| f.inside(area)) {
            return f;
        }
        let mut s = self;
        if slide {
            if s.len > area.len || s.start < area.start {
                s.start = area.start;
            } else if s.end() > area.end() {
                s.start = area.end() - s.len;
            }
        }
        if resize && !s.inside(area) {
            let start = s.start.max(area.start);
            let end = s.end().min(area.end());
            if end > start {
                s = Span {
                    start,
                    len: end - start,
                };
            }
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn rules(anchor: Anchor8, gravity: Anchor8) -> PopupRules {
        PopupRules {
            size: (30, 10),
            anchor_rect: IRect::new(100, 50, 40, 20),
            anchor,
            gravity,
            ..PopupRules::default()
        }
    }

    /// The anchor point on `(100, 50, 40, 20)` for each anchor.
    fn anchor_point(a: Anchor8) -> (i32, i32) {
        match a {
            Anchor8::None => (120, 60),
            Anchor8::Top => (120, 50),
            Anchor8::Bottom => (120, 70),
            Anchor8::Left => (100, 60),
            Anchor8::Right => (140, 60),
            Anchor8::TopLeft => (100, 50),
            Anchor8::BottomLeft => (100, 70),
            Anchor8::TopRight => (140, 50),
            Anchor8::BottomRight => (140, 70),
        }
    }

    /// Where a 30 x 10 popup's top-left goes relative to the anchor point, for each gravity.
    fn gravity_shift(g: Anchor8) -> (i32, i32) {
        match g {
            Anchor8::None => (-15, -5),
            Anchor8::Top => (-15, -10),
            Anchor8::Bottom => (-15, 0),
            Anchor8::Left => (-30, -5),
            Anchor8::Right => (0, -5),
            Anchor8::TopLeft => (-30, -10),
            Anchor8::BottomLeft => (-30, 0),
            Anchor8::TopRight => (0, -10),
            Anchor8::BottomRight => (0, 0),
        }
    }

    #[test]
    fn protocol_values_are_checked() {
        for (i, a) in Anchor8::ALL.iter().enumerate() {
            assert_eq!(Anchor8::from_u32(i as u32), Some(*a));
        }
        assert_eq!(Anchor8::from_u32(9), None);
        assert_eq!(Anchor8::from_u32(u32::MAX), None);
        assert_eq!(Adjust::from_bits(0), Some(Adjust::NONE));
        assert_eq!(Adjust::from_bits(0x3f), Some(Adjust::ALL));
        assert_eq!(Adjust::from_bits(0x40), None);
        assert_eq!(Adjust::from_bits(u32::MAX), None);
    }

    #[test]
    fn anchor_and_gravity_place_the_popup_without_an_area() {
        for anchor in Anchor8::ALL {
            for gravity in Anchor8::ALL {
                let (ax, ay) = anchor_point(anchor);
                let (gx, gy) = gravity_shift(gravity);
                assert_eq!(
                    place(&rules(anchor, gravity), None),
                    IRect::new(ax + gx, ay + gy, 30, 10),
                    "anchor {anchor:?}, gravity {gravity:?}"
                );
                let shifted = PopupRules {
                    offset: (5, -7),
                    ..rules(anchor, gravity)
                };
                assert_eq!(
                    place(&shifted, None),
                    IRect::new(ax + gx + 5, ay + gy - 7, 30, 10),
                    "offset, anchor {anchor:?}, gravity {gravity:?}"
                );
            }
        }
    }

    #[test]
    fn without_adjustments_the_area_changes_nothing() {
        let r = rules(Anchor8::BottomRight, Anchor8::BottomRight);
        let area = IRect::new(0, 0, 120, 60);
        assert_eq!(place(&r, Some(area)), place(&r, None));
        assert_eq!(place(&r, None), IRect::new(140, 70, 30, 10));
    }

    #[test]
    fn a_popup_that_fits_is_not_moved() {
        let r = PopupRules {
            adjust: Adjust::ALL,
            ..rules(Anchor8::BottomLeft, Anchor8::BottomRight)
        };
        assert_eq!(
            place(&r, Some(IRect::new(0, 0, 1000, 1000))),
            IRect::new(100, 70, 30, 10)
        );
    }

    #[test]
    fn flip_x_moves_to_the_other_side_of_the_anchor() {
        // A submenu to the right of the item, past the area's right edge at 160.
        let r = PopupRules {
            adjust: Adjust::FLIP_X,
            ..rules(Anchor8::TopRight, Anchor8::BottomRight)
        };
        let area = IRect::new(0, 0, 160, 500);
        assert_eq!(place(&r, Some(area)), IRect::new(70, 50, 30, 10));
    }

    #[test]
    fn a_flip_that_does_not_fit_either_is_undone() {
        let r = PopupRules {
            adjust: Adjust::FLIP_X,
            ..rules(Anchor8::TopRight, Anchor8::BottomRight)
        };
        // Neither 140..170 nor 70..100 fits in 90..160.
        let area = IRect::new(90, 0, 70, 500);
        assert_eq!(place(&r, Some(area)), IRect::new(140, 50, 30, 10));
    }

    #[test]
    fn flip_y_moves_above_the_anchor() {
        let r = PopupRules {
            adjust: Adjust::FLIP_Y,
            ..rules(Anchor8::Bottom, Anchor8::Bottom)
        };
        let area = IRect::new(0, 0, 500, 75);
        assert_eq!(place(&r, Some(area)), IRect::new(105, 40, 30, 10));
    }

    #[test]
    fn slide_moves_back_inside() {
        let r = PopupRules {
            adjust: Adjust::SLIDE_X | Adjust::SLIDE_Y,
            ..rules(Anchor8::BottomRight, Anchor8::BottomRight)
        };
        let area = IRect::new(0, 0, 160, 75);
        assert_eq!(place(&r, Some(area)), IRect::new(130, 65, 30, 10));
        let left = PopupRules {
            adjust: Adjust::SLIDE_X,
            ..rules(Anchor8::TopLeft, Anchor8::BottomLeft)
        };
        assert_eq!(
            place(&left, Some(IRect::new(90, 0, 500, 500))),
            IRect::new(90, 50, 30, 10)
        );
    }

    #[test]
    fn a_popup_larger_than_the_area_slides_to_its_start() {
        let r = PopupRules {
            size: (300, 10),
            adjust: Adjust::SLIDE_X,
            ..rules(Anchor8::TopLeft, Anchor8::BottomRight)
        };
        let area = IRect::new(20, 0, 200, 500);
        assert_eq!(place(&r, Some(area)), IRect::new(20, 50, 300, 10));
    }

    #[test]
    fn resize_cuts_to_the_area() {
        let r = PopupRules {
            size: (30, 400),
            adjust: Adjust::RESIZE_Y,
            ..rules(Anchor8::BottomLeft, Anchor8::BottomRight)
        };
        let area = IRect::new(0, 0, 500, 300);
        assert_eq!(place(&r, Some(area)), IRect::new(100, 70, 30, 230));
    }

    #[test]
    fn a_popup_wholly_outside_keeps_its_size_when_resized() {
        let r = PopupRules {
            adjust: Adjust::RESIZE_X | Adjust::RESIZE_Y,
            ..rules(Anchor8::BottomRight, Anchor8::BottomRight)
        };
        let area = IRect::new(0, 0, 50, 50);
        assert_eq!(place(&r, Some(area)), IRect::new(140, 70, 30, 10));
    }

    #[test]
    fn flip_comes_before_slide_and_slide_before_resize() {
        // Flip fits: no slide.
        let r = PopupRules {
            adjust: Adjust::FLIP_X | Adjust::SLIDE_X | Adjust::RESIZE_X,
            ..rules(Anchor8::TopRight, Anchor8::BottomRight)
        };
        assert_eq!(
            place(&r, Some(IRect::new(0, 0, 160, 500))),
            IRect::new(70, 50, 30, 10)
        );
        // Flip does not fit, slide does: no resize.
        assert_eq!(
            place(&r, Some(IRect::new(90, 0, 70, 500))),
            IRect::new(130, 50, 30, 10)
        );
        // Neither fits a 20-wide area: slid to its start, then cut to it.
        assert_eq!(
            place(&r, Some(IRect::new(120, 0, 20, 500))),
            IRect::new(120, 50, 20, 10)
        );
    }

    #[test]
    fn huge_values_saturate() {
        let r = PopupRules {
            size: (i32::MAX, i32::MAX),
            anchor_rect: IRect::new(i32::MAX, i32::MIN, i32::MAX, i32::MAX),
            anchor: Anchor8::BottomRight,
            gravity: Anchor8::TopLeft,
            offset: (i32::MAX, i32::MIN),
            adjust: Adjust::ALL,
            ..PopupRules::default()
        };
        for area in [None, Some(IRect::new(0, 0, 100, 100))] {
            let p = place(&r, area);
            assert!(p.w >= 1 && p.h >= 1, "{p:?}");
        }
    }

    fn any_anchor() -> impl Strategy<Value = Anchor8> {
        (0u32..9).prop_map(|v| Anchor8::from_u32(v).unwrap_or_default())
    }

    fn any_rules(adjust: Adjust) -> impl Strategy<Value = PopupRules> {
        (
            (1i32..200, 1i32..200),
            (-500i32..500, -500i32..500, 0i32..100, 0i32..100),
            any_anchor(),
            any_anchor(),
            (-50i32..50, -50i32..50),
        )
            .prop_map(
                move |(size, (x, y, w, h), anchor, gravity, offset)| PopupRules {
                    size,
                    anchor_rect: IRect::new(x, y, w, h),
                    anchor,
                    gravity,
                    adjust,
                    offset,
                    ..PopupRules::default()
                },
            )
    }

    fn inside(r: IRect, area: IRect) -> bool {
        r.x >= area.x
            && r.y >= area.y
            && r.x + r.w <= area.x + area.w
            && r.y + r.h <= area.y + area.h
    }

    proptest! {
        #[test]
        fn sliding_keeps_a_popup_that_fits_inside(
            r in any_rules(Adjust::SLIDE_X | Adjust::SLIDE_Y),
            (ax, ay) in (-300i32..300, -300i32..300),
        ) {
            let area = IRect::new(ax, ay, 250, 250);
            prop_assert!(inside(place(&r, Some(area)), area));
        }

        #[test]
        fn every_adjustment_keeps_a_popup_that_fits_inside(
            r in any_rules(Adjust::ALL),
            (ax, ay) in (-300i32..300, -300i32..300),
        ) {
            let area = IRect::new(ax, ay, 250, 250);
            prop_assert!(inside(place(&r, Some(area)), area));
        }

        #[test]
        fn without_adjustments_placement_ignores_the_area(
            r in any_rules(Adjust::NONE),
            (ax, ay, aw, ah) in (-300i32..300, -300i32..300, 1i32..300, 1i32..300),
        ) {
            prop_assert_eq!(place(&r, Some(IRect::new(ax, ay, aw, ah))), place(&r, None));
        }

        #[test]
        fn flipping_twice_gives_the_original(a in any_anchor()) {
            prop_assert_eq!(a.flip_x().flip_x(), a);
            prop_assert_eq!(a.flip_y().flip_y(), a);
        }
    }
}
