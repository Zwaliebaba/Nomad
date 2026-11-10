//! Blueprints: a slot assignment plus a build order (Design doc 14 §9.1; D-067, D-215; A2
//! plan §4.5).
//!
//! Shapes only. The AI's built-in blueprints are rules data, one per [`SiteKind`] (D-215),
//! and U3 saves and applies player blueprints with the same type (D-035). Which steps a
//! commander actually builds, and when, is the Strategist's choice (`nomad-ai`) or the
//! player's; whether a step fits a slot is the simulation's validator (D-077).

use serde::{Deserialize, Serialize};

use crate::economy::{ModuleKind, Slot};

/// What a site's deposits make it, for choosing a blueprint (D-215; A2 plan §4.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SiteKind {
    /// The commander's mother station.
    Mother,
    /// One ore field.
    Ore,
    /// Two ore fields.
    TwoOre,
    /// A crystal vein, and no ore field.
    Crystal,
    /// An ore field and a crystal vein.
    OreAndCrystal,
    /// No deposit.
    Empty,
}

impl SiteKind {
    /// The kind of a site other than a mother station with `ore` ore fields and `crystal`
    /// crystal veins (D-215; A2 plan §4.5).
    pub fn from_deposits(ore: usize, crystal: usize) -> Self {
        match (ore, crystal) {
            (0, 0) => Self::Empty,
            (0, _) => Self::Crystal,
            (1, 0) => Self::Ore,
            (_, 0) => Self::TwoOre,
            _ => Self::OreAndCrystal,
        }
    }
}

/// One step of a blueprint: a module in a slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct BlueprintStep {
    /// The module.
    pub kind: ModuleKind,
    /// Its slot.
    pub slot: Slot,
}

/// A slot assignment plus a build order (D-067): the steps, in the order they are built.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Blueprint {
    /// The steps, in build order.
    pub steps: Vec<BlueprintStep>,
}
