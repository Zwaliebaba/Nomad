//! The Strategist's rules values: the rules file's `ai.strategist` section (D-211; A1 plan
//! §4.9, A2 plan §4.1, §4.5).
//!
//! The simulation copies them into the Universe with the rest of the rules (D-104), so a
//! season keeps its AI and replay stays exact, and A6 tunes them with `--rules`. The
//! hysteresis values are the plan's (5 min, +20%, doc 13 §12 item 4); the step timeout is
//! D-083 item 6's 10 min. The score weights, the blueprints and the rest are *Proposed*
//! starting values for A6 to tune (A2 plan §4.3–§4.7, §10 item 5).
//!
//! **Scores** are integers on one scale, "per million of the cost returned per hour": a
//! productive step worth `v` per hour for a cost of `c` scores `1,000,000 × v ÷ c` (an
//! extractor: about 72,000). Terms that are not income, such as control or defence, are
//! weights on the same scale.

use std::collections::BTreeMap;

use nomad_types::{Blueprint, ShipClass, SiteKind};
use serde::{Deserialize, Serialize};

/// The Strategist's section of the rules (D-211).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StrategistRules {
    /// A plan, once started, is kept at least this long (doc 13 §12 item 4).
    pub min_commitment_s: u64,
    /// A goal with a running plan scores this much more, per mille (doc 13 §12 item 4).
    pub hysteresis_bonus_permille: u32,
    /// A plan step without progress for this long is abandoned (D-083 item 6).
    pub step_timeout_s: u64,
    /// At most this many plans run at once (A2 plan §4.1: the best three goals).
    pub max_plans: usize,
    /// At most this many sites are scored per evaluation (A2 plan §4.4, §4.10).
    pub max_candidates: usize,
    /// A trip is estimated at its route time times this, per mille (A2 plan §4.8: 1.2).
    pub trip_margin_permille: u32,
    /// Added to a trip's estimate for loading and deploying, seconds.
    pub deploy_margin_s: u64,
    /// The sign-off sends at most this many commands per evaluation, so the command rate
    /// drains them before they expire (D-201).
    pub sign_off_commands: usize,
    /// The weight of crystal against alloy in values (D-066: 3).
    pub crystal_weight: u64,
    /// Score weights.
    pub weights: Weights,
    /// The Garrison stand-in's composition by value, per mille (A2 plan §4.7).
    pub composition_permille: BTreeMap<ShipClass, u32>,
    /// One blueprint per site kind (D-215).
    pub blueprints: BTreeMap<SiteKind, Blueprint>,
    /// The generic order Easy uses for every site (D-215).
    pub generic_blueprint: Blueprint,
}

/// The weights of score terms that are not income (A2 plan §4.3–§4.7), on the score scale.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Weights {
    /// A site that moves the commander towards a reachable majority in its system (D-041).
    pub control_toward: i64,
    /// A site that completes a majority.
    pub control_complete: i64,
    /// A site in a system where a rival is one site short of a majority.
    pub control_deny: i64,
    /// Per hour of the kit's route (exposure in transit).
    pub risk_per_route_hour: i64,
    /// Per hostile ship detected or remembered in the site's system or on the route.
    pub risk_per_hostile: i64,
    /// Per rival holding in the site's system.
    pub risk_per_rival_holding: i64,
    /// For a site one jump from a raider haven; halved per further jump.
    pub risk_haven: i64,
    /// Above this risk an expansion takes an escort (A2 plan §4.4 step 5).
    pub escort_above: i64,
    /// A pylon.
    pub pylon: i64,
    /// A turret.
    pub turret: i64,
    /// The forward holding's dry dock.
    pub dry_dock: i64,
    /// A garrison ship (A2 plan §4.7: the lowest, when nothing else is worth buying).
    pub garrison: i64,
    /// A crystal vein while crystal income is zero: above everything (A2 plan §4.4).
    pub crystal_first: i64,
}

/// How a difficulty chooses its build orders (D-079, D-215).
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub enum BlueprintMode {
    /// One generic order for every site (Easy).
    Generic,
    /// The blueprint of each site kind (Normal).
    #[default]
    PerSiteKind,
    /// Per site kind, with turrets first where raids have struck (Hard).
    PerSiteKindRaidTurrets,
}

impl StrategistRules {
    /// The blueprint for a site kind; the generic one if the kind has none.
    pub fn blueprint(&self, kind: SiteKind, mode: BlueprintMode) -> &Blueprint {
        match mode {
            BlueprintMode::Generic => &self.generic_blueprint,
            BlueprintMode::PerSiteKind | BlueprintMode::PerSiteKindRaidTurrets => self
                .blueprints
                .get(&kind)
                .unwrap_or(&self.generic_blueprint),
        }
    }
}
