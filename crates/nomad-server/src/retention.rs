//! Snapshot and journal retention (Design doc 11 §8, D-048; N2 plan §4.4).
//!
//! Kept after every snapshot write:
//!
//! - the latest 2 snapshots, plus the journal after the older of them, so that recovery can
//!   fall back to it if the latest one is damaged;
//! - for the 48 h of Universe time before the latest snapshot, the first snapshot of each
//!   hour;
//! - before that, the first snapshot of each day, for the rest of the season.
//!
//! Older journal entries are deleted. Hours and days count from the season start (tick 0),
//! so the periodic snapshots every 3,000 ticks (5 min) fall on their boundaries.

use std::collections::BTreeSet;

use nomad_sim::{Tick, Ticks};

/// How many of the latest snapshots are always kept, with their journal.
pub const KEEP_LATEST: usize = 2;
/// How far back from the latest snapshot hourly snapshots are kept.
pub const HOURLY_WINDOW: Ticks = Ticks::from_hours(48);
const HOUR: u64 = Ticks::from_hours(1).get();
const DAY: u64 = Ticks::from_days(1).get();

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

/// Applies doc 11 §8 to the snapshots at `ticks` (any order, no duplicates).
pub fn plan(ticks: &[Tick]) -> RetentionPlan {
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

    let mut keep: BTreeSet<Tick> = sorted.iter().rev().take(KEEP_LATEST).copied().collect();
    let journal_floor = keep.first().copied().unwrap_or(latest);
    // The hourly window starts at the hour that contains the point 48 h before the latest
    // snapshot. Aligning it to whole hours makes retention give the same result whether it
    // runs after every snapshot or once at the end: a snapshot deleted once would never be
    // kept later.
    let first_hourly_bucket = latest.get().saturating_sub(HOURLY_WINDOW.get()) / HOUR;

    // `sorted` is ascending, so the first snapshot seen in a bucket is its earliest.
    let mut last_hour = None;
    let mut last_day = None;
    for &tick in &sorted {
        let (bucket, last) = if tick.get() / HOUR >= first_hourly_bucket {
            (tick.get() / HOUR, &mut last_hour)
        } else {
            (tick.get() / DAY, &mut last_day)
        };
        if *last != Some(bucket) {
            *last = Some(bucket);
            keep.insert(tick);
        }
    }

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

    fn ticks(raw: impl IntoIterator<Item = u64>) -> Vec<Tick> {
        raw.into_iter().map(Tick::new).collect()
    }

    #[test]
    fn nothing_to_do_without_snapshots() {
        let plan = plan(&[]);
        assert!(plan.keep.is_empty() && plan.delete.is_empty());
        assert_eq!(plan.journal_floor, Tick::ZERO);
    }

    #[test]
    fn one_snapshot_is_kept_and_its_journal_floor_is_its_tick() {
        let plan = plan(&ticks([0]));
        assert_eq!(plan.keep, ticks([0]).into_iter().collect());
        assert_eq!(plan.journal_floor, Tick::ZERO);
    }

    #[test]
    fn within_an_hour_only_the_first_and_the_latest_two_remain() {
        let all = ticks((0..12).map(|i| i * 3_000));
        let plan = plan(&all);
        assert_eq!(plan.keep, ticks([0, 30_000, 33_000]).into_iter().collect());
        assert_eq!(plan.journal_floor, Tick::new(30_000));
        assert_eq!(plan.delete.len(), 9);
    }

    #[test]
    fn hourly_for_48_hours_then_daily() {
        // Three days and five minutes of snapshots every 5 min, from the season start.
        let latest = 3 * DAY + 3_000;
        let all = ticks((0..=latest / 3_000).map(|i| i * 3_000));
        let plan = plan(&all);

        // 48 h before the latest snapshot is tick 867,000, in the hour that starts at
        // 864,000 (day 1, hour 0). Before that hour: daily, which leaves only tick 0.
        let first_hour = (latest - HOURLY_WINDOW.get()) / HOUR;
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
        assert_eq!(plan.journal_floor, Tick::new(latest - 3_000));
        assert_eq!(plan.keep.len() + plan.delete.len(), all.len());
    }

    #[test]
    fn retention_is_stable_when_applied_repeatedly() {
        // Applying the plan after every snapshot gives the same result as applying it once
        // at the end: snapshots only ever lose their reason to be kept as time passes.
        let mut kept: Vec<Tick> = Vec::new();
        let every = ticks((0..=(3 * DAY) / 3_000).map(|i| i * 3_000));
        for &tick in &every {
            kept.push(tick);
            kept = plan(&kept).keep.into_iter().collect();
        }
        assert_eq!(kept.into_iter().collect::<BTreeSet<_>>(), plan(&every).keep);
    }
}
