//! Snapshot and journal retention (Design doc 11 §8, D-048; N2 plan §4.4).
//!
//! Kept after every snapshot write, under the production policy
//! ([`RetentionPolicy::default`]):
//!
//! - the latest 2 snapshots, so that recovery can fall back to the older if the latest one
//!   is damaged;
//! - for the 48 h of Universe time before the latest snapshot, the first snapshot of each
//!   hour;
//! - before that, the first snapshot of each day, for the rest of the season;
//! - the journal for the 48 h before the latest snapshot (D-110), from the kept snapshot
//!   that opens that window, so any moment in it can be replayed from the hourly snapshot
//!   before it. The window therefore reaches back to the start of that snapshot's hour,
//!   48 h to 49 h in all.
//!
//! Older journal entries are deleted. Hours and days count from the season start (tick 0),
//! so the periodic snapshots every 3,000 ticks (5 min) fall on their boundaries.
//!
//! The lengths of the hour and the day are part of the policy rather than fixed, so that
//! tests can apply the same rules in scaled time ([`RetentionPolicy::scaled`]): days of
//! retention in thousands of ticks rather than millions.

use std::collections::BTreeSet;

use nomad_sim::{Tick, Ticks};

/// What retention keeps (doc 11 §8, D-048, D-110). Lengths are in ticks, and hours and days
/// count from tick 0. [`RetentionPolicy::default`] is the agreed policy, and the only one
/// outside tests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetentionPolicy {
    /// How many of the latest snapshots are always kept.
    pub keep_latest: usize,
    /// The length of an hour: within the hourly window, the first snapshot of each hour is
    /// kept. Zero counts as one tick.
    pub hour: Ticks,
    /// The length of a day, a whole number of hours: before the hourly window, the first
    /// snapshot of each day is kept. Zero counts as one tick.
    pub day: Ticks,
    /// How far back from the latest snapshot hourly snapshots are kept.
    pub hourly_window: Ticks,
    /// How far back from the latest snapshot the journal is kept (D-110).
    pub journal_window: Ticks,
}

impl Default for RetentionPolicy {
    /// Doc 11 §8: the latest 2 snapshots, hourly snapshots for 48 h, daily ones for the
    /// rest of the season (D-048), and the journal for 48 h (D-110; the 48 h is provisional
    /// until the journal volume is measured).
    fn default() -> Self {
        Self {
            keep_latest: 2,
            hour: Ticks::from_hours(1),
            day: Ticks::from_days(1),
            hourly_window: Ticks::from_hours(48),
            journal_window: Ticks::from_hours(48),
        }
    }
}

impl RetentionPolicy {
    /// The production policy in scaled time, for tests: an hour lasts `hour` ticks, and the
    /// day and both windows keep their length in hours. The number of latest snapshots kept
    /// is unchanged. `scaled(Ticks::from_hours(1))` is the production policy.
    pub fn scaled(hour: Ticks) -> Self {
        let production = Self::default();
        let in_hours = |length: Ticks| {
            Ticks::new((length.get() / production.hour.get()).saturating_mul(hour.get()))
        };
        Self {
            keep_latest: production.keep_latest,
            hour,
            day: in_hours(production.day),
            hourly_window: in_hours(production.hourly_window),
            journal_window: in_hours(production.journal_window),
        }
    }
}

/// What retention keeps and deletes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetentionPlan {
    /// Snapshots to keep, by tick.
    pub keep: BTreeSet<Tick>,
    /// Snapshots to delete, by tick.
    pub delete: Vec<Tick>,
    /// Journal entries at or before this tick are deleted. The journal after it is
    /// complete, so recovery may start from any kept snapshot at or after it.
    pub journal_floor: Tick,
}

