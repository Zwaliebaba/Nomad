//! Raid parameters: the `raids` part of the rules' `ai` section (Design A3 plan §4.2–§4.7,
//! §8 task 1; A1 plan §4.9).
//!
//! | Value | Source |
//! |---|---|
//! | A launch every 2–4 h per haven | D-081 |
//! | No commander the target of two launches within 6 h | D-081 |
//! | Target weight: deposit value + 1 per holding + 2 per outpost under construction, ÷ (1 + jumps from the haven) | A3 plan §4.3 (D-081: weighted by asset value and distance); starting values |
//! | Fleet value: alloy + 3 × crystal; strike craft 1,600, HSTL 17,000 | D-220 (D-066's ratio); starting values, tuned at α |
//! | Budget 30–50% of the target's fleet value in the system, at least 4 strike craft | D-081, D-220, D-313 |
//! | Withdrawal after 15 min from the end of the arrival shroud, at 50% losses by value, or when the sensed hostile value within 5 km exceeds twice the group's | D-081, D-222, D-313 |
//! | The director evaluates each group at 2 Hz | A1 plan §4.5 (D-203) |
//!
//! The section is copied into the Universe with the rest of the rules (D-104), so A6 can
//! tune raids with `--rules` and a season keeps the values it started with.

use serde::{Deserialize, Serialize};

use super::{RulesError, invalid};

/// The raid parameters (A3 plan §8 task 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RaidRules {
    /// The shortest interval between two launches of one haven (D-081: 2 h).
    pub interval_min_s: u64,
    /// The longest interval between two launches of one haven (D-081: 4 h).
    pub interval_max_s: u64,
    /// No commander is the target of a launch twice within this (D-081: 6 h).
    pub target_spacing_s: u64,
    /// What each of a commander's completed holdings in a system adds to its target weight,
    /// in thousandths of a deposit-value unit (A3 plan §4.3: 1).
    pub holding_weight_milli: u64,
    /// What each of a commander's outposts under construction in a system adds to its target
    /// weight, in thousandths of a deposit-value unit (A3 plan §4.3: 2).
    pub outpost_weight_milli: u64,
    /// A commander ship's value is its alloy cost plus this many times its crystal cost
    /// (D-220, D-066's ratio: 3).
    pub crystal_weight: u64,
    /// A raider strike craft's value (D-220: 1,600, about 70% of a LANCER).
    pub strike_craft_value: u64,
    /// A heavy cruiser's (HSTL's) value (D-220: 17,000, about three WARDENs).
    pub heavy_cruiser_value: u64,
    /// The smallest share of the target's fleet value a raid's budget draws, per mille
    /// (D-081: 30%).
    pub budget_min_permille: u32,
    /// The largest share of the target's fleet value a raid's budget draws, per mille
    /// (D-081: 50%).
    pub budget_max_permille: u32,
    /// Every raid has at least this many strike craft (D-313: 4, superseding D-220's 2).
    pub min_strike_craft: u32,
    /// A group withdraws this long after its arrival shroud ends in the target system
    /// (D-222; D-313: 15 min, superseding D-081's 10).
    pub clock_s: u64,
    /// A group withdraws once it has lost this share of its spawned value, per mille
    /// (D-081, D-222: 50%).
    pub loss_limit_permille: u32,
    /// A group withdraws early when the sensed hostile value near it exceeds this share of
    /// its remaining value, per mille (D-222: twice).
    pub withdraw_ratio_permille: u32,
    /// "Near it" for the early withdrawal (D-222: 5 km).
    pub withdraw_radius_m: u32,
    /// The director evaluates each raid group every this many ticks, staggered by the
    /// group's `SimId` (A1 plan §4.5: 2 Hz, every 5 ticks).
    pub director_interval_ticks: u64,
}

impl RaidRules {
    /// Checks every value.
    pub(super) fn check(&self) -> Result<(), RulesError> {
        let field = |name: &str| format!("ai.raids.{name}");
        for (name, value) in [
            ("interval_min_s", self.interval_min_s),
            ("target_spacing_s", self.target_spacing_s),
            ("strike_craft_value", self.strike_craft_value),
            ("heavy_cruiser_value", self.heavy_cruiser_value),
            ("clock_s", self.clock_s),
            ("director_interval_ticks", self.director_interval_ticks),
        ] {
            if value == 0 {
                return Err(invalid(field(name), "must be positive"));
            }
        }
        if self.interval_max_s < self.interval_min_s {
            return Err(invalid(
                field("interval_max_s"),
                "is below the shortest interval",
            ));
        }
        if self.budget_max_permille < self.budget_min_permille {
            return Err(invalid(
                field("budget_max_permille"),
                "is below the smallest share",
            ));
        }
        if self.budget_max_permille > 1_000 || self.loss_limit_permille > 1_000 {
            return Err(invalid(
                field("budget_max_permille or loss_limit_permille"),
                "must be at most 1000",
            ));
        }
        if self.min_strike_craft == 0 {
            // D-220's floor: a raid always has ships, so an undefended target is raided too.
            return Err(invalid(field("min_strike_craft"), "must be positive"));
        }
        if self.withdraw_radius_m == 0 {
            return Err(invalid(field("withdraw_radius_m"), "must be positive"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::rules::Rules;

    #[test]
    fn embedded_values_are_the_plans() {
        let raids = Rules::embedded().ai.raids;
        // D-081.
        assert_eq!(
            (raids.interval_min_s, raids.interval_max_s),
            (7_200, 14_400)
        );
        assert_eq!(raids.target_spacing_s, 21_600);
        assert_eq!(
            (raids.budget_min_permille, raids.budget_max_permille),
            (300, 500)
        );
        // D-313.
        assert_eq!(raids.clock_s, 900);
        assert_eq!(raids.loss_limit_permille, 500);
        // A3 plan §4.3.
        assert_eq!(
            (raids.holding_weight_milli, raids.outpost_weight_milli),
            (1_000, 2_000)
        );
        // D-220.
        assert_eq!(raids.crystal_weight, 3);
        assert_eq!(
            (raids.strike_craft_value, raids.heavy_cruiser_value),
            (1_600, 17_000)
        );
        // D-313.
        assert_eq!(raids.min_strike_craft, 4);
        // D-222.
        assert_eq!(raids.withdraw_ratio_permille, 2_000);
        assert_eq!(raids.withdraw_radius_m, 5_000);
        // A1 plan §4.5.
        assert_eq!(raids.director_interval_ticks, 5);
    }

    #[test]
    fn bad_values_are_rejected() {
        let rules = Rules::embedded();
        let mut r = rules.clone();
        r.ai.raids.interval_max_s = r.ai.raids.interval_min_s - 1;
        assert!(r.check().is_err());
        let mut r = rules.clone();
        r.ai.raids.min_strike_craft = 0;
        assert!(r.check().is_err());
        let mut r = rules.clone();
        r.ai.raids.budget_max_permille = 1_001;
        assert!(r.check().is_err());
        let mut r = rules.clone();
        r.ai.raids.budget_min_permille = 600;
        assert!(r.check().is_err());
        let mut r = rules;
        r.ai.raids.director_interval_ticks = 0;
        assert!(r.check().is_err());
    }
}
