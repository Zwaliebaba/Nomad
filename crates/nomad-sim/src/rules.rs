//! Rules data: unit and movement numbers (Design D-104; S3 plan §4.1).
//!
//! The numbers live in a **rules file**, `rules/default.json`, embedded in the build. A
//! harness option may load another file for tuning runs. At Universe creation the rules are
//! **copied into the [`UniverseState`](crate::UniverseState)**, so a season keeps its rules
//! even if a later build changes the defaults (D-020), and snapshot replay stays exact
//! (D-049).
//!
//! Where each number comes from:
//!
//! | Section | Source |
//! |---|---|
//! | Class speeds, weapon ranges | Doc 15 §12, starting values (D-058) |
//! | Acceleration, turn rate | D-101 (Q92), starting values |
//! | `frigate` | Doc 15 §3.1: the WARDEN; frigates dock only where there is a dry dock (D-102) |
//! | `npc` | Doc 15 §3.1: raider classes, which are not commander warships (D-099 item 5) |
//! | Ops-area radius | D-103 (Q94) |
//! | Formation spacing, neighbour cells | S3 plan §4.3 |
//! | Gates | D-055: 2 km scatter, 5 s spool-up, 10 s shroud; S3 plan §4.4: within 2 km |
//! | Docking | D-102 (Q93) |
//! | Start kit | Doc 14 §12 (*Proposed* there): 3 VX and 2 MULE, one dry dock |
//! | Cruise drop range | Doc 15 §3.3: a hostile within 20 km (a hook until S7) |
//! | Mother-station layout | The phase-1 station (`station_scene.json` module bounds, `ships_placement.json` CAP WING), 1 voxel ≈ 35 m and grid north at azimuth 225° (doc 10 §8, D-046) |
//!
//! *Proposed* (no agreed value; tuning numbers of this implementation): separation distance
//! and response time, obstacle margin, look-ahead time (D-105 fixes the behaviour, not the
//! numbers), the guard radius (S3 plan §4.4 names it without a value) and the launch point.

use std::collections::BTreeMap;

use nomad_types::ShipClass;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The format tag a rules file must carry (`CODING_STANDARDS.md` §9: unknown formats are
/// rejected).
pub const RULES_FORMAT: &str = "nomad-rules@1";

/// The rules file embedded in the build.
pub const DEFAULT_RULES_JSON: &str = include_str!("../rules/default.json");

/// Why a rules file was rejected.
#[derive(Debug, Error)]
pub enum RulesError {
    /// The text is not valid JSON of the rules shape.
    #[error("rules file does not parse: {0}")]
    Parse(#[from] serde_json::Error),
    /// The `format` tag is not [`RULES_FORMAT`].
    #[error("unsupported rules format {0:?} (this build reads {RULES_FORMAT:?})")]
    UnknownFormat(String),
    /// A ship class has no entry.
    #[error("ship class {0:?} has no rules")]
    MissingClass(ShipClass),
    /// A value is out of range.
    #[error("rules value {field} is invalid: {reason}")]
    Invalid {
        /// Which value.
        field: String,
        /// What is wrong with it.
        reason: &'static str,
    },
}

/// All unit and movement numbers of a season (D-104).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rules {
    /// Always [`RULES_FORMAT`].
    pub format: String,
    /// Per-class movement and engagement numbers.
    pub ship_classes: BTreeMap<ShipClass, ShipClassRules>,
    /// Movement, formations, obstacles and separation.
    pub movement: MovementRules,
    /// Gate departure and arrival.
    pub gates: GateRules,
    /// Docking capacity.
    pub docking: DockingRules,
    /// What each commander starts with.
    pub start_kit: StartKit,
    /// The mother station's layout, for obstacles and the launch point.
    pub mother_station: StationLayout,
}

/// One ship class (doc 15 §3.3, §12; D-101).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShipClassRules {
    /// Top speed in tactical mode.
    pub tactical_speed_m_per_s: f64,
    /// Top speed in cruise mode (doc 15 §3.3).
    pub cruise_speed_m_per_s: f64,
    /// Largest change of velocity per second.
    pub acceleration_m_per_s2: f64,
    /// Largest change of heading per second.
    pub turn_rate_deg_per_s: f64,
    /// The class weapon range; `None` for unarmed classes such as the MULE. Where a class
    /// has several weapons this is the longest, the WARDEN's battery.
    pub weapon_range_m: Option<f64>,
    /// A frigate docks only at holdings with a dry dock (D-102).
    pub frigate: bool,
    /// An NPC class, which is not a commander warship (D-099 item 5).
    pub npc: bool,
}

