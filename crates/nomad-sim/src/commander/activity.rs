//! The activity history: minutes online per quarter hour of each UTC day (D-154, D-155,
//! D-171; S6 plan §4.3).
//!
//! One row of 96 counts (0–15 minutes) per UTC day, newest first, about 1.3 KB per commander
//! over 14 days. Every 60 s sample rolls the history forward to the sample's day, so days
//! older than the history are dropped whether or not the commander was online; an online
//! sample also adds a minute to its quarter hour.
//!
//! The history arrives in snapshots from outside the process, so nothing here indexes
//! without a check: a malformed row is read as far as it goes.

use std::collections::VecDeque;

use nomad_types::QuarterHour;
use serde::{Deserialize, Serialize};

const MILLIS_PER_DAY: i64 = 86_400_000;
/// Samples per quarter hour: one a minute.
const MAX_MINUTES: u8 = 15;

/// A commander's activity history.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ActivityHistory {
    /// The UTC day (days since 1970-01-01) of `rows[0]`, once there is a row.
    newest_day: Option<i64>,
    /// Minutes online per quarter hour, one row per UTC day, newest first.
    rows: VecDeque<Vec<u8>>,
}

/// The UTC day and quarter hour of a Universe time.
pub(crate) fn day_and_quarter(unix_ms: i64) -> (i64, usize) {
    let day = unix_ms.div_euclid(MILLIS_PER_DAY);
    let quarter = unix_ms.rem_euclid(MILLIS_PER_DAY) / QuarterHour::MILLIS;
    (day, usize::try_from(quarter).unwrap_or(0))
}

impl ActivityHistory {
    /// Records the 60 s sample taken at Universe time `unix_ms`: rolls the history to that
    /// day, keeping `days` days, and if the commander is `online` adds a minute to the
    /// quarter hour (D-154).
    pub(crate) fn sample(&mut self, unix_ms: i64, online: bool, days: u32) {
        let (day, quarter) = day_and_quarter(unix_ms);
        let keep = usize::try_from(days).unwrap_or(usize::MAX);
        let empty = || vec![0; usize::from(QuarterHour::PER_DAY)];
        match self.newest_day {
            None => self.rows.push_front(empty()),
            Some(newest) if day > newest => {
                // One new row per day passed, but never more than the history keeps.
                let passed = usize::try_from(day - newest).unwrap_or(usize::MAX);
                for _ in 0..passed.min(keep) {
                    self.rows.push_front(empty());
                }
            }
            // Time never runs backwards; a sample from an earlier day changes nothing.
            Some(newest) if day < newest => return,
            Some(_) => {}
        }
        self.newest_day = Some(day);
        self.rows.truncate(keep);
        if online && let Some(minutes) = self.rows.front_mut().and_then(|row| row.get_mut(quarter))
        {
            *minutes = minutes.saturating_add(1).min(MAX_MINUTES);
        }
    }

    /// Minutes online per quarter hour of the day, summed over the days of the history that
    /// lie within `days` days of Universe time `unix_ms`, today included.
    pub(crate) fn totals(&self, unix_ms: i64, days: u32) -> Vec<u32> {
        let mut totals = vec![0_u32; usize::from(QuarterHour::PER_DAY)];
        let Some(newest) = self.newest_day else {
            return totals;
        };
        let (today, _) = day_and_quarter(unix_ms);
        let oldest = today.saturating_sub(i64::from(days)).saturating_add(1);
        for (age, row) in (0_i64..).zip(&self.rows) {
            let day = newest.saturating_sub(age);
            if day < oldest || day > today {
                continue;
            }
            for (total, &minutes) in totals.iter_mut().zip(row) {
                *total = total.saturating_add(u32::from(minutes));
            }
        }
        totals
    }

    /// The number of days held, for tests.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.rows.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINUTE: i64 = 60_000;
    const HOUR: i64 = 60 * MINUTE;

    #[test]
    fn samples_fill_quarter_hours_of_utc_days() {
        let mut history = ActivityHistory::default();
        // 19:00–19:15 on day 3: 15 online samples, one offline.
        for m in 0..15 {
            history.sample(3 * MILLIS_PER_DAY + 19 * HOUR + m * MINUTE, true, 14);
        }
        history.sample(3 * MILLIS_PER_DAY + 19 * HOUR + 15 * MINUTE, false, 14);
        let totals = history.totals(3 * MILLIS_PER_DAY + 20 * HOUR, 14);
        assert_eq!(totals[76], 15);
        assert_eq!(totals.iter().sum::<u32>(), 15);
        // A sixteenth sample in one quarter hour cannot happen at 60 s, but is capped.
        history.sample(3 * MILLIS_PER_DAY + 19 * HOUR, true, 14);
        assert_eq!(history.totals(3 * MILLIS_PER_DAY + 20 * HOUR, 14)[76], 15);
    }

    #[test]
    fn old_days_drop_out_whether_online_or_not() {
        let mut history = ActivityHistory::default();
        history.sample(19 * HOUR, true, 14);
        // Thirteen days later the first day still counts; fourteen days later it does not.
        assert_eq!(history.totals(13 * MILLIS_PER_DAY, 14)[76], 1);
        history.sample(13 * MILLIS_PER_DAY, false, 14);
        assert_eq!(history.len(), 14);
        assert_eq!(history.totals(13 * MILLIS_PER_DAY, 14)[76], 1);
        history.sample(14 * MILLIS_PER_DAY, false, 14);
        assert_eq!(history.len(), 14);
        assert_eq!(history.totals(14 * MILLIS_PER_DAY, 14)[76], 0);
        // A long gap leaves at most 14 rows, all empty.
        history.sample(400 * MILLIS_PER_DAY, false, 14);
        assert_eq!(history.len(), 14);
        // Totals ignore rows the sampling has not rolled out yet.
        let mut stale = ActivityHistory::default();
        stale.sample(19 * HOUR, true, 14);
        assert_eq!(stale.totals(30 * MILLIS_PER_DAY, 14)[76], 0);
    }

    #[test]
    fn times_before_1970_and_malformed_rows_do_not_panic() {
        assert_eq!(day_and_quarter(-1), (-1, 95));
        let mut history = ActivityHistory {
            newest_day: Some(0),
            rows: VecDeque::from([vec![1, 2]]),
        };
        history.sample(23 * HOUR, true, 14);
        assert_eq!(history.totals(23 * HOUR, 14)[..3], [1, 2, 0]);
    }
}
