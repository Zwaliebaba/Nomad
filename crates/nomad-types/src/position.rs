//! Positions in metres (Design doc 10 §8, doc 11 §5; D-099 item 2).

use serde::{Deserialize, Serialize};

/// A point in a star system's frame, in metres: 2D, origin at the star, +x towards system
/// east, +y towards system north (doc 10 §8).
///
/// Metres are the simulation's unit and the unit on the wire (D-099 item 2). Kilometres are
/// for documents and the UI only.
///
/// Decoding accepts any `f64`, including NaN and infinities. Whoever acts on a position from
/// outside the process checks [`PositionMetres::is_finite`] first (`CODING_STANDARDS.md`
/// §5).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PositionMetres {
    /// Towards system east, m.
    pub x: f64,
    /// Towards system north, m.
    pub y: f64,
}

impl PositionMetres {
    /// The star.
    pub const ORIGIN: Self = Self { x: 0.0, y: 0.0 };

    /// A point `x` m east and `y` m north of the star.
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    /// Whether both coordinates are finite numbers.
    pub const fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finiteness() {
        assert!(PositionMetres::ORIGIN.is_finite());
        assert!(PositionMetres::new(-190_000.0, 55_000.0).is_finite());
        assert!(!PositionMetres::new(f64::NAN, 0.0).is_finite());
        assert!(!PositionMetres::new(0.0, f64::INFINITY).is_finite());
    }
}
