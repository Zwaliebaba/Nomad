//! Snapshot and journal retention (Design doc 11 §8, D-048; N2 plan §4.4).
//!
//! Kept after every snapshot write:
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

use std::collections::BTreeSet;

use nomad_sim::{Tick, Ticks};

/// How many of the latest snapshots are always kept.
pub const KEEP_LATEST: usize = 2;
/// How far back from the latest snapshot hourly snapshots are kept.
pub const HOURLY_WINDOW: Ticks = Ticks::from_hours(48);
/// How far back from the latest snapshot the journal is kept (D-110; provisional until the
/// journal volume is measured).
pub const JOURNAL_WINDOW: Ticks = Ticks::from_hours(48);
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

    // The journal starts at the newest kept snapshot at or before the start of its window
    // (D-110), or at the oldest kept snapshot if every one is newer. Either way it is at or
    // before the older of the latest two, so that fallback stays possible.
    let window_start = Tick::new(latest.get().saturating_sub(JOURNAL_WINDOW.get()));
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
        // Younger than 48 h: the whole journal is kept (D-110).
        assert_eq!(plan.journal_floor, Tick::ZERO);
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
            let plan = plan(&kept);
            let floor = plan.journal_floor;
            assert!(
                plan.keep.contains(&floor),
                "floor {floor:?} is not a kept snapshot"
            );
            let covered = latest.since(floor).get();
            if latest.get() >= JOURNAL_WINDOW.get() {
                assert!(
                    covered >= JOURNAL_WINDOW.get(),
                    "at {latest:?}: {covered} ticks"
                );
                assert!(
                    covered < JOURNAL_WINDOW.get() + HOUR,
                    "at {latest:?}: {covered} ticks"
                );
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
        assert_eq!(plan(&all).journal_floor, Tick::new(DAY));
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
