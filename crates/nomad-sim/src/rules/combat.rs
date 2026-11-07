//! Combat rules: hull, size classes, hit radii, weapons and the multiplier table (Design
//! doc 15 §3.1–§4, §12; D-052, D-058, D-143; S5 plan §4.1).
//!
//! Where each number comes from:
//!
//! | Value | Source |
//! |---|---|
//! | Hull, damage, fire interval, range, lock, torpedo and bolt speeds, spread | Doc 15 §4 and §12, starting values (D-058) |
//! | Size classes, the multiplier table | Doc 15 §3.1–§3.2 (D-052) |
//! | Hit radii | D-143: half the phase-1 model's short side at 35 m per voxel (`ships_placeholder.json`) |
//! | Raider torpedo: 0 against domes; no NPC weapon damages a dome ([`CombatRules::damage_by`]) | Doc 15 §12, D-056 |
//! | Engagement window | D-114 |
//! | Ranging laser: 4 km, 50% faster lock | Doc 15 §4, §12 |
//!
//! *Proposed* (doc 15 gives no value; flagged for the owner): the battery's projectile speed
//! (800 m/s, read as a "bolt"), the flak shell's speed (800 m/s) and the flak's spread (that
//! of cannons and batteries, 20 → 4 mrad).

use std::collections::BTreeMap;

use nomad_types::{Owner, ShipClass};
use serde::{Deserialize, Serialize};

use super::{RulesError, invalid, positive};
use crate::time::TICK_MILLIS;

/// How big a ship is, for the multiplier table (doc 15 §3.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SizeClass {
    /// Interceptors and strike craft.
    Light,
    /// Frigates and haulers.
    Medium,
    /// The HSTL heavy cruiser.
    Heavy,
}

/// What a hit lands on, for the multiplier table (doc 15 §3.2): a ship of a size class, a
/// structure (a module, S5b) or a dome sector (S5b).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TargetClass {
    /// A ship of this size class.
    Ship(SizeClass),
    /// A module.
    Structure,
    /// A dome sector.
    Dome,
}

/// A weapon's class, the row of the multiplier table (doc 15 §3.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WeaponKind {
    /// The VX's cannon.
    LightCannon,
    /// The LANCER's torpedo.
    Torpedo,
    /// The raider strike craft's torpedo: a torpedo with no effect on domes (D-056).
    RaiderTorpedo,
    /// Flak (WARDEN, station): bursts at the aim point, damaging Light and Medium targets.
    Flak,
    /// The WARDEN's and the HSTL's battery.
    Battery,
    /// A turret module (S5b).
    Turret,
}

/// One row of the multiplier table: integer percentages (D-052).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Multipliers {
    /// Against Light ships.
    pub light: u32,
    /// Against Medium ships.
    pub medium: u32,
    /// Against Heavy ships.
    pub heavy: u32,
    /// Against modules.
    pub structure: u32,
    /// Against dome sectors.
    pub dome: u32,
}

impl Multipliers {
    /// The percentage against `target`.
    pub const fn against(&self, target: TargetClass) -> u32 {
        match target {
            TargetClass::Ship(SizeClass::Light) => self.light,
            TargetClass::Ship(SizeClass::Medium) => self.medium,
            TargetClass::Ship(SizeClass::Heavy) => self.heavy,
            TargetClass::Structure => self.structure,
            TargetClass::Dome => self.dome,
        }
    }
}

/// One weapon of a ship class (doc 15 §4, §12).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeaponRules {
    /// Its row in the multiplier table.
    pub kind: WeaponKind,
    /// Damage per shot before the multiplier.
    pub damage: u32,
    /// Time between shots. At least one tick.
    pub interval_ms: u64,
    /// Projectile speed.
    pub speed_m_per_s: f64,
    /// Range: the weapon fires at targets this close, and its projectiles expire after
    /// flying this far.
    pub range_m: f64,
    /// Spread in milliradians at no lock and at full lock: the full width of the cone the
    /// shot's direction is drawn from.
    pub spread_mrad: [f64; 2],
    /// Flak only: the burst's radius.
    pub burst_radius_m: Option<f64>,
}

