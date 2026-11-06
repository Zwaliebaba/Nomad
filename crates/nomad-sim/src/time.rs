//! Clocks (Design doc 11 §2–3; D-019, D-036, D-047).
//!
//! The simulation knows only the [`Tick`]. Universe time (UTC) is computed from it by
//! [`SeasonClock`], so calendar rules stay deterministic. Nothing here reads a real clock.

use serde::{Deserialize, Serialize};

use crate::util::splitmix64;

/// Length of one tick in milliseconds (10 Hz, D-019).
pub const TICK_MILLIS: u64 = 100;
/// Ticks per second.
pub const TICKS_PER_SECOND: u64 = 1_000 / TICK_MILLIS;

const MILLIS_PER_DAY: i64 = 86_400_000;

/// A point in simulation time: the number of ticks since season start.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub struct Tick(u64);

impl Tick {
    /// The season start.
    pub const ZERO: Self = Self(0);

    /// Creates a tick from its raw value.
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    /// The raw value.
    pub const fn get(self) -> u64 {
        self.0
    }

    /// The following tick.
    #[must_use]
    pub fn next(self) -> Self {
        Self(
            self.0
                .checked_add(1)
                .expect("invariant: the u64 tick counter never overflows"),
        )
    }

    /// This tick plus a duration (saturating).
    #[must_use]
    pub const fn after(self, duration: Ticks) -> Self {
        Self(self.0.saturating_add(duration.0))
    }

    /// The duration from `earlier` to this tick (zero if `earlier` is later).
    pub const fn since(self, earlier: Self) -> Ticks {
        Ticks(self.0.saturating_sub(earlier.0))
    }
}

/// A duration in ticks.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub struct Ticks(u64);

impl Ticks {
    /// Creates a duration from a raw tick count.
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    /// The raw tick count.
    pub const fn get(self) -> u64 {
        self.0
    }

    /// From milliseconds, rounded **up** so that a deadline is never early.
    pub const fn from_millis(millis: u64) -> Self {
        Self(millis.div_ceil(TICK_MILLIS))
    }

    /// From whole seconds.
    pub const fn from_secs(secs: u64) -> Self {
        Self(secs.saturating_mul(TICKS_PER_SECOND))
    }

    /// From whole minutes.
    pub const fn from_minutes(minutes: u64) -> Self {
        Self::from_secs(minutes.saturating_mul(60))
    }

    /// From whole hours.
    pub const fn from_hours(hours: u64) -> Self {
        Self::from_minutes(hours.saturating_mul(60))
    }

    /// From whole days.
    pub const fn from_days(days: u64) -> Self {
        Self::from_hours(days.saturating_mul(24))
    }
}

/// Maps ticks to Universe time: UTC milliseconds since the Unix epoch (Design doc 11 §2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeasonClock {
    season_start_unix_ms: i64,
}

impl SeasonClock {
    /// A clock whose tick 0 is `season_start_unix_ms`.
    pub const fn new(season_start_unix_ms: i64) -> Self {
        Self {
            season_start_unix_ms,
        }
    }

    /// Universe time of tick 0.
    pub const fn season_start_unix_ms(self) -> i64 {
        self.season_start_unix_ms
    }

    /// Universe time at the start of `tick`.
    pub fn universe_time_ms(self, tick: Tick) -> i64 {
        let offset = i64::try_from(tick.get().saturating_mul(TICK_MILLIS)).unwrap_or(i64::MAX);
        self.season_start_unix_ms.saturating_add(offset)
    }

    /// The first tick whose Universe time is at or after `unix_ms`.
    pub fn first_tick_at_or_after(self, unix_ms: i64) -> Tick {
        let delta = unix_ms.saturating_sub(self.season_start_unix_ms);
        match u64::try_from(delta) {
            Ok(delta) => Tick(delta.div_ceil(TICK_MILLIS)),
            // A time before the season start maps to the season start.
            Err(_) => Tick::ZERO,
        }
    }

