//! Plain 2D geometry in `f64`: points, vectors and sizes.
//!
//! The same types serve canvas units and logical pixels; which one is meant follows from
//! context (see `Camera`).

use std::ops::{Add, Div, Mul, Neg, Sub};

/// A position.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

/// A displacement between two positions.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vector {
    pub x: f64,
    pub y: f64,
}

/// A width and height, never negative when built with [`Size::new`].
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Size {
    pub w: f64,
    pub h: f64,
}

impl Point {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

impl Vector {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

impl Size {
    /// Builds a size; negative components become zero.
    pub fn new(w: f64, h: f64) -> Self {
        Self {
            w: w.max(0.0),
            h: h.max(0.0),
        }
    }

    /// The centre point of a rectangle of this size with its corner at the origin.
    pub fn center(self) -> Point {
        Point::new(self.w / 2.0, self.h / 2.0)
    }

    /// The size as a vector from the origin to the far corner.
    pub fn to_vector(self) -> Vector {
        Vector::new(self.w, self.h)
    }
}

/// An axis-aligned rectangle: top-left `origin` and `size`. It covers `[x, x + w)` and
/// `[y, y + h)`, so rectangles that only touch do not overlap.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub origin: Point,
    pub size: Size,
}

impl Rect {
    pub fn new(origin: Point, size: Size) -> Self {
        Self { origin, size }
    }

    /// The bottom-right corner (exclusive).
    pub fn end(&self) -> Point {
        self.origin + self.size.to_vector()
    }

    pub fn is_empty(&self) -> bool {
        self.size.w <= 0.0 || self.size.h <= 0.0
    }

    pub fn contains(&self, p: Point) -> bool {
        let end = self.end();
        p.x >= self.origin.x && p.y >= self.origin.y && p.x < end.x && p.y < end.y
    }

    pub fn intersects(&self, other: &Rect) -> bool {
        self.intersection(other).is_some()
    }

    /// The overlapping part, if it is not empty.
    pub fn intersection(&self, other: &Rect) -> Option<Rect> {
        let (a, b) = (self.end(), other.end());
        let x0 = self.origin.x.max(other.origin.x);
        let y0 = self.origin.y.max(other.origin.y);
        let x1 = a.x.min(b.x);
        let y1 = a.y.min(b.y);
        (x1 > x0 && y1 > y0).then(|| Rect::new(Point::new(x0, y0), Size::new(x1 - x0, y1 - y0)))
    }

    pub fn translate(&self, v: Vector) -> Rect {
        Rect::new(self.origin + v, self.size)
    }

    /// The smallest rectangle containing all of `rects`.
    pub fn bounding(rects: &[Rect]) -> Option<Rect> {
        let (first, rest) = rects.split_first()?;
        let (mut lo, mut hi) = (first.origin, first.end());
        for r in rest {
            let end = r.end();
            lo = Point::new(lo.x.min(r.origin.x), lo.y.min(r.origin.y));
            hi = Point::new(hi.x.max(end.x), hi.y.max(end.y));
        }
        Some(Rect::new(lo, Size::new(hi.x - lo.x, hi.y - lo.y)))
    }