impl WeaponRules {
    /// Spread at `lock_fraction` (0 to 1), in radians: interpolated from the base value at no
    /// lock to the minimum at full lock (doc 15 §4).
    pub fn spread_rad(&self, lock_fraction: f64) -> f64 {
        let [base, minimum] = self.spread_mrad;
        let lock = lock_fraction.clamp(0.0, 1.0);
        (base + (minimum - base) * lock) / 1_000.0
    }
}

/// The combat numbers of one ship class (doc 15 §12; D-143).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClassCombat {
    /// Hull points of an undamaged ship.
    pub hull: u32,
    /// Size class for the multiplier table.
    pub size: SizeClass,
    /// Radius of the circle projectiles hit (D-143).
    pub hit_radius_m: f64,
    /// Time for a weapon's lock to rise from 0 to 100%.
    pub lock_ms: u64,
    /// The ranging laser's reach, for classes that carry one (the HSTL).
    pub ranging_laser_m: Option<f64>,
    /// The weapons, in a fixed order: weapon `i` of every ship of the class is the same.
    pub weapons: Vec<WeaponRules>,
}

impl ClassCombat {
    /// The longest weapon range, or `None` for an unarmed class.
    pub fn longest_range_m(&self) -> Option<f64> {
        self.weapons.iter().map(|w| w.range_m).reduce(f64::max)
    }
}

/// Combat rules of a season (doc 15 §3–§4; S5 plan §4.1).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CombatRules {
    /// An engagement lasts this long after its last trigger: taking fire, or a hostile
    /// within the cruise-drop range (D-114).
    pub engagement_s: u64,
    /// Lock rate, in percent of normal, of a weapon whose target an allied ranging laser
    /// designates (doc 15 §4: "50% faster").
    pub ranging_laser_lock_rate_percent: u32,
    /// The multiplier table (doc 15 §3.2).
    pub multipliers: BTreeMap<WeaponKind, Multipliers>,
    /// Per ship class.
    pub classes: BTreeMap<ShipClass, ClassCombat>,
}

impl CombatRules {
    /// The combat rules of `class`.
    pub fn class(&self, class: ShipClass) -> &ClassCombat {
        self.classes
            .get(&class)
            .expect("invariant: checked rules have combat rules for every ship class")
    }

    /// The multiplier row of `kind`.
    pub fn multipliers(&self, kind: WeaponKind) -> &Multipliers {
        self.multipliers
            .get(&kind)
            .expect("invariant: checked rules have a multiplier row for every weapon kind")
    }

    /// Damage of one hit of `weapon` on `target`: `base × multiplier / 100`, rounded down
    /// (D-052).
    pub fn damage(&self, weapon: &WeaponRules, target: TargetClass) -> u32 {
        let percent = u64::from(self.multipliers(weapon.kind).against(target));
        let damage = u64::from(weapon.damage) * percent / 100;
        u32::try_from(damage).unwrap_or(u32::MAX)
    }

    /// Damage of one hit of `weapon`, fired by `owner`, on `target`. As [`Self::damage`],
    /// except that **NPC weapons do no damage to domes**, whatever their multiplier row, so
    /// raiders can never breach or siege (D-056). The HSTL's batteries share the battery row,
    /// which does 75% to domes, so this is a rule of the owner, not of the weapon.
    pub fn damage_by(&self, owner: Owner, weapon: &WeaponRules, target: TargetClass) -> u32 {
        match (owner, target) {
            (Owner::Npc(_), TargetClass::Dome) => 0,
            _ => self.damage(weapon, target),
        }
    }

