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
}
