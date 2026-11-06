//! Deposits on sites (Design doc 14 §5, D-066).
//!
//! A site has 0–2 deposits. Each is an ore field or a crystal vein of poor, standard or rich
//! richness, and never runs out: deposits limit the extraction rate, not a quantity. Deposit
//! value feeds doc 10 invariant 5 (balanced home neighbourhoods) and the D-041 season
//! tie-break. Harvesting and extraction rates are doc 14 §6 and are not modelled here.
//!
//! The deposit types are public data (doc 16 §2), so they live in `nomad-types` (D-086,
//! doc 20 §3) and are re-exported here. The value table and how many deposits a site may
//! have are rules and stay in the simulation, so the client links no rules.

pub use nomad_types::{Deposit, DepositKind, DepositValue, Richness};

/// At most this many deposits on one site (D-066).
pub const MAX_DEPOSITS_PER_SITE: usize = 2;

/// Value of a standard deposit of `kind`: ore 1, crystal 3 (D-066).
pub const fn base_value(kind: DepositKind) -> u64 {
    match kind {
        DepositKind::OreField => 1,
        DepositKind::CrystalVein => 3,
    }
}

/// The richness multiplier in thousandths: 500, 1,000 or 1,500 (D-066). Integer, so no
/// floating point enters values or, later, rates (D-049).
pub const fn richness_per_mille(richness: Richness) -> u64 {
    match richness {
        Richness::Poor => 500,
        Richness::Standard => 1_000,
        Richness::Rich => 1_500,
    }
}

/// The value of one deposit: base value × richness (D-066).
pub const fn deposit_value(deposit: Deposit) -> DepositValue {
    DepositValue::from_milli(base_value(deposit.kind) * richness_per_mille(deposit.richness))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_follow_d066() {
        let value = |d| deposit_value(d).milli();
        assert_eq!(value(Deposit::ore(Richness::Poor)), 500);
        assert_eq!(value(Deposit::ore(Richness::Standard)), 1_000);
        assert_eq!(value(Deposit::ore(Richness::Rich)), 1_500);
        assert_eq!(value(Deposit::crystal(Richness::Poor)), 1_500);
        assert_eq!(value(Deposit::crystal(Richness::Standard)), 3_000);
        assert_eq!(value(Deposit::crystal(Richness::Rich)), 4_500);
    }
}
