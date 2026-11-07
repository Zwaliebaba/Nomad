//! Amounts of alloy and crystal (Design doc 14 §2; D-049, D-064).
//!
//! Data only: what things cost and how pools fill are rules in `nomad-sim`. The type lives
//! here because the AI (`nomad-ai`, D-199) reads costs and income through its view and must
//! not depend on the simulation (A1 plan §4.1: "plain data the AI must read moves to
//! `nomad-types`, for example `Stock`").

use serde::{Deserialize, Serialize};

/// Milli-units per whole unit: amounts are integer milli-units, never floats (D-049).
pub const MILLI: u64 = 1_000;

/// Alloy and crystal, in milli-units (D-049).
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub struct Stock {
    /// Alloy, milli-units.
    pub alloy: u64,
    /// Crystal, milli-units.
    pub crystal: u64,
}

impl Stock {
    /// Nothing.
    pub const ZERO: Self = Self {
        alloy: 0,
        crystal: 0,
    };

    /// Whole units, as rules files and costs give them.
    pub const fn units(alloy: u64, crystal: u64) -> Self {
        Self {
            alloy: alloy.saturating_mul(MILLI),
            crystal: crystal.saturating_mul(MILLI),
        }
    }

    /// Whether `self` holds at least `other` of each.
    pub const fn covers(self, other: Self) -> bool {
        self.alloy >= other.alloy && self.crystal >= other.crystal
    }

    /// Both added.
    #[must_use]
    pub const fn plus(self, other: Self) -> Self {
        Self {
            alloy: self.alloy.saturating_add(other.alloy),
            crystal: self.crystal.saturating_add(other.crystal),
        }
    }

    /// `other` taken away, at most down to zero.
    #[must_use]
    pub const fn minus(self, other: Self) -> Self {
        Self {
            alloy: self.alloy.saturating_sub(other.alloy),
            crystal: self.crystal.saturating_sub(other.crystal),
        }
    }

    /// Half of each, rounded down.
    #[must_use]
    pub const fn half(self) -> Self {
        Self {
            alloy: self.alloy / 2,
            crystal: self.crystal / 2,
        }
    }

    /// Each scaled by `permille` thousandths, rounded down.
    #[must_use]
    pub fn permille(self, permille: u32) -> Self {
        fn scale(amount: u64, permille: u32) -> u64 {
            let scaled = u128::from(amount) * u128::from(permille) / u128::from(MILLI);
            u64::try_from(scaled).unwrap_or(u64::MAX)
        }
        Self {
            alloy: scale(self.alloy, permille),
            crystal: scale(self.crystal, permille),
        }
    }

    /// Both amounts together.
    pub const fn total(self) -> u64 {
        self.alloy.saturating_add(self.crystal)
    }

    /// Whether both are zero.
    pub const fn is_zero(self) -> bool {
        self.alloy == 0 && self.crystal == 0
    }

    /// Each capped at `cap`'s, with what did not fit.
    pub fn clip(self, cap: Self) -> (Self, Self) {
        let kept = Self {
            alloy: self.alloy.min(cap.alloy),
            crystal: self.crystal.min(cap.crystal),
        };
        (kept, self.minus(kept))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permille_rounds_down_and_saturates() {
        let s = Stock {
            alloy: 1_001,
            crystal: 7,
        };
        assert_eq!(
            s.permille(300),
            Stock {
                alloy: 300,
                crystal: 2
            }
        );
        assert_eq!(s.permille(1_000), s);
        let big = Stock {
            alloy: u64::MAX,
            crystal: 0,
        };
        assert_eq!(big.permille(2_000).alloy, u64::MAX);
    }
}
