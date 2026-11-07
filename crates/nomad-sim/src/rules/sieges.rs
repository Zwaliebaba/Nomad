//! Offline and siege rules (Design doc 12 §4, §5.3, §8; S6 plan §4.9).
//!
//! S6a holds the values for presence and exposure windows:
//!
//! | Value | Source |
//! |---|---|
//! | Disconnect grace 120 s | Doc 12 §5.3 and §8; D-160, D-162 |
//! | Window 4 h | D-015, D-032 |
//! | Drift at most 1 h per day | D-032, D-155 |
//! | Activity history 14 days | D-032, D-155 |
//! | Free choice in the first 7 days | D-032, D-157 |
//!
//! The siege values (notice, siege clock, protections, stagger, breach credit, restores) join
//! this section with S6b. Windows and drift are whole quarter hours, the granularity of the
//! activity history (D-155). All values are configurable, as D-015 says.

use nomad_types::QuarterHour;
use serde::{Deserialize, Serialize};

use super::{RulesError, invalid};

/// Presence and exposure-window numbers (D-015, D-032, D-155 to D-157, D-160).
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
        Ok(())
    }
}
