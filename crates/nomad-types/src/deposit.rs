//! Deposits on sites (Design doc 14 §5, D-066).
//!
//! A site has 0–2 deposits. Each is an ore field or a crystal vein of poor, standard or rich
//! richness, and never runs out: deposits limit the extraction rate, not a quantity. Deposit
//! kinds and richness are public (D-044, doc 16 §2), and so is the value derived from them.
//! Deposit value feeds doc 10 invariant 5 (balanced home neighbourhoods) and the D-041
//! season tie-break.
//!
//! Only the data lives here. The value table (ore 1, crystal 3, richness ×0.5/×1/×1.5) is a
//! game rule and lives in `nomad-sim`, which computes [`DepositValue`]s (D-086: the client
//! links no rules). Harvesting and extraction rates are doc 14 §6 and belong to the
//! simulation too.

use std::fmt;
use std::iter::Sum;
use std::ops::Add;

use serde::{Deserialize, Serialize};

/// What a deposit yields (D-066).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum DepositKind {
    /// Asteroids harvested for ore, like the phase-1 field AST-1.
    OreField,
    /// A crystal vein, extracted for CRYSTAL.
    CrystalVein,
}

/// How rich a deposit is (D-066). Stored as an enum; the simulation turns it into an integer
/// per-mille multiplier, so no floating point enters values or, later, rates (D-049).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Richness {
    /// ×0.5.
    Poor,
    /// ×1.
    Standard,
    /// ×1.5.
    Rich,
}

/// One deposit on a site.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Deposit {
    /// Ore field or crystal vein.
    pub kind: DepositKind,
    /// Its richness.
    pub richness: Richness,
}

impl Deposit {
    /// An ore field.
    pub const fn ore(richness: Richness) -> Self {
        Self {
            kind: DepositKind::OreField,
            richness,
        }
    }

    /// A crystal vein.
    pub const fn crystal(richness: Richness) -> Self {
        Self {
            kind: DepositKind::CrystalVein,
            richness,
        }
    }
}

/// Deposit value in thousandths, so that richness multipliers stay integer. A standard ore
/// field is worth 1,000; a rich crystal vein 4,500 (D-066).
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub struct DepositValue(u64);

impl DepositValue {
    /// No value.
    pub const ZERO: Self = Self(0);

    /// A value in thousandths.
    pub const fn from_milli(milli: u64) -> Self {
        Self(milli)
    }

    /// The value in thousandths.
    pub const fn milli(self) -> u64 {
        self.0
    }
}

impl Add for DepositValue {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        Self(self.0.saturating_add(rhs.0))
    }
}

impl Sum for DepositValue {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Self::ZERO, Add::add)
    }
}

impl fmt::Display for DepositValue {
    /// Whole units with up to three decimals, without trailing zeros: `12.5`, `7`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let whole = self.0 / 1_000;
        let frac = self.0 % 1_000;
        if frac == 0 {
            write!(f, "{whole}")
        } else {
            let digits = format!("{frac:03}");
            write!(f, "{whole}.{}", digits.trim_end_matches('0'))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_add_and_display() {
        let total: DepositValue = [
            DepositValue::from_milli(500),
            DepositValue::from_milli(4_500),
        ]
        .into_iter()
        .sum();
        assert_eq!(total, DepositValue::from_milli(5_000));
        assert_eq!(total.to_string(), "5");
        assert_eq!(DepositValue::from_milli(12_500).to_string(), "12.5");
        assert_eq!(DepositValue::from_milli(1_250).to_string(), "1.25");
        assert_eq!(DepositValue::ZERO.to_string(), "0");
    }
}