impl ShipClassRules {
    /// The turn rate in radians per second.
    pub fn turn_rate_rad_per_s(&self) -> f64 {
        self.turn_rate_deg_per_s.to_radians()
    }
}

/// Movement, formation and obstacle numbers (S3 plan §4.3; D-103, D-105).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MovementRules {
    /// Radius of a site's ops area. Cruise mode is allowed only outside every ops area
    /// (D-103).
    pub ops_area_radius_m: f64,
    /// Spacing of the default compact-grid formation.
    pub formation_spacing_m: f64,
    /// Cell size of the per-system grid for neighbour queries.
    pub neighbour_cell_m: f64,
    /// Ships closer than this push each other apart (soft separation, D-105). *Proposed;*
    /// smaller than the formation spacing, so it never fights a formation.
    pub separation_m: f64,
    /// How quickly separation resolves an overlap: the push speed is the overlap divided by
    /// this time. *Proposed.*
    pub separation_response_s: f64,
    /// Steering keeps this clearance from obstacle footprints. *Proposed.*
    pub obstacle_margin_m: f64,
    /// How far ahead, in seconds of travel, steering looks for obstacles. *Proposed.*
    pub look_ahead_s: f64,
    /// A guarding fleet stays within this distance of what it guards. *Proposed;* the S3
    /// plan names the radius without a value.
    pub guard_radius_m: f64,
    /// A hostile detected within this range drops a fleet out of cruise mode (doc 15 §3.3).
    /// A hook for S7.
    pub cruise_drop_hostile_range_m: f64,
}

/// Gate departure and arrival (D-055; S3 plan §4.4).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateRules {
    /// A ship spools up once it is this close to the gate, and the spool-up is cancelled if
    /// it moves farther away.
    pub jump_range_m: f64,
    /// Spool-up before a ship enters the lane.
    pub spool_up_s: u64,
    /// Arriving ships appear up to this far from the destination gate.
    pub arrival_scatter_m: f64,
    /// Jump shroud after arrival: the ship cannot be targeted and cannot fire.
    pub shroud_s: u64,
}

/// Docking capacity in ships (D-102).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DockingRules {
    /// A mother-station core.
    pub mother_station_core_ships: u32,
    /// Any other core.
    pub core_ships: u32,
    /// Added by each dry dock.
    pub dry_dock_ships: u32,
}

/// What each commander starts with (doc 14 §12).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartKit {
    /// The ships of the starting fleet, docked at the mother station.
    pub fleet: BTreeMap<ShipClass, u32>,
    /// Dry docks of the mother station, for docking capacity.
    pub mother_station_dry_docks: u32,
}

/// The mother station's layout in phase-1 grid voxels (phase-1 doc 03; doc 10 §8).
///
/// Grid axes are `x`, `y` horizontal and `z` up, with the origin at the core. Grid north is
/// system north (D-046), at the given azimuth in grid xy.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StationLayout {
    /// Metres per voxel (doc 10 §8: ≈ 35 m).
    pub voxel_m: f64,
    /// Height of the single ship plane (D-003).
    pub ship_plane_z_voxels: i32,
    /// Azimuth of grid north in grid xy, degrees from +x towards +y (D-046).
    pub grid_north_azimuth_deg: f64,
    /// Where launched ships appear, in grid xy. *Proposed:* the centre of the phase-1 CAP
    /// WING, inside the dome and clear of every obstacle.
    pub launch_point_voxels: [f64; 2],
    /// The station modules, with their bounds.
    pub modules: Vec<ModuleShape>,
}

/// One module's bounds in grid voxels, both corners inclusive (phase-1 `station_scene.json`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleShape {
    /// The phase-1 module id.
    pub id: String,
    /// The lowest corner.
    pub min_voxels: [i32; 3],
    /// The highest corner.
    pub max_voxels: [i32; 3],
}

impl Rules {
    /// The rules embedded in the build.
    pub fn embedded() -> Self {
        Self::from_json(DEFAULT_RULES_JSON)
            .expect("invariant: the embedded rules file is valid (checked by a unit test)")
    }

    /// Parses and checks a rules file. Nothing in it can cause a panic: anything malformed
    /// or out of range is an error (`CODING_STANDARDS.md` §5).
    pub fn from_json(text: &str) -> Result<Self, RulesError> {
        let rules: Self = serde_json::from_str(text)?;
        rules.check()?;
        Ok(rules)
    }

    /// The rules of `class`.
    pub fn class(&self, class: ShipClass) -> &ShipClassRules {
        self.ship_classes
            .get(&class)
            .expect("invariant: checked rules have every ship class")
    }

