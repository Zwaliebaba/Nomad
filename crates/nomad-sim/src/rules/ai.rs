//! AI parameters (Design D-211; A1 plan §4.9).
//!
//! | Value | Source |
//! |---|---|
//! | Doctrine presets Fortress, Balanced and Trader | Doc 13 §9 (D-082), starting values |
//! | Difficulty: latency 10, 6 or 3 ticks; 1, 2 or 4 commands in any 10 ticks; Strategist every 30, 15 or 10 s | D-079, D-201, D-229 |
//! | A human's Deputy: 3 ticks, 4 commands in any 10 ticks | Doc 12 §5.4, D-201 |
//! | Deputy at 2 Hz (every 5 ticks) | Doc 11 §4.1, D-203 |
//! | Inbox kept 60 s; waiting commands expire after 10 s | D-202, D-201 |
//! | Repair at dock on for every preset | *Proposed*: doc 13 §9 gives no value, and the A1 plan §5.6 describes repair at dock as the default behaviour |
//! | At most 32 replacement-list entries | *Proposed*: a bound on input from the network, no design value |
//!
//! The section is copied into the Universe with the rest of the rules (D-104), so a season
//! keeps its AI and replay stays exact; A6 tunes it with `--rules`. A2 adds the Strategist's
//! hysteresis and noise and the blueprints, A3 the raid parameters.

use std::collections::BTreeMap;

use nomad_types::{Difficulty, DoctrinePreset, DoctrineValues};
use serde::{Deserialize, Serialize};

use super::{RulesError, invalid};

/// The AI section of the rules (D-211).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AiRules {
    /// What each doctrine preset sets (doc 13 §9, D-082).
    pub presets: BTreeMap<DoctrinePreset, DoctrineValues>,
    /// The competence of AI commanders by difficulty (D-079, D-201).
    pub difficulty: BTreeMap<Difficulty, DifficultyRules>,
    /// The competence of a human commander's Deputy (doc 12 §5.4, D-201).
    pub human_deputy: Competence,
    /// The command rate holds in any window of this many consecutive ticks (D-201).
    pub rate_window_ticks: u64,
    /// The Deputy evaluates each holding and fleet every this many ticks, staggered by
    /// `SimId` (D-203: 2 Hz).
    pub deputy_interval_ticks: u64,
    /// Inbox entries are kept this long (D-202).
    pub inbox_retention_s: u64,
    /// A due AI command still waiting for the command rate after this long is dropped
    /// (D-201).
    pub command_expiry_s: u64,
    /// The longest replacement list a commander may set.
    pub max_replacement_entries: u32,
}

/// One difficulty's numbers (D-079).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DifficultyRules {
    /// Reaction latency and command rate, for every AI layer of the commander.
    pub competence: Competence,
    /// The Strategist evaluates this often (D-079, D-229).
    pub strategist_interval_s: u64,
}

/// The competence cap (doc 12 §5.4, doc 13 §4 item 6; D-201).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Competence {
    /// An AI command applies at least this many ticks after the event that triggered it.
    pub latency_ticks: u64,
    /// At most this many of the commander's AI commands apply in any rate window.
    pub commands_per_window: u32,
}

const PRESETS: [DoctrinePreset; 3] = [
    DoctrinePreset::Fortress,
    DoctrinePreset::Balanced,
    DoctrinePreset::Trader,
];
const DIFFICULTIES: [Difficulty; 3] = [Difficulty::Easy, Difficulty::Normal, Difficulty::Hard];

impl AiRules {
    /// What `preset` sets.
    pub fn preset(&self, preset: DoctrinePreset) -> &DoctrineValues {
        self.presets
            .get(&preset)
            .expect("invariant: checked rules have every preset")
    }

    /// The numbers of `difficulty`.
    pub fn difficulty(&self, difficulty: Difficulty) -> &DifficultyRules {
        self.difficulty
            .get(&difficulty)
            .expect("invariant: checked rules have every difficulty")
    }

    /// Checks every value.
    pub(super) fn check(&self) -> Result<(), RulesError> {
        for preset in PRESETS {
            let Some(values) = self.presets.get(&preset) else {
                return Err(invalid(format!("ai.presets.{preset:?}"), "is missing"));
            };
            check_preset(preset, values)?;
        }
        for difficulty in DIFFICULTIES {
            let Some(rules) = self.difficulty.get(&difficulty) else {
                return Err(invalid(
                    format!("ai.difficulty.{difficulty:?}"),
                    "is missing",
                ));
            };
            check_competence(&format!("ai.difficulty.{difficulty:?}"), rules.competence)?;
            if rules.strategist_interval_s == 0 {
                return Err(invalid(
                    format!("ai.difficulty.{difficulty:?}.strategist_interval_s"),
                    "must be positive",
                ));
            }
        }
        check_competence("ai.human_deputy", self.human_deputy)?;
        for (field, value) in [
            ("ai.rate_window_ticks", self.rate_window_ticks),
            ("ai.deputy_interval_ticks", self.deputy_interval_ticks),
            ("ai.inbox_retention_s", self.inbox_retention_s),
            ("ai.command_expiry_s", self.command_expiry_s),
        ] {
            if value == 0 {
                return Err(invalid(field.to_owned(), "must be positive"));
            }
        }
        Ok(())
    }
}

