//! Offline and siege rules (Design doc 12 §4, §5.3, §8; S6 plan §4.9).
//!
//! | Value | Source |
//! |---|---|
//! | Disconnect grace 120 s | Doc 12 §5.3 and §8; D-160, D-162 |
//! | Window 4 h | D-015, D-032 |
//! | Drift at most 1 h per day | D-032, D-155 |
//! | Activity history 14 days | D-032, D-155 |
//! | Free choice in the first 7 days | D-032, D-157 |
//! | Notice 12 h, siege clock 60 min | D-015 |
//! | Stagger 60 min, so 4 exits per window | D-033, D-158 |
//! | Breach credit 10 min | D-057 |
//! | Protection 48 h after Held, 48 h for a capturer, 72 h after an emergency jump | D-015, D-057, D-034 |
//! | Held restores the sectors to 50%; a capture the core and the sectors to 25% | D-057 |
//! | Comeback grant: 1 to 3 core kits | D-167 |
//! | A landed mother station's core at 25% hull | *Proposed*: as a captured core; no agreed text gives it |
//! | A fallen mother station's emergency jump at 500 m/s on in-system legs | *Proposed*: the WARDEN's cruise speed, the slowest commander warship (doc 15 §12); D-166 times the jump at cruise speed but a station has none |
//!
//! Windows, drift and the stagger are whole quarter hours, the granularity of the activity
//! history (D-155). All values are configurable, as D-015 says.

use nomad_types::QuarterHour;
use serde::{Deserialize, Serialize};

use super::{RulesError, invalid};

/// Presence, exposure-window and siege numbers (D-015, D-032, D-033, D-034, D-057, D-155 to
/// D-167).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SiegeRules {
    /// A commander who goes offline keeps their direct orders this long; then they lapse
    /// (doc 12 §5.3, D-162). Inside it the commander still counts as online for sieges
    /// (D-160).
    pub disconnect_grace_s: u64,
    /// Length of the daily exposure window (D-015, D-032).
    pub window_minutes: u32,
    /// How far the window may move when it closes, towards the most active hours (D-032,
    /// D-155).
    pub window_drift_minutes_per_day: u32,
    /// UTC days of activity the window follows, today included (D-032, D-155).
    pub activity_history_days: u32,
    /// For this many days after joining, a commander may choose their window (D-157).
    pub free_choice_days: u32,

    // S6b: the siege lifecycle.
    /// The least notice from a breach to its siege (D-015, D-158).
    pub notice_minutes: u32,
    /// The siege clock (D-015).
    pub siege_minutes: u32,
    /// Exits of one commander's holdings lie this far apart, at slots this far apart from
    /// the window's start (D-033, D-158). The window holds `window_minutes / stagger_minutes`
    /// slots.
    pub stagger_minutes: u32,
    /// The attackers of a breach damaged the dome this long before it (D-057).
    pub breach_credit_s: u64,
    /// After Held the holding cannot be breached for this long (D-015).
    pub held_protection_hours: u32,
    /// After a capture the new owner cannot be breached for this long (D-057).
    pub capture_protection_hours: u32,
    /// After an emergency jump the landed mother station cannot be breached for this long
    /// (D-034).
    pub jump_protection_hours: u32,
    /// Held raises every sector to this share of its maximum (D-057).
    pub held_restore_percent: u64,
    /// A captured core's hull is restored to this share (D-057).
    pub capture_core_percent: u32,
    /// A captured holding's sectors are raised to this share (D-057).
    pub capture_sector_percent: u64,
    /// A fallen mother station's core lands with this share of its hull (*Proposed*).
    pub landing_core_percent: u32,
    /// A fallen mother station's in-system legs during its emergency jump (*Proposed*;
    /// D-166 times the jump at cruise speed).
    pub station_jump_speed_m_per_s: u32,
    /// The comeback grant pays at least this many core kits (D-167).
    pub comeback_min_kits: u32,
    /// And at most this many (D-167).
    pub comeback_max_kits: u32,
}

impl SiegeRules {
    /// The window's length in quarter hours.
    pub fn window_quarters(&self) -> u8 {
        u8::try_from(self.window_minutes / QuarterHour::MINUTES).unwrap_or(u8::MAX)
    }

    /// The largest move of a window, in quarter hours.
    pub fn drift_quarters(&self) -> u8 {
        u8::try_from(self.window_drift_minutes_per_day / QuarterHour::MINUTES).unwrap_or(u8::MAX)
    }

    /// How many exit slots one window holds (D-033: 4).
    pub fn slots_per_window(&self) -> u32 {
        self.window_minutes / self.stagger_minutes.max(1)
    }

    /// Checks every value.
    pub(super) fn check(&self) -> Result<(), RulesError> {
        let day = u32::from(QuarterHour::PER_DAY) * QuarterHour::MINUTES;
        if self.window_minutes == 0
            || self.window_minutes >= day
            || !self.window_minutes.is_multiple_of(QuarterHour::MINUTES)
        {
            return Err(invalid(
                "sieges.window_minutes".to_owned(),
                "must be whole quarter hours, shorter than a day",
            ));
        }
        if self.window_drift_minutes_per_day > day / 2
            || !self
                .window_drift_minutes_per_day
                .is_multiple_of(QuarterHour::MINUTES)
        {
            return Err(invalid(
                "sieges.window_drift_minutes_per_day".to_owned(),
                "must be whole quarter hours, at most half a day",
            ));
        }
        if self.activity_history_days == 0 {
            return Err(invalid(
                "sieges.activity_history_days".to_owned(),
                "must be at least one day",
            ));
        }
        self.check_sieges()
    }

    /// What the siege values need of other sections: the ledger must keep a commander's dome
    /// damage at least as long as breach credit reaches back (D-057).
    pub(super) fn check_with(
        &self,
        defence: &super::defence::DefenceRules,
    ) -> Result<(), RulesError> {
        if defence.ledger_window_s < self.breach_credit_s {
            return Err(invalid(
                "sieges.breach_credit_s".to_owned(),
                "must not exceed defence.ledger_window_s",
            ));
        }
        Ok(())
    }

    /// The S6b values.
    fn check_sieges(&self) -> Result<(), RulesError> {
        if self.stagger_minutes == 0
            || !self.stagger_minutes.is_multiple_of(QuarterHour::MINUTES)
            || !self.window_minutes.is_multiple_of(self.stagger_minutes)
        {
            return Err(invalid(
                "sieges.stagger_minutes".to_owned(),
                "must be whole quarter hours that divide the window",
            ));
        }
        for (field, value) in [
            ("notice_minutes", self.notice_minutes),
            ("siege_minutes", self.siege_minutes),
            (
                "station_jump_speed_m_per_s",
                self.station_jump_speed_m_per_s,
            ),
        ] {
            if value == 0 {
                return Err(invalid(format!("sieges.{field}"), "must be > 0"));
            }
        }
        for (field, value) in [
            ("held_restore_percent", self.held_restore_percent),
            ("capture_sector_percent", self.capture_sector_percent),
            ("capture_core_percent", u64::from(self.capture_core_percent)),
            ("landing_core_percent", u64::from(self.landing_core_percent)),
        ] {
            if value == 0 || value > 100 {
                return Err(invalid(format!("sieges.{field}"), "must be 1 to 100"));
            }
        }
        if self.comeback_min_kits > self.comeback_max_kits {
            return Err(invalid(
                "sieges.comeback_min_kits".to_owned(),
                "must not exceed comeback_max_kits",
            ));
        }
        Ok(())
    }
}