/// Applies `policy` to the snapshots at `ticks` (any order, no duplicates).
pub fn plan(ticks: &[Tick], policy: RetentionPolicy) -> RetentionPlan {
    let hour = policy.hour.get().max(1);
    let day = policy.day.get().max(1);
    let mut sorted: Vec<Tick> = ticks.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    let Some(&latest) = sorted.last() else {
        return RetentionPlan {
            keep: BTreeSet::new(),
            delete: Vec::new(),
            journal_floor: Tick::ZERO,
        };
    };

    let mut keep: BTreeSet<Tick> = sorted
        .iter()
        .rev()
        .take(policy.keep_latest)
        .copied()
        .collect();
    // The hourly window starts at the hour that contains the point 48 h before the latest
    // snapshot. Aligning it to whole hours makes retention give the same result whether it
    // runs after every snapshot or once at the end: a snapshot deleted once would never be
    // kept later.
    let first_hourly_bucket = latest.get().saturating_sub(policy.hourly_window.get()) / hour;

    // `sorted` is ascending, so the first snapshot seen in a bucket is its earliest.
    let mut last_hour = None;
    let mut last_day = None;
    for &tick in &sorted {
        let (bucket, last) = if tick.get() / hour >= first_hourly_bucket {
            (tick.get() / hour, &mut last_hour)
        } else {
            (tick.get() / day, &mut last_day)
        };
        if *last != Some(bucket) {
            *last = Some(bucket);
            keep.insert(tick);
        }
    }

    // The journal starts at the newest kept snapshot at or before the start of its window
    // (D-110), or at the oldest kept snapshot if every one is newer. Either way it is at or
    // before the older of the latest two, so that fallback stays possible.
    let window_start = Tick::new(latest.get().saturating_sub(policy.journal_window.get()));
    let journal_floor = keep
        .range(..=window_start)
        .next_back()
        .or_else(|| keep.first())
        .copied()
        .unwrap_or(latest);

    let delete = sorted
        .into_iter()
        .filter(|tick| !keep.contains(tick))
        .collect();
    RetentionPlan {
        keep,
        delete,
        journal_floor,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Doc 11 §8 at 10 Hz (D-019), independent of the policy under test.
    const HOUR: u64 = 36_000;
    const DAY: u64 = 24 * HOUR;
    const WINDOW: u64 = 48 * HOUR;

    fn ticks(raw: impl IntoIterator<Item = u64>) -> Vec<Tick> {
        raw.into_iter().map(Tick::new).collect()
    }

    /// The production policy.
    fn production(ticks: &[Tick]) -> RetentionPlan {
        plan(ticks, RetentionPolicy::default())
    }

    #[test]
    fn the_default_policy_is_doc_11() {
        assert_eq!(
            RetentionPolicy::default(),
            RetentionPolicy {
                keep_latest: 2,
                hour: Ticks::new(HOUR),
                day: Ticks::new(DAY),
                hourly_window: Ticks::new(WINDOW),
                journal_window: Ticks::new(WINDOW),
            }
        );
    }

    #[test]
    fn a_scaled_policy_keeps_the_hours_of_doc_11() {
        assert_eq!(
            RetentionPolicy::scaled(Ticks::from_hours(1)),
            RetentionPolicy::default()
        );
        assert_eq!(
            RetentionPolicy::scaled(Ticks::new(120)),
            RetentionPolicy {
                keep_latest: 2,
                hour: Ticks::new(120),
                day: Ticks::new(24 * 120),
                hourly_window: Ticks::new(48 * 120),
                journal_window: Ticks::new(48 * 120),
            }
        );
    }

    #[test]
    fn a_scaled_policy_keeps_the_same_snapshots_in_scaled_time() {
        // Three days and five minutes of snapshots every 5 min, in production time and at
        // 1/300 of it, the scale of the end-to-end test: retention keeps and deletes the
        // same snapshots, and the journal starts at the same one. This is what lets the
        // end-to-end test run in scaled time.
        const SCALE: u64 = 300;
        let scaled = RetentionPolicy::scaled(Ticks::new(HOUR / SCALE));
        let count = (3 * DAY + 3_000) / 3_000;
        let real = ticks((0..=count).map(|i| i * 3_000));
        let small = ticks((0..=count).map(|i| i * 3_000 / SCALE));
        let to_real = |tick: Tick| Tick::new(tick.get() * SCALE);

        let expected = production(&real);
        let actual = plan(&small, scaled);
        assert_eq!(
            actual
                .keep
                .iter()
                .copied()
                .map(to_real)
                .collect::<BTreeSet<_>>(),
            expected.keep
        );
        assert_eq!(
            actual.delete.into_iter().map(to_real).collect::<Vec<_>>(),
            expected.delete
        );
        assert_eq!(to_real(actual.journal_floor), expected.journal_floor);
    }

    #[test]
    fn a_zero_hour_or_day_counts_as_one_tick() {
        let policy = RetentionPolicy {
            hour: Ticks::new(0),
            day: Ticks::new(0),
            ..RetentionPolicy::default()
        };
        let plan = plan(&ticks([0, 1, 2]), policy);
        assert_eq!(plan.keep, ticks([0, 1, 2]).into_iter().collect());
    }

    #[test]
    fn nothing_to_do_without_snapshots() {
        let plan = production(&[]);
        assert!(plan.keep.is_empty() && plan.delete.is_empty());
        assert_eq!(plan.journal_floor, Tick::ZERO);
    }

    #[test]
    fn one_snapshot_is_kept_and_its_journal_floor_is_its_tick() {
        let plan = production(&ticks([0]));
        assert_eq!(plan.keep, ticks([0]).into_iter().collect());
        assert_eq!(plan.journal_floor, Tick::ZERO);
    }

    #[test]
    fn within_an_hour_only_the_first_and_the_latest_two_remain() {
        let all = ticks((0..12).map(|i| i * 3_000));
        let plan = production(&all);
        assert_eq!(plan.keep, ticks([0, 30_000, 33_000]).into_iter().collect());
        // Younger than 48 h: the whole journal is kept (D-110).
        assert_eq!(plan.journal_floor, Tick::ZERO);
        assert_eq!(plan.delete.len(), 9);
    }

    #[test]
    fn hourly_for_48_hours_then_daily() {
        // Three days and five minutes of snapshots every 5 min, from the season start.
        let latest = 3 * DAY + 3_000;
        let all = ticks((0..=latest / 3_000).map(|i| i * 3_000));
        let plan = production(&all);

        // 48 h before the latest snapshot is tick 867,000, in the hour that starts at
        // 864,000 (day 1, hour 0). Before that hour: daily, which leaves only tick 0.
        let first_hour = (latest - WINDOW) / HOUR;
        assert_eq!(first_hour * HOUR, DAY);
        let mut expected: BTreeSet<Tick> = BTreeSet::new();
        expected.insert(Tick::new(0));
        // From then on: the first snapshot of each hour.
        for hour in first_hour..=latest / HOUR {
            expected.insert(Tick::new(hour * HOUR));
        }
        // The latest two.
        expected.insert(Tick::new(latest));
        expected.insert(Tick::new(latest - 3_000));
        assert_eq!(plan.keep, expected);
        // The journal starts at the hourly snapshot that opens the 48 h window (D-110).
        assert_eq!(plan.journal_floor, Tick::new(DAY));
        assert_eq!(plan.keep.len() + plan.delete.len(), all.len());
    }

    #[test]
    fn the_journal_covers_48_hours_from_a_kept_snapshot() {
        // D-110 at every step of five days of snapshots every 5 min: the journal starts at
        // a kept snapshot, covers at least 48 h (or everything, early on), at most one hour
        // more, and never starts after the older of the latest two snapshots.
        let mut kept: Vec<Tick> = Vec::new();
        for i in 0..=(5 * DAY) / 3_000 {
            let latest = Tick::new(i * 3_000);
            kept.push(latest);
            let plan = production(&kept);
            let floor = plan.journal_floor;
            assert!(
                plan.keep.contains(&floor),
                "floor {floor:?} is not a kept snapshot"
            );
            let covered = latest.since(floor).get();
            if latest.get() >= WINDOW {
                assert!(covered >= WINDOW, "at {latest:?}: {covered} ticks");
                assert!(covered < WINDOW + HOUR, "at {latest:?}: {covered} ticks");
            } else {
                assert_eq!(floor, Tick::ZERO);
            }
            let older_of_two = plan.keep.iter().rev().nth(1).copied().unwrap_or(latest);
            assert!(floor <= older_of_two);
            kept = plan.keep.into_iter().collect();
        }
    }

    #[test]
    fn sparse_snapshots_keep_the_journal_from_the_last_one_before_the_window() {
        // Snapshots a day apart: the journal starts at the newest one at least 48 h old.
        let all = ticks([0, DAY, 2 * DAY, 3 * DAY + 5]);
        assert_eq!(production(&all).journal_floor, Tick::new(DAY));
    }

    #[test]
    fn retention_is_stable_when_applied_repeatedly() {
        // Applying the plan after every snapshot gives the same result as applying it once
        // at the end: snapshots only ever lose their reason to be kept as time passes.
        let mut kept: Vec<Tick> = Vec::new();
        let every = ticks((0..=(3 * DAY) / 3_000).map(|i| i * 3_000));
        for &tick in &every {
            kept.push(tick);
            kept = production(&kept).keep.into_iter().collect();
        }
        assert_eq!(
            kept.into_iter().collect::<BTreeSet<_>>(),
            production(&every).keep
        );
    }
}
