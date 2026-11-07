//! Sensor rules: ranges per sensor kind, signatures per size class, and the timings of
//! last-known memory and lane traffic (Design doc 16 §3–§5; D-072 to D-076, D-176, D-179;
//! S7 plan §4.1).
//!
//! Its own section of the rules file, `sensors`. Each ship class names its sensor kind in
//! `ship_classes` (`sensor`), so classes can share a sensor and a tuning run can add one.
//!
//! | Value | Source |
//! |---|---|
//! | Holding core 40 km; VX 15, LANCER 10, WARDEN 25, MULE 8 km; raider strike craft 12, HSTL 30 km | D-073, doc 16 §3 |
//! | Signatures Light 0.6, Medium 1.0, Heavy 1.5, Structure 1.5 | D-072 |
//! | Fleet entries dropped 24 h after they were seen | D-076 |
//! | Memory swept once a minute per commander | S7 plan §4.4 |
//! | A lane-traffic record lasts until 10 min after the arrival tick | D-179 |
//!
//! Ranges are whole metres and signatures thousandths, so a detection range, `range ×
//! signature`, is exact in `f64` and detection at the boundary is bit-exact on every
//! platform (D-049).
//!
//! **The range rule** (S5 plan §4.1 and §4.7, now a check): every weapon of an armed
//! platform reaches less far than its own sensor detects the smallest signature. So a
//! target in weapon range is always one the shooter itself detects, and fog of war cannot
//! change who can be fired on, only where fleets find each other. Ship weapons and the
//! HSTL's ranging laser are checked against the class's sensor, turrets and FLAK SCREEN
//! against the holding core's.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::combat::{CombatRules, SizeClass};
use super::defence::DefenceRules;
use super::{RulesError, ShipClassRules, invalid};
use nomad_types::ShipClass;

/// The sensor numbers of a season (D-072, D-073, D-076, D-179).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SensorRules {
    /// A completed holding core's range at signature 1.0 (D-073: 40 km, the phase-1
    /// "PULSE · 40 KM").
    pub holding_core_range_m: u32,
    /// Range at signature 1.0 per ship sensor kind. Each ship class names its kind
    /// (`ship_classes.*.sensor`).
    pub ranges_m: BTreeMap<String, u32>,
    /// Signature per size class, in thousandths (D-072).
    pub signature_permille: BTreeMap<SizeClass, u32>,
    /// A holding's signature as one target at its dome centre, in thousandths (D-072:
    /// Structure; D-175).
    pub structure_signature_permille: u32,
    /// A fleet entry in last-known memory is dropped this long after it was seen (D-076).
    pub fleet_memory_s: u64,
    /// How often each commander's memory is swept for expired entries (S7 plan §4.4).
    pub memory_sweep_s: u64,
    /// A lane-traffic record expires this long after the fleet's arrival tick, unless the
    /// arrival is observed first (D-179).
    pub lane_traffic_after_arrival_s: u64,
}

impl SensorRules {
    /// The signature of a ship of `size`, in thousandths.
    pub fn ship_signature(&self, size: SizeClass) -> u32 {
        self.signature_permille
            .get(&size)
            .copied()
            .expect("invariant: checked rules have a signature for every size class")
    }

    /// The smallest signature any target has, in thousandths.
    pub fn smallest_signature(&self) -> u32 {
        self.signature_permille
            .values()
            .copied()
            .chain([self.structure_signature_permille])
            .min()
            .unwrap_or(0)
    }

    /// The largest signature any target has, in thousandths.
    pub fn largest_signature(&self) -> u32 {
        self.signature_permille
            .values()
            .copied()
            .chain([self.structure_signature_permille])
            .max()
            .unwrap_or(0)
    }

    /// The range of sensor `kind` at signature 1.0, if the kind exists.
    pub fn range_m(&self, kind: &str) -> Option<u32> {
        self.ranges_m.get(kind).copied()
    }

    /// Checks every value, and the range rule against the weapons of `ship_classes`,
    /// `combat` and `defence`.
    pub(super) fn check(
        &self,
        ship_classes: &BTreeMap<ShipClass, ShipClassRules>,
        combat: &CombatRules,
        defence: &DefenceRules,
    ) -> Result<(), RulesError> {
        if self.holding_core_range_m == 0 {
            return Err(invalid(
                "sensors.holding_core_range_m".to_owned(),
                "must be positive",
            ));
        }
        for (kind, &range) in &self.ranges_m {
            if range == 0 {
                return Err(invalid(
                    format!("sensors.ranges_m.{kind}"),
                    "must be positive",
                ));
            }
        }
        for size in [SizeClass::Light, SizeClass::Medium, SizeClass::Heavy] {
            if self.signature_permille.get(&size).is_none_or(|&s| s == 0) {
                return Err(invalid(
                    format!("sensors.signature_permille.{size:?}"),
                    "must be given and positive",
                ));
            }
        }
        if self.structure_signature_permille == 0 {
            return Err(invalid(
                "sensors.structure_signature_permille".to_owned(),
                "must be positive",
            ));
        }
        for (field, value) in [
            ("fleet_memory_s", self.fleet_memory_s),
            ("memory_sweep_s", self.memory_sweep_s),
            (
                "lane_traffic_after_arrival_s",
                self.lane_traffic_after_arrival_s,
            ),
        ] {
            if value == 0 {
                return Err(invalid(format!("sensors.{field}"), "must be positive"));
            }
        }

        // The range rule (S5 plan §4.1): weapon range < own sensor range × the smallest
        // signature, compared in thousandths of a metre so it is exact.
        let smallest = u64::from(self.smallest_signature());
        let below = |weapon_m: f64, sensor_m: u32| {
            weapon_m * 1_000.0 < (u64::from(sensor_m) * smallest) as f64
        };
        for (&class, rules) in ship_classes {
            let sensor = self.range_m(&rules.sensor).ok_or_else(|| {
                invalid(
                    format!("ship_classes.{class:?}.sensor"),
                    "names no sensor in sensors.ranges_m",
                )
            })?;
            let Some(class_combat) = combat.classes.get(&class) else {
                continue;
            };
            let reaches = class_combat
                .weapons
                .iter()
                .map(|w| w.range_m)
                .chain(class_combat.ranging_laser_m);
            if reaches.into_iter().any(|range| !below(range, sensor)) {
                return Err(invalid(
                    format!("ship_classes.{class:?}.sensor"),
                    "detects the smallest signature no farther than a weapon reaches",
                ));
            }
        }
        let core = self.holding_core_range_m;
        let station = [
            defence.turret.weapon.range_m,
            defence.flak_screen.reach_m,
            defence.flak_screen.weapon.range_m,
        ];
        if station.into_iter().any(|range| !below(range, core)) {
            return Err(invalid(
                "sensors.holding_core_range_m".to_owned(),
                "detects the smallest signature no farther than a turret or FLAK SCREEN reaches",
            ));
        }
        Ok(())
    }
}
