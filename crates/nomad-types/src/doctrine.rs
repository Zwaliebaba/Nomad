//! Doctrine: the standing orders a commander writes for their Deputy (Design doc 12 §5.2,
//! doc 13 §9; D-013, D-082, D-092; A1 plan §5.1).
//!
//! Shapes only. What a doctrine command may set, how a preset and its overrides resolve and
//! what the Deputy does with them are rules for `nomad-sim` and `nomad-ai`. The types live
//! here because they travel on the wire (N4 sends a commander their own doctrine, U4 edits
//! it), and the AI reads them through its view (D-199).
//!
//! **The shape** *(Proposed, A1 plan §5.1):* a commander's doctrine is a [`DoctrinePreset`]
//! plus optional overrides per holding ([`HoldingDoctrine`]) and per fleet
//! ([`FleetDoctrine`]), and the replacement list ([`ReplacementEntry`]). An override is
//! whole: it replaces the preset's values for that holding or fleet, and setting it to
//! `None` follows the preset again. Distances are whole metres and thresholds per mille, so
//! doctrine compares exactly (D-049).

use serde::{Deserialize, Serialize};

use crate::command::RulesOfEngagement;
use crate::economy::PowerProfile;
use crate::id::{FleetId, HoldingId};
use crate::position::Location;
use crate::ship::ShipClass;

/// The doctrine presets of doc 13 §9 (D-082). Balanced is the default.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub enum DoctrinePreset {
    /// Strong defence, generous rebuilding.
    Fortress,
    /// The default for new commanders and the Strategist's starting point.
    #[default]
    Balanced,
    /// Light defence, convoys that keep running.
    Trader,
}

/// A holding's doctrine (doc 12 §5.2 "Holding doctrine", doc 13 §9).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HoldingDoctrine {
    /// The defended zone's radius around the site, metres.
    pub zone_radius_m: u32,
    /// Rules of engagement in the zone: weapons free, return fire or hold.
    pub zone_rules: RulesOfEngagement,
    /// The power profile while the holding is on alert.
    pub alert_profile: PowerProfile,
    /// SHLD SURGE when a sector falls below this share of its maximum, per mille.
    pub surge_below_permille: u32,
    /// FLAK SCREEN when this many light hostiles are within 3 km of the dome; `None` for
    /// never.
    pub flak_light_hostiles: Option<u32>,
    /// Dock harvesting craft and hold convoys while hostiles are within this distance,
    /// metres.
    pub evacuation_radius_m: u32,
    /// Rebuild destroyed modules in the same slot (D-208; on by default).
    pub rebuild_modules: bool,
}

/// A fleet's role (doc 12 §5.2; D-207).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum FleetRole {
    /// Docked at the holding; launches when it is on alert.
    Garrison {
        /// The holding.
        holding: HoldingId,
    },
    /// Docked at the holding; never launched by the Deputy.
    Reserve {
        /// The holding.
        holding: HoldingId,
    },
    /// Patrols a route of points in one system.
    Patrol {
        /// The route, in order.
        route: Vec<Location>,
    },
    /// Guards another own fleet.
    Escort {
        /// The fleet it escorts.
        fleet: FleetId,
    },
}

/// A fleet's doctrine (doc 12 §5.2 "Fleet doctrine", doc 13 §9).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FleetDoctrine {
    /// Its role; `None` keeps the default role (D-207: the garrison of the holding that
    /// built it).
    pub role: Option<FleetRole>,
    /// How far from its anchor it pursues, metres.
    pub leash_m: u32,
    /// It retreats to dock when its hull falls below this share, per mille.
    pub retreat_below_permille: u32,
    /// It docks to repair when damaged and not needed (D-071).
    pub repair_at_dock: bool,
}

/// One entry of the replacement list: keep `fleet` at `count` ships of `class` (doc 12
/// §5.1, D-016, D-208).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReplacementEntry {
    /// The fleet to keep up.
    pub fleet: FleetId,
    /// The class to replace.
    pub class: ShipClass,
    /// How many ships of the class the fleet should have.
    pub count: u32,
    /// The role of the new fleet a replacement forms if `fleet` no longer exists; `None`
    /// for the default role (A1 plan §5.7).
    pub role: Option<FleetRole>,
}

/// What a preset sets (doc 13 §9; D-082), as the rules file gives it (D-211).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DoctrineValues {
    /// Every holding's doctrine without an override.
    pub holding: HoldingDoctrine,
    /// The leash of fleets without an override, metres.
    pub leash_m: u32,
    /// The retreat threshold of fleets without an override, per mille of hull.
    pub retreat_below_permille: u32,
    /// Whether fleets without an override repair at dock.
    pub repair_at_dock: bool,
    /// The replacement budget: this share of each resource's income over the last 24 h,
    /// per mille (D-208).
    pub replacement_budget_permille: u32,
    /// The share of the fleet kept as garrison, per mille: a Strategist parameter (D-209).
    pub garrison_share_permille: u32,
}

impl DoctrineValues {
    /// The fleet doctrine of a fleet without an override.
    pub const fn fleet(&self) -> FleetDoctrine {
        FleetDoctrine {
            role: None,
            leash_m: self.leash_m,
            retreat_below_permille: self.retreat_below_permille,
            repair_at_dock: self.repair_at_dock,
        }
    }
}

/// Doctrine commands (doc 12 §5.2; A1 plan §5.1, §5.2). Changing doctrine is the
/// commander's own initiative (PR-3): the validator refuses these from the Deputy.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum DoctrineCommand {
    /// Chooses the preset.
    SetPreset {
        /// The preset.
        preset: DoctrinePreset,
    },
    /// Overrides a holding's doctrine, or with `None` follows the preset again.
    SetHolding {
        /// The holding.
        holding: HoldingId,
        /// Its doctrine.
        doctrine: Option<HoldingDoctrine>,
    },
    /// Overrides a fleet's doctrine, or with `None` follows the preset again.
    SetFleet {
        /// The fleet.
        fleet: FleetId,
        /// Its doctrine.
        doctrine: Option<FleetDoctrine>,
    },
    /// Replaces the replacement list.
    SetReplacement {
        /// The entries, in order.
        entries: Vec<ReplacementEntry>,
    },
    /// Ends direct control of a fleet: its doctrine applies again (RK-10).
    Release {
        /// The fleet.
        fleet: FleetId,
    },
}