    /// The slowest commander warship by cruise speed, the class invariant 3 is checked for
    /// (doc 10 §4.3, D-099 item 5): armed, not an NPC class. Ties go to the first class.
    pub fn slowest_commander_warship(&self) -> ShipClass {
        let mut slowest: Option<(ShipClass, f64)> = None;
        for (&class, rules) in &self.ship_classes {
            if rules.npc || rules.weapon_range_m.is_none() {
                continue;
            }
            if slowest.is_none_or(|(_, speed)| rules.cruise_speed_m_per_s < speed) {
                slowest = Some((class, rules.cruise_speed_m_per_s));
            }
        }
        slowest
            .map(|(class, _)| class)
            .expect("invariant: checked rules have a commander warship")
    }

    /// Checks every value. [`Rules::from_json`] calls it; call it after changing a value.
    pub fn check(&self) -> Result<(), RulesError> {
        if self.format != RULES_FORMAT {
            return Err(RulesError::UnknownFormat(self.format.clone()));
        }
        for class in ShipClass::ALL {
            let rules = self
                .ship_classes
                .get(&class)
                .ok_or(RulesError::MissingClass(class))?;
            let name = |field: &str| format!("ship_classes.{class:?}.{field}");
            positive(
                &name("tactical_speed_m_per_s"),
                rules.tactical_speed_m_per_s,
            )?;
            positive(&name("cruise_speed_m_per_s"), rules.cruise_speed_m_per_s)?;
            positive(&name("acceleration_m_per_s2"), rules.acceleration_m_per_s2)?;
            positive(&name("turn_rate_deg_per_s"), rules.turn_rate_deg_per_s)?;
            if rules.cruise_speed_m_per_s < rules.tactical_speed_m_per_s {
                return Err(invalid(
                    name("cruise_speed_m_per_s"),
                    "is below the tactical speed",
                ));
            }
            if let Some(range) = rules.weapon_range_m {
                positive(&name("weapon_range_m"), range)?;
            }
        }
        if !self
            .ship_classes
            .values()
            .any(|c| !c.npc && c.weapon_range_m.is_some())
        {
            return Err(invalid(
                "ship_classes".to_owned(),
                "has no armed commander class",
            ));
        }

        let m = &self.movement;
        for (field, value) in [
            ("ops_area_radius_m", m.ops_area_radius_m),
            ("formation_spacing_m", m.formation_spacing_m),
            ("neighbour_cell_m", m.neighbour_cell_m),
            ("separation_m", m.separation_m),
            ("separation_response_s", m.separation_response_s),
            ("look_ahead_s", m.look_ahead_s),
            ("guard_radius_m", m.guard_radius_m),
            ("cruise_drop_hostile_range_m", m.cruise_drop_hostile_range_m),
        ] {
            positive(&format!("movement.{field}"), value)?;
        }
        non_negative("movement.obstacle_margin_m", m.obstacle_margin_m)?;
        if m.separation_m >= m.formation_spacing_m {
            return Err(invalid(
                "movement.separation_m".to_owned(),
                "must be smaller than the formation spacing",
            ));
        }
        if m.neighbour_cell_m < m.separation_m {
            return Err(invalid(
                "movement.neighbour_cell_m".to_owned(),
                "must be at least the separation distance",
            ));
        }

        let g = &self.gates;
        positive("gates.jump_range_m", g.jump_range_m)?;
        positive("gates.arrival_scatter_m", g.arrival_scatter_m)?;

        let layout = &self.mother_station;
        positive("mother_station.voxel_m", layout.voxel_m)?;
        finite(
            "mother_station.grid_north_azimuth_deg",
            layout.grid_north_azimuth_deg,
        )?;
        for (i, &value) in layout.launch_point_voxels.iter().enumerate() {
            finite(&format!("mother_station.launch_point_voxels[{i}]"), value)?;
        }
        for module in &layout.modules {
            if (0..3).any(|axis| module.min_voxels[axis] > module.max_voxels[axis]) {
                return Err(invalid(
                    format!("mother_station.modules.{}", module.id),
                    "has a minimum corner above its maximum",
                ));
            }
        }
        Ok(())
    }
}

fn invalid(field: String, reason: &'static str) -> RulesError {
    RulesError::Invalid { field, reason }
}

fn finite(field: &str, value: f64) -> Result<(), RulesError> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(invalid(field.to_owned(), "is not a finite number"))
    }
}

fn positive(field: &str, value: f64) -> Result<(), RulesError> {
    finite(field, value)?;
    if value > 0.0 {
        Ok(())
    } else {
        Err(invalid(field.to_owned(), "must be positive"))
    }
}

