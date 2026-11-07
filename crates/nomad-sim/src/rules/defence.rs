//! Station defence rules: dome sectors, regeneration, turrets, abilities and recovery (Design
//! doc 15 §5, §12; D-053, D-140 to D-142, D-147 to D-149; S5 plan §4.5, §4.8, S5b).
//!
//! Its own section of the rules file, `defence`, next to `combat` (ships) and `economy`
//! (modules and power). The dome's radius is the slot template's (`economy.slots`, D-141), and
//! the regeneration draw of a full sector is `economy.power.regen_mw_per_sector`.
//!
//! Where each number comes from:
//!
//! | Value | Source |
//! |---|---|
//! | 150,000 points per full sector, 50/s regeneration at the full draw | Doc 15 §12, D-141 |
//! | 25% from the core, 75% from the sector's own pylon | D-140 |
//! | Regeneration = allocated MW × 50 / 120 per second | D-142 |
//! | Weak below 50% | Doc 15 §5.1 (phase 1) |
//! | Dome damage kept 10 min per commander | D-057: breach credit for the 10 min before a breach |
//! | Turret: 40 damage every 1 s, 2.5 km, lock 2 s | Doc 15 §12 |
//! | SHLD SURGE: 20% of the sector's maximum, 90 s cooldown, 200 MW for 10 s | Doc 15 §5.3, S4 plan §4.5, D-147 |
//! | FLAK SCREEN: 10 s, 90 MW, 60 s cooldown; a burst a second at each hostile Light or Medium ship within 2 km of the dome centre, at most 8 a second, the WARDEN's flak (30 damage, 150 m) | Doc 15 §5.3, D-148 |
//! | Recovery after 10 min with no hostile in the 20 km ops area, at 10% hull | D-149 |
//!
//! *Proposed* (no document gives a value; flagged for the owner): the turret projectile's
//! speed (800 m/s, a bolt like the battery's, D-188) and spread (20 → 4 mrad, as cannons and
//! batteries); the FLAK SCREEN shell's speed and spread (the WARDEN flak's, D-188), fired
//! from the dome centre without lock, so at the base spread; and the abilities' cooldowns
//! counted from the moment they start.

use serde::{Deserialize, Serialize};

use super::combat::{WeaponKind, WeaponRules, check_weapon};
use super::{RulesError, invalid, positive};
use crate::time::TICK_MILLIS;

/// The station defence numbers of a season (S5b).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DefenceRules {
    /// Strength of a sector at full maximum, in points (D-141).
    pub sector_full_points: u64,
    /// Share of a full sector that every core projects, in percent (D-140).
    pub core_share_percent: u64,
    /// Share a sector's own working pylon adds, in percent (D-140).
    pub pylon_share_percent: u64,
    /// Regeneration of a full sector, in points per second, when it is allocated the full
    /// draw `economy.power.regen_mw_per_sector` (D-141, D-142).
    pub regen_points_per_s: u64,
    /// A sector below this share of its maximum is weak, in percent (doc 15 §5.1).
    pub weak_below_percent: u64,
    /// How long the damage ledger keeps a commander's dome damage after their last hit
    /// (D-057's 10 minutes of breach credit).
    pub ledger_window_s: u64,
    /// Turret modules (doc 15 §5.3, §12).
    pub turret: TurretRules,
    /// SHLD SURGE (doc 15 §5.3; D-147).
    pub shield_surge: SurgeRules,
    /// FLAK SCREEN (doc 15 §5.3; D-148).
    pub flak_screen: FlakScreenRules,
    /// Recovery of disabled structures (D-149).
    pub recovery: RecoveryRules,
}

/// A turret module's weapon (doc 15 §12).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TurretRules {
    /// Time for its lock to rise from 0 to 100%.
    pub lock_ms: u64,
    /// The weapon, of kind `TURRET`.
    pub weapon: WeaponRules,
}

/// SHLD SURGE (doc 15 §5.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurgeRules {
    /// Restores this share of the chosen sector's maximum at once, in percent.
    pub restore_percent: u64,
    /// Time from one use to the next.
    pub cooldown_s: u64,
    /// Fixed power draw while it lasts.
    pub draw_mw: u64,
    /// How long it draws.
    pub draw_s: u64,
}

/// FLAK SCREEN (doc 15 §5.3; D-148).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlakScreenRules {
    /// How long it fires.
    pub duration_s: u64,
    /// Time from one use to the next.
    pub cooldown_s: u64,
    /// Fixed power draw while it fires.
    pub draw_mw: u64,
    /// Targets lie within this distance of the dome centre.
    pub reach_m: f64,
    /// Time between two bursts at the same target.
    pub interval_ms: u64,
    /// Most bursts in one volley.
    pub max_bursts: u32,
    /// Each burst's shell, of kind `FLAK`.
    pub weapon: WeaponRules,
}

/// Recovery of disabled structures (D-149).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryRules {
    /// Back online once no hostile has been inside the holding's ops area for this long.
    pub quiet_s: u64,
    /// The hull it comes back with, at least, in percent of its maximum.
    pub hull_percent: u32,
}

