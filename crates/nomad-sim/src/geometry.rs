//! 2D vectors in the system frame (Design doc 10 §8, doc 11 §5; D-049, D-099).
//!
//! Positions are metres and headings radians, as `f64`. Square roots and trigonometry come
//! from `libm`, never from `std`'s platform maths, so results are bit-identical on Linux and
//! Windows (D-049). Basic IEEE operations (`+ - * /`) are exact and reproducible already.

use std::ops::{Add, AddAssign, Mul, Neg, Sub, SubAssign};

use nomad_types::PositionMetres;
use serde::{Deserialize, Serialize};

/// A 2D vector: a displacement in metres or a velocity in metres per second, in the system
/// frame (+x east, +y north).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Vec2 {
    /// East component.
    pub x: f64,
    /// North component.
    pub y: f64,
}

impl Vec2 {
    /// The zero vector.
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

    /// A vector with these components.
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    /// The unit vector at `angle_rad` counter-clockwise from +x (east).
    pub fn from_angle(angle_rad: f64) -> Self {
        Self::new(libm::cos(angle_rad), libm::sin(angle_rad))
    }

    /// Dot product.
    pub fn dot(self, other: Self) -> f64 {
        self.x * other.x + self.y * other.y
    }

    /// The z component of the cross product: positive if `other` is counter-clockwise from
    /// `self`.
    pub fn cross(self, other: Self) -> f64 {
        self.x * other.y - self.y * other.x
    }

    /// Squared length.
    pub fn length_squared(self) -> f64 {
        self.dot(self)
    }

    /// Length.
    pub fn length(self) -> f64 {
        libm::sqrt(self.length_squared())
    }

    /// The unit vector in this direction, or `None` for a vector too short to have one.
    pub fn normalized(self) -> Option<Self> {
        let length = self.length();
        (length > 1e-9).then(|| self * (1.0 / length))
    }

    /// This vector, shortened to at most `max` long.
    #[must_use]
    pub fn clamp_length(self, max: f64) -> Self {
        let length = self.length();
        if length > max && length > 0.0 {
            self * (max / length)
        } else {
            self
        }
    }

    /// This vector rotated counter-clockwise by `angle_rad`.
    #[must_use]
    pub fn rotated(self, angle_rad: f64) -> Self {
        let (sin, cos) = (libm::sin(angle_rad), libm::cos(angle_rad));
        Self::new(self.x * cos - self.y * sin, self.x * sin + self.y * cos)
    }

    /// The angle counter-clockwise from +x, in `(-π, π]`.
    pub fn angle(self) -> f64 {
        libm::atan2(self.y, self.x)
    }

    /// Whether both components are finite.
    pub const fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

impl Add for Vec2 {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self::new(self.x + rhs.x, self.y + rhs.y)
    }
}

impl AddAssign for Vec2 {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl Sub for Vec2 {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self::new(self.x - rhs.x, self.y - rhs.y)
    }
}

impl SubAssign for Vec2 {
    fn sub_assign(&mut self, rhs: Self) {
        *self = *self - rhs;
    }
}

impl Mul<f64> for Vec2 {
    type Output = Self;
    fn mul(self, rhs: f64) -> Self {
        Self::new(self.x * rhs, self.y * rhs)
    }
}

impl Neg for Vec2 {
    type Output = Self;
    fn neg(self) -> Self {
        Self::new(-self.x, -self.y)
    }
}

impl From<PositionMetres> for Vec2 {
    fn from(p: PositionMetres) -> Self {
        Self::new(p.x, p.y)
    }
}

impl From<Vec2> for PositionMetres {
    fn from(v: Vec2) -> Self {
        Self::new(v.x, v.y)
    }
}

/// The point `radius_m` from the star, `angle_rad` counter-clockwise from +x.
pub fn polar(radius_m: f64, angle_rad: f64) -> PositionMetres {
    (Vec2::from_angle(angle_rad) * radius_m).into()
}

/// Straight-line distance between two points, in metres.
pub fn distance_m(a: PositionMetres, b: PositionMetres) -> f64 {
    (Vec2::from(a) - Vec2::from(b)).length()
}

/// The signed difference `to − from` between two angles, wrapped into `(-π, π]`.
pub fn angle_difference(from_rad: f64, to_rad: f64) -> f64 {
    use std::f64::consts::{PI, TAU};
    let mut delta = libm::fmod(to_rad - from_rad, TAU);
    if delta > PI {
        delta -= TAU;
    } else if delta <= -PI {
        delta += TAU;
    }
    delta
}

#[cfg(test)]
mod tests {
    use std::f64::consts::{FRAC_PI_2, PI};

    use super::*;

    #[test]
    fn polar_and_distance() {
        let p = polar(100_000.0, 0.0);
        assert_eq!(p, PositionMetres::new(100_000.0, 0.0));
        let q = PositionMetres::new(100_000.0, 0.0);
        let r = PositionMetres::new(103_000.0, 4_000.0);
        assert!((distance_m(q, r) - 5_000.0).abs() < 1e-9);
        assert!((distance_m(PositionMetres::ORIGIN, p) - 100_000.0).abs() < 1e-9);
    }

    #[test]
    fn vector_operations() {
        let a = Vec2::new(3.0, 4.0);
        assert!((a.length() - 5.0).abs() < 1e-12);
        assert!((a.clamp_length(1.0).length() - 1.0).abs() < 1e-12);
        assert_eq!(a.clamp_length(10.0), a);
        assert!(Vec2::ZERO.normalized().is_none());
        let r = Vec2::new(1.0, 0.0).rotated(FRAC_PI_2);
        assert!(r.x.abs() < 1e-12 && (r.y - 1.0).abs() < 1e-12);
        assert!(Vec2::new(1.0, 0.0).cross(Vec2::new(0.0, 1.0)) > 0.0);
        assert!((Vec2::new(0.0, -2.0).angle() + FRAC_PI_2).abs() < 1e-12);
    }

    #[test]
    fn angle_difference_wraps() {
        assert!((angle_difference(0.1, -0.1) + 0.2).abs() < 1e-12);
        assert!((angle_difference(PI - 0.1, -PI + 0.1) - 0.2).abs() < 1e-12);
        assert!((angle_difference(-PI + 0.1, PI - 0.1) + 0.2).abs() < 1e-12);
        assert!((angle_difference(0.0, 3.0 * PI) - PI).abs() < 1e-12);
    }
}
