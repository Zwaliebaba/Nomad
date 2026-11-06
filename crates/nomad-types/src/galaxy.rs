//! Kinds of galaxy object (Design doc 10 §4; D-042, D-043). Galaxy topology is public
//! (D-044, doc 16 §2), so clients receive these.

use serde::{Deserialize, Serialize};

/// The kinds of system (doc 10 §4.1, D-043).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SystemKind {
    /// 7 sites; one holds a commander's mother station at season start.
    Home,
    /// 5–7 sites.
    Standard,
    /// 8–9 sites with richer deposits; the contested centre of a sector. Up to 5 gates.
    Hub,
    /// NPC raiders operate from here. Not claimable in the MVP and not counted for control.
    RaiderHaven,
}

impl SystemKind {
    /// Whether commanders can claim sites here and the system counts for control
    /// (doc 10 §4.1, §6).
    pub const fn is_claimable(self) -> bool {
        !matches!(self, Self::RaiderHaven)
    }
}

/// The kinds of lane between gates (doc 10 §4.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum LaneKind {
    /// Between systems of one sector; 40–70 min.
    Jump,
    /// Between border systems of neighbouring sectors; 90–150 min.
    Trunk,
}
