//! Clocks (Design doc 11 §2–3; D-019, D-036, D-047).
//!
//! The simulation knows only the [`Tick`]. Universe time (UTC) is computed from it by
//! [`SeasonClock`], so calendar rules stay deterministic. Nothing here reads a real clock.
//!
//! The clock types live in `nomad-types` (D-086, doc 20 §3), because clients need Universe
//! time too, and are re-exported here. Rate groups ([`runs_on`]) are simulation scheduling
//! and stay in this crate.

pub use nomad_types::{SeasonClock, TICK_MILLIS, TICKS_PER_SECOND, Tick, Ticks};

use crate::util::splitmix64;

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