fn check_competence(field: &str, competence: Competence) -> Result<(), RulesError> {
    if competence.latency_ticks == 0 {
        // A command never applies on the tick it was issued (doc 11 §4.1).
        return Err(invalid(
            format!("{field}.latency_ticks"),
            "must be positive",
        ));
    }
    if competence.commands_per_window == 0 {
        return Err(invalid(
            format!("{field}.commands_per_window"),
            "must be positive",
        ));
    }
    Ok(())
}

fn check_preset(preset: DoctrinePreset, values: &DoctrineValues) -> Result<(), RulesError> {
    let field = |name: &str| format!("ai.presets.{preset:?}.{name}");
    for (name, permille) in [
        (
            "holding.surge_below_permille",
            values.holding.surge_below_permille,
        ),
        ("retreat_below_permille", values.retreat_below_permille),
        (
            "replacement_budget_permille",
            values.replacement_budget_permille,
        ),
        ("garrison_share_permille", values.garrison_share_permille),
    ] {
        if permille > 1_000 {
            return Err(invalid(field(name), "must be at most 1000"));
        }
    }
    if values.holding.zone_radius_m == 0 || values.holding.evacuation_radius_m == 0 {
        return Err(invalid(field("holding"), "radii must be positive"));
    }
    if values.holding.flak_light_hostiles == Some(0) {
        return Err(invalid(
            field("holding.flak_light_hostiles"),
            "must be positive or null",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use nomad_types::{PowerProfile, RulesOfEngagement};

    use super::*;
    use crate::rules::Rules;

    #[test]
    fn embedded_values_are_doc_13s() {
        let rules = Rules::embedded();
        let ai = &rules.ai;
        // Doc 13 §9 (D-082).
        let fortress = ai.preset(DoctrinePreset::Fortress);
        assert_eq!(fortress.holding.zone_radius_m, 15_000);
        assert_eq!(fortress.holding.surge_below_permille, 400);
        assert_eq!(fortress.holding.flak_light_hostiles, Some(6));
        assert_eq!(fortress.leash_m, 8_000);
        assert_eq!(fortress.replacement_budget_permille, 500);
        let balanced = ai.preset(DoctrinePreset::Balanced);
        assert_eq!(balanced.holding.zone_rules, RulesOfEngagement::WeaponsFree);
        assert_eq!(balanced.holding.alert_profile, PowerProfile::ShieldsFirst);
        assert_eq!(balanced.holding.evacuation_radius_m, 20_000);
        assert_eq!(balanced.retreat_below_permille, 400);
        assert_eq!(balanced.leash_m, 12_000);
        assert_eq!(balanced.garrison_share_permille, 500);
        let trader = ai.preset(DoctrinePreset::Trader);
        assert_eq!(trader.holding.zone_rules, RulesOfEngagement::ReturnFire);
        assert_eq!(trader.holding.alert_profile, PowerProfile::Balanced);
        assert_eq!(trader.holding.flak_light_hostiles, None);
        assert_eq!(trader.replacement_budget_permille, 150);
        // D-079, D-201.
        let normal = ai.difficulty(Difficulty::Normal);
        assert_eq!(normal.competence.latency_ticks, 6);
        assert_eq!(normal.competence.commands_per_window, 2);
        assert_eq!(normal.strategist_interval_s, 15);
        assert_eq!(ai.difficulty(Difficulty::Easy).competence.latency_ticks, 10);
        assert_eq!(
            ai.difficulty(Difficulty::Hard)
                .competence
                .commands_per_window,
            4
        );
        assert_eq!(ai.human_deputy.latency_ticks, 3);
        assert_eq!(ai.human_deputy.commands_per_window, 4);
        assert_eq!(ai.rate_window_ticks, 10);
        assert_eq!(ai.deputy_interval_ticks, 5);
    }

    #[test]
    fn a_missing_preset_or_zero_rate_is_rejected() {
        let mut rules = Rules::embedded();
        rules.ai.presets.remove(&DoctrinePreset::Trader);
        assert!(rules.check().is_err());
        let mut rules = Rules::embedded();
        rules.ai.human_deputy.commands_per_window = 0;
        assert!(rules.check().is_err());
        let mut rules = Rules::embedded();
        if let Some(values) = rules.ai.presets.get_mut(&DoctrinePreset::Balanced) {
            values.retreat_below_permille = 1_001;
        }
        assert!(rules.check().is_err());
    }
}