fn non_negative(field: &str, value: f64) -> Result<(), RulesError> {
    finite(field, value)?;
    if value >= 0.0 {
        Ok(())
    } else {
        Err(invalid(field.to_owned(), "must not be negative"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "the rules file holds whole numbers, which parse exactly"
    )]
    fn embedded_rules_parse_and_hold_the_agreed_values() {
        let rules = Rules::embedded();
        // Doc 15 §12 (D-058) and D-101.
        let vx = rules.class(ShipClass::Vx);
        assert_eq!(
            (vx.tactical_speed_m_per_s, vx.cruise_speed_m_per_s),
            (280.0, 900.0)
        );
        assert_eq!(
            (vx.acceleration_m_per_s2, vx.turn_rate_deg_per_s),
            (60.0, 90.0)
        );
        let warden = rules.class(ShipClass::Warden);
        assert_eq!(
            (warden.tactical_speed_m_per_s, warden.cruise_speed_m_per_s),
            (110.0, 500.0)
        );
        assert_eq!(
            (warden.acceleration_m_per_s2, warden.turn_rate_deg_per_s),
            (15.0, 20.0)
        );
        assert!(warden.frigate);
        assert_eq!(rules.class(ShipClass::Mule).weapon_range_m, None);
        assert_eq!(
            rules.class(ShipClass::HeavyCruiser).acceleration_m_per_s2,
            8.0
        );
        // D-103, D-055, D-102, doc 14 §12.
        assert_eq!(rules.movement.ops_area_radius_m, 20_000.0);
        assert_eq!(rules.movement.formation_spacing_m, 150.0);
        assert_eq!((rules.gates.spool_up_s, rules.gates.shroud_s), (5, 10));
        assert_eq!(rules.gates.arrival_scatter_m, 2_000.0);
        assert_eq!(rules.docking.mother_station_core_ships, 24);
        assert_eq!(rules.docking.core_ships, 8);
        assert_eq!(rules.docking.dry_dock_ships, 6);
        assert_eq!(
            rules.start_kit.fleet,
            BTreeMap::from([(ShipClass::Vx, 3), (ShipClass::Mule, 2)])
        );
        // The slowest commander warship is the WARDEN (D-099 item 5), not the HSTL.
        assert_eq!(rules.slowest_commander_warship(), ShipClass::Warden);
        assert!((vx.turn_rate_rad_per_s() - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    }

    #[test]
    fn rejects_unknown_formats_fields_and_bad_values() {
        let swap = |from: &str, to: &str| {
            assert!(DEFAULT_RULES_JSON.contains(from), "{from}");
            Rules::from_json(&DEFAULT_RULES_JSON.replacen(from, to, 1))
        };
        assert!(matches!(
            swap("nomad-rules@1", "nomad-rules@2"),
            Err(RulesError::UnknownFormat(f)) if f == "nomad-rules@2"
        ));
        assert!(matches!(
            swap("\"ops_area_radius_m\"", "\"ops_radius_m\""),
            Err(RulesError::Parse(_))
        ));
        assert!(matches!(
            swap("\"HEAVY_CRUISER\"", "\"BATTLESHIP\""),
            Err(RulesError::Parse(_))
        ));
        assert!(matches!(
            swap("\"tactical_speed_m_per_s\": 280", "\"tactical_speed_m_per_s\": -280"),
            Err(RulesError::Invalid { field, .. }) if field.contains("tactical_speed")
        ));
        assert!(matches!(
            swap(
                "\"cruise_speed_m_per_s\": 900",
                "\"cruise_speed_m_per_s\": 100"
            ),
            Err(RulesError::Invalid { .. })
        ));
        assert!(matches!(
            swap("\"separation_m\": 120", "\"separation_m\": 150"),
            Err(RulesError::Invalid { .. })
        ));
        assert!(matches!(swap("{", "["), Err(RulesError::Parse(_))));

        let mut rules = Rules::embedded();
        rules.ship_classes.remove(&ShipClass::Lancer);
        assert!(matches!(
            rules.check(),
            Err(RulesError::MissingClass(ShipClass::Lancer))
        ));
        let mut rules = Rules::embedded();
        rules.movement.look_ahead_s = f64::NAN;
        assert!(matches!(rules.check(), Err(RulesError::Invalid { .. })));
        let mut rules = Rules::embedded();
        for class in rules.ship_classes.values_mut() {
            class.weapon_range_m = None;
        }
        assert!(matches!(rules.check(), Err(RulesError::Invalid { .. })));
    }
}