    /// The first tick at or after `from` whose UTC time of day is `time_of_day_ms` or later.
    ///
    /// The building block for calendar rules such as exposure windows (D-015, D-032).
    /// `time_of_day_ms` is taken modulo one day.
    pub fn next_time_of_day(self, from: Tick, time_of_day_ms: u32) -> Tick {
        let now_ms = self.universe_time_ms(from);
        let day_start = now_ms - now_ms.rem_euclid(MILLIS_PER_DAY);
        let mut candidate = day_start + i64::from(time_of_day_ms).rem_euclid(MILLIS_PER_DAY);
        if candidate < now_ms {
            candidate += MILLIS_PER_DAY;
        }
        self.first_tick_at_or_after(candidate)
    }
}

/// Whether a periodic job identified by `key` runs on `tick` (Design doc 11 §4.1).
///
/// Jobs with the same period are spread evenly across ticks by a stable hash of their key,
/// so a rate group's load does not land on a single tick. A period of 0 or 1 runs every tick.
pub fn runs_on(tick: Tick, period: Ticks, key: u64) -> bool {
    let period = period.get();
    period <= 1
        || tick
            .get()
            .wrapping_add(splitmix64(key))
            .is_multiple_of(period)
}

#[cfg(test)]
mod tests {
    use super::*;

    // 2026-11-06T00:00:00Z
    const DAY0: i64 = 1_793_923_200_000;

    #[test]
    fn durations_round_up() {
        assert_eq!(Ticks::from_millis(0).get(), 0);
        assert_eq!(Ticks::from_millis(1).get(), 1);
        assert_eq!(Ticks::from_millis(100).get(), 1);
        assert_eq!(Ticks::from_millis(101).get(), 2);
        assert_eq!(Ticks::from_hours(12).get(), 432_000);
    }

    #[test]
    fn universe_time_round_trips() {
        let clock = SeasonClock::new(DAY0 + 50);
        let tick = Tick::new(12_345);
        let t = clock.universe_time_ms(tick);
        assert_eq!(t, DAY0 + 50 + 1_234_500);
        assert_eq!(clock.first_tick_at_or_after(t), tick);
        assert_eq!(clock.first_tick_at_or_after(t + 1), tick.next());
        assert_eq!(clock.first_tick_at_or_after(DAY0 - 1), Tick::ZERO);
    }

    #[test]
    fn next_time_of_day_same_day_and_wrap() {
        let clock = SeasonClock::new(DAY0);
        let hour = 3_600_000;
        // From 10:00, the next 19:00 is the same day.
        let from = clock.first_tick_at_or_after(DAY0 + 10 * i64::from(hour));
        let t = clock.next_time_of_day(from, 19 * hour);
        assert_eq!(clock.universe_time_ms(t), DAY0 + 19 * i64::from(hour));
        // From 20:00, the next 19:00 is the following day.
        let from = clock.first_tick_at_or_after(DAY0 + 20 * i64::from(hour));
        let t = clock.next_time_of_day(from, 19 * hour);
        assert_eq!(clock.universe_time_ms(t), DAY0 + 43 * i64::from(hour));
        // Exactly at the time of day: that same tick.
        let at = clock.first_tick_at_or_after(DAY0 + 19 * i64::from(hour));
        assert_eq!(clock.next_time_of_day(at, 19 * hour), at);
    }

    #[test]
    fn rate_groups_spread_evenly() {
        let period = Ticks::new(10);
        let mut per_tick = [0_u32; 10];
        for key in 0..10_000_u64 {
            let hits: Vec<u64> = (0..10)
                .filter(|&t| runs_on(Tick::new(t), period, key))
                .collect();
            assert_eq!(hits.len(), 1, "each job runs exactly once per period");
            per_tick[usize::try_from(hits[0]).unwrap()] += 1;
        }
        assert!(per_tick.iter().all(|&n| (800..=1_200).contains(&n)));
    }
}
