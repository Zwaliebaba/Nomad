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
//! | Start kit | Doc 14 §12 (D-112): 3 VX and 2 MULE; the station's modules and stock are in the economy section |
//! | Economy | Doc 14 §6–§10 (D-126 to D-129, D-133, D-134), module and ship tables §9.3–§9.4, command costs doc 15 §12, slot template D-130/D-131 (positions *Proposed*), the core kit's growth k = 0.25 (D-189), the diminishing yield F = 2 (D-217) |
//! | Cruise drop range | Doc 15 §3.3: a detected hostile within 20 km (D-114, S7) |
//! | Combat: hull, weapons, multipliers, hit radii | [`combat`] (doc 15 §3–§4, §12; D-052, D-143) |
//! | Defence: dome sectors, regeneration, turrets, abilities, recovery | [`defence`] (doc 15 §5, §12; D-140 to D-142, D-147 to D-149) |
//! | Mother-station layout | The phase-1 station (`station_scene.json` module bounds, `ships_placement.json` CAP WING), 1 voxel ≈ 35 m and grid north at azimuth 225° (doc 10 §8, D-046) |
//! | Sieges: disconnect grace, exposure window, drift, activity history, free choice; notice, siege clock, stagger, breach credit, protections, restores, the comeback grant | [`sieges`] (doc 12 §4, §5.3, §8; doc 15 §6–§7; D-015, D-032 to D-034, D-057, D-155 to D-167) |
//! | Season: the early-win share | [`season`] (doc 10 §6; D-020, D-192) |
//! | Sensors: ranges per sensor kind (each class names its kind), signatures, memory and lane-traffic timings, the range rule | [`sensors`] (doc 16 §3–§5; D-072 to D-076, D-179; S7) |
//!
//! Starting values adopted from this implementation (D-113): separation distance, obstacle
//! margin, look-ahead time (D-105 fixes the behaviour, D-113 the numbers), the guard radius
//! (S3 plan §4.4 names it without a value) and the launch point.

pub mod combat;
pub mod defence;
pub mod sensors;
pub mod sieges;

// S8: season standings and the win.
pub mod season;

use std::collections::BTreeMap;