impl DefenceRules {
    /// Checks every value; called by [`super::Rules::check`].
    pub(super) fn check(&self) -> Result<(), RulesError> {
        let name = |field: &str| format!("defence.{field}");
        if self.sector_full_points == 0 || self.regen_points_per_s == 0 {
            return Err(invalid(
                name("sector_full_points"),
                "needs full strength and regeneration",
            ));
        }
        if self.core_share_percent == 0
            || self
                .core_share_percent
                .saturating_add(self.pylon_share_percent)
                != 100
        {
            return Err(invalid(
                name("core_share_percent"),
                "the core's and the pylon's shares must be positive and add up to 100",
            ));
        }
        if !(1..=100).contains(&self.weak_below_percent) {
            return Err(invalid(name("weak_below_percent"), "must be 1 to 100"));
        }
        if self.ledger_window_s == 0 {
            return Err(invalid(name("ledger_window_s"), "must be positive"));
        }
        let turret = &self.turret;
        if turret.lock_ms == 0 {
            return Err(invalid(name("turret.lock_ms"), "must be positive"));
        }
        check_weapon(&turret.weapon, &|f| name(&format!("turret.weapon.{f}")))?;
        if turret.weapon.kind != WeaponKind::Turret {
            return Err(invalid(name("turret.weapon.kind"), "must be TURRET"));
        }
        let surge = &self.shield_surge;
        if !(1..=100).contains(&surge.restore_percent)
            || surge.cooldown_s == 0
            || surge.draw_mw == 0
            || surge.draw_s == 0
        {
            return Err(invalid(
                name("shield_surge"),
                "needs a share of 1 to 100%, a cooldown, a draw and its duration",
            ));
        }
        let flak = &self.flak_screen;
        if flak.duration_s == 0 || flak.cooldown_s == 0 || flak.draw_mw == 0 || flak.max_bursts == 0
        {
            return Err(invalid(
                name("flak_screen"),
                "needs a duration, a cooldown, a draw and bursts",
            ));
        }
        if flak.interval_ms < TICK_MILLIS {
            return Err(invalid(
                name("flak_screen.interval_ms"),
                "must be at least one tick",
            ));
        }
        positive(&name("flak_screen.reach_m"), flak.reach_m)?;
        check_weapon(&flak.weapon, &|f| name(&format!("flak_screen.weapon.{f}")))?;
        if flak.weapon.kind != WeaponKind::Flak {
            return Err(invalid(name("flak_screen.weapon.kind"), "must be FLAK"));
        }
        if self.recovery.hull_percent == 0 || self.recovery.hull_percent > 100 {
            return Err(invalid(name("recovery.hull_percent"), "must be 1 to 100"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use nomad_types::ShipClass;

    use crate::rules::Rules;

    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "the rules file holds whole numbers, which parse exactly"
    )]
    fn defence_values_are_the_agreed_ones() {
        let rules = Rules::embedded();
        let d = &rules.defence;
        // D-141, D-140, D-142 with S4's 120 MW per full sector.
        assert_eq!(d.sector_full_points, 150_000);
        assert_eq!((d.core_share_percent, d.pylon_share_percent), (25, 75));
        assert_eq!(d.regen_points_per_s, 50);
        assert_eq!(rules.economy.power.regen_mw_per_sector, 120);
        assert_eq!(rules.economy.slots.dome_radius_voxels, 36);
        assert_eq!(d.weak_below_percent, 50);
        assert_eq!(d.ledger_window_s, 600);
        // Doc 15 §12.
        let turret = &d.turret.weapon;
        assert_eq!(
            (turret.damage, turret.interval_ms, turret.range_m),
            (40, 1_000, 2_500.0)
        );
        assert_eq!(d.turret.lock_ms, 2_000);
        // Doc 15 §5.3, D-147.
        let surge = d.shield_surge;
        assert_eq!(
            (
                surge.restore_percent,
                surge.cooldown_s,
                surge.draw_mw,
                surge.draw_s
            ),
            (20, 90, 200, 10)
        );
        // D-148: the WARDEN's flak values.
        let flak = &d.flak_screen;
        assert_eq!(
            (flak.duration_s, flak.cooldown_s, flak.draw_mw),
            (10, 60, 90)
        );
        assert_eq!(
            (flak.reach_m, flak.interval_ms, flak.max_bursts),
            (2_000.0, 1_000, 8)
        );
        let warden_flak = &rules.combat.class(ShipClass::Warden).weapons[2];
        assert_eq!(flak.weapon.damage, warden_flak.damage);
        assert_eq!(flak.weapon.burst_radius_m, warden_flak.burst_radius_m);
        assert_eq!(flak.weapon.speed_m_per_s, warden_flak.speed_m_per_s);
        // D-149.
        assert_eq!((d.recovery.quiet_s, d.recovery.hull_percent), (600, 10));
        // Turrets are out-ranged by torpedoes (doc 15 §12).
        let torpedo = &rules.combat.class(ShipClass::Lancer).weapons[0];
        assert!(turret.range_m < torpedo.range_m);
    }

    #[test]
    fn checks_reject_bad_defence_rules() {
        let edits: [fn(&mut Rules); 9] = [
            |r| r.defence.sector_full_points = 0,
            |r| r.defence.pylon_share_percent = 50,
            |r| r.defence.weak_below_percent = 0,
            |r| r.defence.turret.lock_ms = 0,
            |r| r.defence.turret.weapon.kind = crate::rules::combat::WeaponKind::Battery,
            |r| r.defence.shield_surge.restore_percent = 0,
            |r| r.defence.flak_screen.weapon.burst_radius_m = None,
            |r| r.defence.flak_screen.reach_m = 0.0,
            |r| r.defence.recovery.hull_percent = 0,
        ];
        for (i, edit) in edits.iter().enumerate() {
            let mut r = Rules::embedded();
            edit(&mut r);
            assert!(r.check().is_err(), "edit {i}");
        }
    }
}