    /// Checks every value; `npc` and `weapon_range_m` are the classes' S3 rules, which must
    /// agree with the weapons here.
    pub(super) fn check(
        &self,
        ship_classes: &BTreeMap<ShipClass, super::ShipClassRules>,
    ) -> Result<(), RulesError> {
        if self.engagement_s == 0 {
            return Err(invalid(
                "combat.engagement_s".to_owned(),
                "must be positive",
            ));
        }
        if self.ranging_laser_lock_rate_percent < 100 {
            return Err(invalid(
                "combat.ranging_laser_lock_rate_percent".to_owned(),
                "must be at least 100",
            ));
        }
        for class in ShipClass::ALL {
            let combat = self
                .classes
                .get(&class)
                .ok_or(RulesError::MissingClass(class))?;
            let name = |field: &str| format!("combat.classes.{class:?}.{field}");
            if combat.hull == 0 {
                return Err(invalid(name("hull"), "must be positive"));
            }
            positive(&name("hit_radius_m"), combat.hit_radius_m)?;
            if combat.lock_ms == 0 {
                return Err(invalid(name("lock_ms"), "must be positive"));
            }
            if let Some(laser) = combat.ranging_laser_m {
                positive(&name("ranging_laser_m"), laser)?;
            }
            for (i, weapon) in combat.weapons.iter().enumerate() {
                let field = |f: &str| name(&format!("weapons[{i}].{f}"));
                check_weapon(weapon, &field)?;
                if !self.multipliers.contains_key(&weapon.kind) {
                    return Err(invalid(field("kind"), "has no row in the multiplier table"));
                }
            }
            let s3 = ship_classes
                .get(&class)
                .ok_or(RulesError::MissingClass(class))?;
            // The S3 class range is the longest weapon range (doc 15 §12; D-115).
            if s3.weapon_range_m != combat.longest_range_m() {
                return Err(invalid(
                    format!("ship_classes.{class:?}.weapon_range_m"),
                    "must equal the longest range of the class's weapons",
                ));
            }
        }
        Ok(())
    }
}