use nomad_types::ShipClass;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The format tag a rules file must carry (`CODING_STANDARDS.md` §9: unknown formats are
/// rejected).
pub const RULES_FORMAT: &str = "nomad-rules@10";

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
    /// The economy: modules, ships, production, power, storage, logistics and the start
    /// station (doc 14; S4).
    pub economy: crate::economy::rules::EconomyRules,
    /// The mother station's layout, for obstacles and the launch point.
    pub mother_station: StationLayout,
    /// Hull, weapons and the multiplier table (S5).
    pub combat: combat::CombatRules,
    /// Dome sectors, regeneration, turrets, abilities and recovery (S5b).
    pub defence: defence::DefenceRules,
    /// Presence, exposure windows and the siege lifecycle (S6; doc 12 §4, §5.3; doc 15 §6–§7).
    pub sieges: sieges::SiegeRules,

    // S8: season standings and the win.
    /// The early win (S8; doc 10 §6, D-020, D-192).
    pub season: season::SeasonRules,
    // S7: sensors and fog of war.
    /// Sensor ranges, signatures, memory and lane-traffic timings (doc 16 §3–§5; S7).
    pub sensors: sensors::SensorRules,
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
    /// Its sensor kind, a key of `sensors.ranges_m` (D-073; S7 plan §4.1).
    pub sensor: String,
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
    /// Ships closer than this give way to each other (soft separation, D-105; value D-113).
    /// Smaller than the formation spacing, so it never fights a formation.
    pub separation_m: f64,
    /// Steering keeps this clearance from obstacle footprints (D-113).
    pub obstacle_margin_m: f64,
    /// How far ahead, in seconds of travel, steering looks for obstacles (D-113).
    pub look_ahead_s: f64,
    /// A guarding fleet stays within this distance of what it guards (D-113).
    pub guard_radius_m: f64,
    /// A hostile detected within this range drops a fleet out of cruise mode and engages it
    /// (doc 15 §3.3, D-114); detection is the commander's visibility set (S7).
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
    /// The ships of the starting fleet, docked at the mother station. The station's modules
    /// and stock are in [`crate::economy::rules::StartRules`].
    pub fleet: BTreeMap<ShipClass, u32>,
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
    /// Where launched ships appear and dock, in grid xy: the centre of the phase-1 CAP WING,
    /// inside the dome and clear of every obstacle (D-113).
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
        self.combat.check(&self.ship_classes)?;
        self.economy.check()?;
        self.defence.check()?;
        self.sieges.check()?;
        self.season.check()?;
        self.sieges.check_with(&self.defence)?;
        self.sensors
            .check(&self.ship_classes, &self.combat, &self.defence)?;
        for class in self.economy.ships.keys() {
            if self.class(*class).npc {
                return Err(invalid(
                    format!("economy.ships.{class:?}"),
                    "NPC classes cannot be built",
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
    use nomad_types::ModuleKind;

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
        // Doc 14 §9.3–§9.4, §12 (D-112), D-129, D-133.
        let economy = &rules.economy;
        assert_eq!(economy.module(ModuleKind::Refinery).cost.alloy, 3_500);
        assert_eq!(economy.module(ModuleKind::Solar).supply_mw, 620);
        assert_eq!(economy.ship(ShipClass::Warden).map(|s| s.command), Some(25));
        assert_eq!(economy.ship(ShipClass::HeavyCruiser), None);
        assert_eq!((economy.start.alloy, economy.start.crystal), (6_000, 500));
        assert_eq!(economy.start.modules.len(), 9);
        assert_eq!(
            (economy.command_cap.base, economy.command_cap.per_holding),
            (120, 10)
        );
        assert_eq!(economy.logistics.mule_units, 2_000);
        // D-189: the core kit's base price and its growth k = 0.25.
        assert_eq!(
            (economy.core_kit.alloy, economy.core_kit.crystal),
            (6_000, 500)
        );
        assert_eq!(economy.core_kit_growth_permille, 250);
        // D-217: the diminishing yield starts at F = 2.
        assert_eq!(economy.diminishing_yield_permille, 2_000);
        // Doc 12 §8, D-015, D-032, D-155, D-157: the S6a values.
        let sieges = &rules.sieges;
        assert_eq!(sieges.disconnect_grace_s, 120);
        assert_eq!((sieges.window_minutes, sieges.window_quarters()), (240, 16));
        assert_eq!(
            (sieges.window_drift_minutes_per_day, sieges.drift_quarters()),
            (60, 4)
        );
        assert_eq!(
            (sieges.activity_history_days, sieges.free_choice_days),
            (14, 7)
        );
        // D-015, D-033, D-034, D-057, D-167: the S6b values.
        assert_eq!(
            (
                sieges.notice_minutes,
                sieges.siege_minutes,
                sieges.stagger_minutes,
                sieges.slots_per_window()
            ),
            (720, 60, 60, 4)
        );
        assert_eq!(sieges.breach_credit_s, 600);
        assert_eq!(
            (
                sieges.held_protection_hours,
                sieges.capture_protection_hours,
                sieges.jump_protection_hours
            ),
            (48, 48, 72)
        );
        assert_eq!(
            (
                sieges.held_restore_percent,
                sieges.capture_core_percent,
                sieges.capture_sector_percent
            ),
            (50, 25, 25)
        );
        assert_eq!((sieges.comeback_min_kits, sieges.comeback_max_kits), (1, 3));
        // D-224: an outpost's core starts at 10% hull.
        assert_eq!(economy.core.deploy_start_hull_percent, 10);
        // The slowest commander warship is the WARDEN (D-099 item 5), not the HSTL.
        assert_eq!(rules.slowest_commander_warship(), ShipClass::Warden);
        assert!((vx.turn_rate_rad_per_s() - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    }

    #[test]
    fn rejects_bad_siege_values() {
        let swap = |from: &str, to: &str| {
            assert!(DEFAULT_RULES_JSON.contains(from), "{from}");
            Rules::from_json(&DEFAULT_RULES_JSON.replacen(from, to, 1))
        };
        // S6b: the stagger divides the window; percentages and the grant are in range;
        // breach credit fits in the ledger's window (D-015, D-033, D-057, D-167).
        for (from, to) in [
            ("\"stagger_minutes\": 60", "\"stagger_minutes\": 0"),
            ("\"stagger_minutes\": 60", "\"stagger_minutes\": 75"),
            ("\"notice_minutes\": 720", "\"notice_minutes\": 0"),
            ("\"siege_minutes\": 60", "\"siege_minutes\": 0"),
            (
                "\"held_restore_percent\": 50",
                "\"held_restore_percent\": 101",
            ),
            (
                "\"capture_core_percent\": 25",
                "\"capture_core_percent\": 0",
            ),
            ("\"comeback_min_kits\": 1", "\"comeback_min_kits\": 4"),
            ("\"breach_credit_s\": 600", "\"breach_credit_s\": 601"),
        ] {
            assert!(
                matches!(swap(from, to), Err(RulesError::Invalid { field, .. }) if field.starts_with("sieges.")),
                "{to}"
            );
        }
        // D-224: an outpost's starting hull is a share of its hull.
        assert!(matches!(
            swap(
                "\"deploy_start_hull_percent\": 10",
                "\"deploy_start_hull_percent\": 0"
            ),
            Err(RulesError::Invalid { field, .. }) if field == "economy.core.deploy_start_hull_percent"
        ));
    }

    #[test]
    fn rejects_unknown_formats_fields_and_bad_values() {
        let swap = |from: &str, to: &str| {
            assert!(DEFAULT_RULES_JSON.contains(from), "{from}");
            Rules::from_json(&DEFAULT_RULES_JSON.replacen(from, to, 1))
        };
        assert!(matches!(
            swap("nomad-rules@10", "nomad-rules@9"),
            Err(RulesError::UnknownFormat(f)) if f == "nomad-rules@9"
        ));
        // The sieges section (S6a): whole quarter hours, a window shorter than a day.
        for (from, to) in [
            ("\"window_minutes\": 240", "\"window_minutes\": 250"),
            ("\"window_minutes\": 240", "\"window_minutes\": 0"),
            ("\"window_minutes\": 240", "\"window_minutes\": 1440"),
            (
                "\"window_drift_minutes_per_day\": 60",
                "\"window_drift_minutes_per_day\": 70",
            ),
            (
                "\"window_drift_minutes_per_day\": 60",
                "\"window_drift_minutes_per_day\": 735",
            ),
            (
                "\"activity_history_days\": 14",
                "\"activity_history_days\": 0",
            ),
        ] {
            assert!(
                matches!(swap(from, to), Err(RulesError::Invalid { field, .. }) if field.starts_with("sieges.")),
                "{to}"
            );
        }
        assert!(matches!(
            swap("\"disconnect_grace_s\": 120,", ""),
            Err(RulesError::Parse(_))
        ));
        // A rules file without the D-189 growth does not parse; a zero growth is the flat
        // price D-189 replaced.
        assert!(matches!(
            swap("\"core_kit_growth_permille\": 250,", ""),
            Err(RulesError::Parse(_))
        ));
        assert!(matches!(
            swap(
                "\"core_kit_growth_permille\": 250",
                "\"core_kit_growth_permille\": 0"
            ),
            Err(RulesError::Invalid { field, .. }) if field == "economy.core_kit_growth_permille"
        ));
        assert!(matches!(
            swap(
                "\"core_kit_growth_permille\": 250",
                "\"core_kit_growth_permille\": -250"
            ),
            Err(RulesError::Parse(_))
        ));
        // D-228: peripheral slots lie wholly outside the dome (the S5b positions did not).
        assert!(matches!(
            swap("\"solar\": [-62, 0]", "\"solar\": [-40, -24]"),
            Err(RulesError::Invalid { field, .. }) if field == "economy.slots.solar"
        ));
        assert!(matches!(
            swap("\"deposits\": [[50, 0]", "\"deposits\": [[46, 0]"),
            Err(RulesError::Invalid { field, .. }) if field == "economy.slots.deposits"
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

    #[test]
    fn the_diminishing_yield_is_required_and_positive() {
        let swap = |from: &str, to: &str| {
            assert!(DEFAULT_RULES_JSON.contains(from), "{from}");
            Rules::from_json(&DEFAULT_RULES_JSON.replacen(from, to, 1))
        };
        // D-217's F: required, and never zero, which would stop all remote production.
        assert!(matches!(
            swap("\"diminishing_yield_permille\": 2000,", ""),
            Err(RulesError::Parse(_))
        ));
        assert!(matches!(
            swap(
                "\"diminishing_yield_permille\": 2000",
                "\"diminishing_yield_permille\": 0"
            ),
            Err(RulesError::Invalid { field, .. }) if field == "economy.diminishing_yield_permille"
        ));
        assert!(matches!(
            swap(
                "\"diminishing_yield_permille\": 2000",
                "\"diminishing_yield_permille\": -2000"
            ),
            Err(RulesError::Parse(_))
        ));
    }

    #[test]
    fn the_early_win_share_is_60_percent_and_more_than_half() {
        // Doc 10 §6, D-020, D-192.
        assert_eq!(Rules::embedded().season.early_win_share_permille, 600);
        let swap = |to: &str| {
            Rules::from_json(&DEFAULT_RULES_JSON.replacen(
                "\"early_win_share_permille\": 600",
                to,
                1,
            ))
        };
        for share in ["500", "0", "1001"] {
            assert!(
                matches!(
                    swap(&format!("\"early_win_share_permille\": {share}")),
                    Err(RulesError::Invalid { field, .. }) if field == "season.early_win_share_permille"
                ),
                "{share}"
            );
        }
        assert!(swap("\"early_win_share_permille\": 1000").is_ok());
        assert!(matches!(swap(""), Err(RulesError::Parse(_))));
    }
}