    /// The parts of `self` not covered by `other`, as at most four disjoint rectangles.
    pub fn difference(&self, other: &Rect) -> Vec<Rect> {
        let Some(cut) = self.intersection(other) else {
            return if self.is_empty() {
                Vec::new()
            } else {
                vec![*self]
            };
        };
        let (end, cut_end) = (self.end(), cut.end());
        let band = |y0: f64, y1: f64, x0: f64, x1: f64| {
            Rect::new(Point::new(x0, y0), Size::new(x1 - x0, y1 - y0))
        };
        [
            band(self.origin.y, cut.origin.y, self.origin.x, end.x),
            band(cut_end.y, end.y, self.origin.x, end.x),
            band(cut.origin.y, cut_end.y, self.origin.x, cut.origin.x),
            band(cut.origin.y, cut_end.y, cut_end.x, end.x),
        ]
        .into_iter()
        .filter(|r| !r.is_empty())
        .collect()
    }
}

impl Add<Vector> for Point {
    type Output = Point;
    fn add(self, v: Vector) -> Point {
        Point::new(self.x + v.x, self.y + v.y)
    }
}

impl Sub<Vector> for Point {
    type Output = Point;
    fn sub(self, v: Vector) -> Point {
        Point::new(self.x - v.x, self.y - v.y)
    }
}

impl Sub for Point {
    type Output = Vector;
    fn sub(self, other: Point) -> Vector {
        Vector::new(self.x - other.x, self.y - other.y)
    }
}

impl Add for Vector {
    type Output = Vector;
    fn add(self, v: Vector) -> Vector {
        Vector::new(self.x + v.x, self.y + v.y)
    }
}

impl Mul<f64> for Vector {
    type Output = Vector;
    fn mul(self, k: f64) -> Vector {
        Vector::new(self.x * k, self.y * k)
    }
}

impl Div<f64> for Vector {
    type Output = Vector;
    fn div(self, k: f64) -> Vector {
        Vector::new(self.x / k, self.y / k)
    }
}

impl Neg for Vector {
    type Output = Vector;
    fn neg(self) -> Vector {
        Vector::new(-self.x, -self.y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn point_plus_vector() {
        assert_eq!(
            Point::new(1.0, 2.0) + Vector::new(3.0, -4.0),
            Point::new(4.0, -2.0)
        );
    }

    #[test]
    fn point_minus_vector() {
        assert_eq!(
            Point::new(1.0, 2.0) - Vector::new(3.0, -4.0),
            Point::new(-2.0, 6.0)
        );
    }

    #[test]
    fn point_minus_point_is_vector() {
        assert_eq!(
            Point::new(5.0, 7.0) - Point::new(2.0, 3.0),
            Vector::new(3.0, 4.0)
        );
    }

    #[test]
    fn vector_scaling() {
        assert_eq!(Vector::new(2.0, -3.0) * 2.0, Vector::new(4.0, -6.0));
        assert_eq!(Vector::new(2.0, -3.0) / 2.0, Vector::new(1.0, -1.5));
        assert_eq!(-Vector::new(2.0, -3.0), Vector::new(-2.0, 3.0));
    }

    #[test]
    fn size_center() {
        assert_eq!(Size::new(1280.0, 800.0).center(), Point::new(640.0, 400.0));
    }

    #[test]
    fn size_clamps_negative_to_zero() {
        assert_eq!(Size::new(-5.0, 10.0), Size::new(0.0, 10.0));
    }

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect::new(Point::new(x, y), Size::new(w, h))
    }

    #[test]
    fn rect_contains_is_half_open() {
        let r = rect(10.0, 20.0, 30.0, 40.0);
        assert!(r.contains(Point::new(10.0, 20.0)));
        assert!(r.contains(Point::new(39.9, 59.9)));
        assert!(!r.contains(Point::new(40.0, 30.0)));
        assert!(!r.contains(Point::new(20.0, 60.0)));
        assert!(!r.contains(Point::new(9.9, 30.0)));
    }

    #[test]
    fn rect_intersects() {
        let r = rect(0.0, 0.0, 10.0, 10.0);
        assert!(r.intersects(&rect(5.0, 5.0, 10.0, 10.0)));
        assert!(!r.intersects(&rect(10.0, 0.0, 5.0, 5.0)));
        assert!(!r.intersects(&rect(0.0, 0.0, 0.0, 5.0)));
    }

    #[test]
    fn rect_translate() {
        assert_eq!(
            rect(1.0, 2.0, 3.0, 4.0).translate(Vector::new(10.0, -2.0)),
            rect(11.0, 0.0, 3.0, 4.0)
        );
    }

    #[test]
    fn rect_bounding() {
        assert_eq!(Rect::bounding(&[]), None);
        assert_eq!(
            Rect::bounding(&[rect(0.0, 0.0, 10.0, 10.0), rect(-5.0, 4.0, 2.0, 20.0)]),
            Some(rect(-5.0, 0.0, 15.0, 24.0))
        );
    }

    #[test]
    fn rect_intersection() {
        let r = rect(0.0, 0.0, 10.0, 10.0);
        assert_eq!(
            r.intersection(&rect(5.0, -5.0, 10.0, 10.0)),
            Some(rect(5.0, 0.0, 5.0, 5.0))
        );
        assert_eq!(r.intersection(&rect(20.0, 0.0, 1.0, 1.0)), None);
    }

    #[test]
    fn rect_difference_covers_exactly_the_rest() {
        let r = rect(0.0, 0.0, 10.0, 10.0);
        let hole = rect(3.0, 3.0, 4.0, 4.0);
        let parts = r.difference(&hole);
        let area: f64 = parts.iter().map(|p| p.size.w * p.size.h).sum();
        assert_eq!(area, 100.0 - 16.0);
        for p in &parts {
            assert!(!p.intersects(&hole));
        }
        assert_eq!(r.difference(&rect(20.0, 20.0, 1.0, 1.0)), vec![r]);
        assert!(r.difference(&rect(-1.0, -1.0, 12.0, 12.0)).is_empty());
    }
}