/// Checks one weapon's values; `field` names them in errors.
pub(super) fn check_weapon(
    weapon: &WeaponRules,
    field: &dyn Fn(&str) -> String,
) -> Result<(), RulesError> {
    if weapon.damage == 0 {
        return Err(invalid(field("damage"), "must be positive"));
    }
    if weapon.interval_ms < TICK_MILLIS {
        return Err(invalid(field("interval_ms"), "must be at least one tick"));
    }
    positive(&field("speed_m_per_s"), weapon.speed_m_per_s)?;
    positive(&field("range_m"), weapon.range_m)?;
    let [base, minimum] = weapon.spread_mrad;
    super::non_negative(&field("spread_mrad"), minimum)?;
    super::finite(&field("spread_mrad"), base)?;
    if base < minimum {
        return Err(invalid(
            field("spread_mrad"),
            "the base spread must not be below the minimum",
        ));
    }
    match (weapon.kind, weapon.burst_radius_m) {
        (WeaponKind::Flak, Some(radius)) => positive(&field("burst_radius_m"), radius),
        (WeaponKind::Flak, None) => Err(invalid(field("burst_radius_m"), "flak needs one")),
        (_, Some(_)) => Err(invalid(field("burst_radius_m"), "only flak bursts")),
        (_, None) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::Rules;

    fn rules() -> Rules {
        Rules::embedded()
    }

    #[test]
    fn the_multiplier_table_is_doc_15s() {
        // Doc 15 §3.2 (D-052), row by row: light, medium, heavy, structure, dome.
        let expected = [
            (WeaponKind::LightCannon, [100, 50, 25, 25, 10]),
            (WeaponKind::Torpedo, [25, 150, 150, 150, 100]),
            (WeaponKind::RaiderTorpedo, [25, 150, 150, 150, 0]),
            (WeaponKind::Flak, [200, 25, 10, 10, 0]),
            (WeaponKind::Battery, [50, 100, 100, 75, 75]),
            (WeaponKind::Turret, [100, 100, 100, 0, 0]),
        ];
        let rules = rules();
        assert_eq!(rules.combat.multipliers.len(), expected.len());
        for (kind, row) in expected {
            let m = rules.combat.multipliers(kind);
            assert_eq!(
                [m.light, m.medium, m.heavy, m.structure, m.dome],
                row,
                "{kind:?}"
            );
        }
    }

    #[test]
    fn damage_is_base_times_multiplier_rounded_down() {
        let rules = rules();
        let combat = &rules.combat;
        let targets = [
            TargetClass::Ship(SizeClass::Light),
            TargetClass::Ship(SizeClass::Medium),
            TargetClass::Ship(SizeClass::Heavy),
            TargetClass::Structure,
            TargetClass::Dome,
        ];
        // Every weapon of every class against every target class.
        for class in combat.classes.values() {
            for weapon in &class.weapons {
                for target in targets {
                    let percent = u64::from(combat.multipliers(weapon.kind).against(target));
                    let exact = u64::from(weapon.damage) * percent;
                    let damage = u64::from(combat.damage(weapon, target));
                    assert!(damage * 100 <= exact && exact < (damage + 1) * 100);
                }
            }
        }
        // A VX cannon (8) against a Heavy target (25%) does 2.
        let cannon = &combat.class(ShipClass::Vx).weapons[0];
        assert_eq!(
            combat.damage(cannon, TargetClass::Ship(SizeClass::Heavy)),
            2
        );
        // A battery (60) against a Light target (50%) does 30; a torpedo (160) against a
        // Light target (25%) does 40.
        let battery = &combat.class(ShipClass::Warden).weapons[0];
        assert_eq!(
            combat.damage(battery, TargetClass::Ship(SizeClass::Light)),
            30
        );
        let torpedo = &combat.class(ShipClass::Lancer).weapons[0];
        assert_eq!(
            combat.damage(torpedo, TargetClass::Ship(SizeClass::Light)),
            40
        );
        // A raider torpedo's row does nothing to domes (doc 15 §12).
        let raider = &combat.class(ShipClass::RaiderStrikeCraft).weapons[0];
        assert_eq!(combat.damage(raider, TargetClass::Dome), 0);
    }

    #[test]
    fn npc_weapons_never_damage_domes() {
        // D-056: whatever the weapon, an NPC owner does no damage to domes; a commander's
        // battery does 75%.
        let rules = rules();
        let c = &rules.combat;
        let npc = Owner::Npc(nomad_types::FactionId(1));
        let commander = Owner::Commander(nomad_types::CommanderId(1));
        for class in c.classes.values() {
            for weapon in &class.weapons {
                assert_eq!(c.damage_by(npc, weapon, TargetClass::Dome), 0);
            }
        }
        let hstl_battery = &c.class(ShipClass::HeavyCruiser).weapons[0];
        assert_eq!(c.damage_by(commander, hstl_battery, TargetClass::Dome), 52);
        assert_eq!(
            c.damage_by(npc, hstl_battery, TargetClass::Ship(SizeClass::Medium)),
            70
        );
    }

    #[test]
    fn class_values_are_doc_15s_starting_values() {
        let rules = rules();
        let c = &rules.combat;
        // Doc 15 §12 (D-058) and D-143.
        let hulls: Vec<u32> = ShipClass::ALL.iter().map(|&k| c.class(k).hull).collect();
        assert_eq!(hulls, [600, 800, 7_500, 2_500, 700, 30_000]);
        let radii: Vec<f64> = ShipClass::ALL
            .iter()
            .map(|&k| c.class(k).hit_radius_m)
            .collect();
        assert_eq!(radii, [90.0, 120.0, 120.0, 50.0, 90.0, 190.0]);
        let warden = c.class(ShipClass::Warden);
        assert_eq!(warden.size, SizeClass::Medium);
        let kinds: Vec<WeaponKind> = warden.weapons.iter().map(|w| w.kind).collect();
        assert_eq!(
            kinds,
            [WeaponKind::Battery, WeaponKind::Battery, WeaponKind::Flak]
        );
        assert_eq!(warden.weapons[2].burst_radius_m, Some(150.0));
        let cruiser = c.class(ShipClass::HeavyCruiser);
        assert_eq!(cruiser.weapons.len(), 4);
        assert_eq!(cruiser.ranging_laser_m, Some(4_000.0));
        assert!(c.class(ShipClass::Mule).weapons.is_empty());
        let vx = &c.class(ShipClass::Vx).weapons[0];
        assert_eq!((vx.damage, vx.interval_ms, vx.range_m), (8, 250, 1_500.0));
        assert_eq!(c.engagement_s, 30);
    }

    #[test]
    fn spread_narrows_with_lock() {
        let rules = rules();
        let torpedo = &rules.combat.class(ShipClass::Lancer).weapons[0];
        assert!((torpedo.spread_rad(0.0) - 0.030).abs() < 1e-12);
        assert!((torpedo.spread_rad(0.5) - 0.018).abs() < 1e-12);
        assert!((torpedo.spread_rad(1.0) - 0.006).abs() < 1e-12);
        assert!((torpedo.spread_rad(7.0) - 0.006).abs() < 1e-12);
    }

    /// S5 plan §4.1 and §4.7: every weapon's range stays below its own platform's sensor
    /// range for the smallest signature (0.6), so a target in range is always detected by the
    /// shooter itself. The sensor ranges are doc 16 §3's table until S7 puts them in the
    /// rules file.
    #[test]
    fn every_weapon_range_is_inside_its_platforms_own_detection() {
        let sensor_km = [
            (ShipClass::Vx, 15.0),
            (ShipClass::Lancer, 10.0),
            (ShipClass::Warden, 25.0),
            (ShipClass::Mule, 8.0),
            (ShipClass::RaiderStrikeCraft, 12.0),
            (ShipClass::HeavyCruiser, 30.0),
        ];
        let smallest_signature = 0.6;
        let rules = rules();
        for (class, km) in sensor_km {
            let detection_m = km * 1_000.0 * smallest_signature;
            let combat = rules.combat.class(class);
            for weapon in &combat.weapons {
                assert!(weapon.range_m < detection_m, "{class:?}");
            }
            if let Some(laser) = combat.ranging_laser_m {
                assert!(laser < detection_m, "{class:?} ranging laser");
            }
        }
    }

    #[test]
    fn checks_reject_bad_combat_rules() {
        let mut r = rules();
        r.combat.classes.remove(&ShipClass::Warden);
        assert!(matches!(
            r.check(),
            Err(RulesError::MissingClass(ShipClass::Warden))
        ));

        let mut r = rules();
        r.combat.multipliers.remove(&WeaponKind::Flak);
        assert!(r.check().is_err());

        let edits: [fn(&mut Rules); 8] = [
            // The S3 class range must match the weapons.
            |r| {
                if let Some(c) = r.ship_classes.get_mut(&ShipClass::Vx) {
                    c.weapon_range_m = Some(1_400.0);
                }
            },
            |r| weapon(r, ShipClass::Vx, 0).interval_ms = 50,
            |r| weapon(r, ShipClass::Vx, 0).spread_mrad = [4.0, 20.0],
            |r| weapon(r, ShipClass::Warden, 2).burst_radius_m = None,
            |r| weapon(r, ShipClass::Vx, 0).burst_radius_m = Some(10.0),
            |r| weapon(r, ShipClass::Lancer, 0).damage = 0,
            |r| {
                if let Some(c) = r.combat.classes.get_mut(&ShipClass::Lancer) {
                    c.hit_radius_m = 0.0;
                }
            },
            |r| r.combat.engagement_s = 0,
        ];
        for (i, edit) in edits.iter().enumerate() {
            let mut r = rules();
            edit(&mut r);
            assert!(r.check().is_err(), "edit {i}");
        }
    }

    fn weapon(rules: &mut Rules, class: ShipClass, index: usize) -> &mut WeaponRules {
        rules
            .combat
            .classes
            .get_mut(&class)
            .and_then(|c| c.weapons.get_mut(index))
            .expect("the class has the weapon")
    }
}
