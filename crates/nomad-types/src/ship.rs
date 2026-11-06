//! Ship classes (Design doc 15 §3.1, D-052). What a class can do is a rule and lives in the
//! simulation's rules file (D-104); clients only need the name.

use serde::{Deserialize, Serialize};

/// The MVP ship classes: the phase-1 placeholder classes (doc 15 §3.1, D-052).
///
/// In text formats such as the rules file the names are written in capitals (`"VX"`,
/// `"RAIDER_STRIKE_CRAFT"`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ShipClass {
    /// VX interceptor: light, space superiority and screening.
    Vx,
    /// LANCER strike craft: light, torpedoes against ships, structures and domes.
    Lancer,
    /// WARDEN frigate: medium line ship with flak and a main battery.
    Warden,
    /// MULE hauler: medium logistics craft, unarmed.
    Mule,
    /// NPC raider strike craft.
    RaiderStrikeCraft,
    /// NPC HSTL heavy cruiser.
    HeavyCruiser,
}

impl ShipClass {
    /// Every class, in declaration order.
    pub const ALL: [Self; 6] = [
        Self::Vx,
        Self::Lancer,
        Self::Warden,
        Self::Mule,
        Self::RaiderStrikeCraft,
        Self::HeavyCruiser,
    ];
}
