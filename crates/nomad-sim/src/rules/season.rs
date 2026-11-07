//! Season rules (Design doc 10 §6; D-020, D-192; S8 plan §4.3).
//!
//! | Value | Source |
//! |---|---|
//! | Early win at 60% of the claimable systems, rounded up | D-020, doc 10 §6, D-192 |
//!
//! The share is in per mille, so the threshold stays integer (D-049). *Derived:* it must be
//! more than half, so that at most one commander can reach it (S8 plan §4.3).

use serde::{Deserialize, Serialize};

use super::{RulesError, invalid};

/// The season's win numbers (D-020, D-192).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeasonRules {
    /// A commander who controls this share of the claimable systems, rounded up, wins at once
    /// (D-020, D-192), in per mille.
    pub early_win_share_permille: u32,
}

impl SeasonRules {
    /// The early-win threshold for `claimable` systems: ⌈share × claimable⌉, at least 1, so
    /// a galaxy without claimable systems has no early win. 6 of 9 at stage 1.
    pub fn early_win_threshold(&self, claimable: usize) -> u32 {
        let claimable = u64::try_from(claimable).unwrap_or(u64::MAX);
        let threshold = claimable
            .saturating_mul(u64::from(self.early_win_share_permille))
            .div_ceil(1_000);
        u32::try_from(threshold).unwrap_or(u32::MAX).max(1)
    }

    /// Checks every value.
    pub(super) fn check(&self) -> Result<(), RulesError> {
        if self.early_win_share_permille <= 500 || self.early_win_share_permille > 1_000 {
            return Err(invalid(
                "season.early_win_share_permille".to_owned(),
                "must be more than 500 and at most 1000",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_threshold_rounds_up() {
        let rules = SeasonRules {
            early_win_share_permille: 600,
        };
        // Doc 10 §6: 6 of 9 at stage 1, and 12 of about 20 at stage 2.
        assert_eq!(rules.early_win_threshold(9), 6);
        assert_eq!(rules.early_win_threshold(20), 12);
        assert_eq!(rules.early_win_threshold(10), 6);
        assert_eq!(rules.early_win_threshold(1), 1);
        assert_eq!(rules.early_win_threshold(0), 1);
        // More than half of every count, so at most one commander reaches it.
        for n in 1..200 {
            assert!(2 * rules.early_win_threshold(n) as usize > n, "{n}");
        }
    }
}
