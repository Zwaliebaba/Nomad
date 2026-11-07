//! The exposure window: its occurrences, how it follows activity, the free choice of the
//! first 7 days and the freeze (D-032, D-154 to D-157, D-171; S6 plan §4.3).
//!
//! - **Occurrences.** A window is a start, a quarter hour of the UTC day, and a length (4 h).
//!   The record holds the occurrence that is open now or opens next, as deadline ticks
//!   (doc 11 §3). At creation it is the occurrence open at that moment, if any, else the
//!   next one.
//! - **It moves only when it closes** (D-155), at its end deadline, so it never changes while
//!   open. The next occurrence is the first that opens at or after the close. *Derived:* a
//!   window moved later therefore does not reopen at once; without this, a window moved by
//!   1 h at its close would be open again and move again within the hour.
//! - **Where it moves:**
//!   - while a defence is pending (a holding Reinforced or under siege, S6b), nowhere
//!     (D-156); a chosen start waits;
//!   - to the start chosen with `SetExposureWindow`, if there is one (D-157);
//!   - otherwise towards the 4 h with the most activity over the history, by at most 1 h,
//!     the shorter way round. Ties keep the current start, then prefer the nearest, then
//!     the earliest (D-155).
//! - **The free choice** (D-157): `SetExposureWindow` is accepted during the commander's
//!   first 7 days and takes effect when the window next closes. *Reading of D-032 and
//!   D-157, as the S6 plan §4.3 writes it:* activity moves the window from the start too;
//!   a chosen start replaces that day's drift.
//! - **Sampling** (D-154, D-171): every 60 s per commander, staggered (doc 11 §4.1 phase 9).
//!   A minute counts when the commander is online at the sample, which during catch-up
//!   nobody is.
//!
//! *Proposed* where D-155 leaves it open: "the earliest" among equally near starts is the
//! one earlier in the UTC day (the lower quarter-hour index), and a target exactly half a day
//! away is approached by moving earlier.

use nomad_types::{Owner, QuarterHour};

use super::{Presence, free_choice};
use crate::announce::{Announcement, AnnouncementKind, Recipients};
use crate::command::CommandRejection;
use crate::id::CommanderId;
use crate::state::UniverseState;
use crate::time::{Tick, Ticks, runs_on};

use super::Deadline;

const MILLIS_PER_DAY: i64 = 86_400_000;
/// Activity is sampled once a minute (doc 11 §4.1, §11; D-154).
pub(super) const SAMPLE_INTERVAL_S: u64 = 60;
/// Mixed into the sampling key, so commanders' samples do not share slots with other rate
/// groups keyed by small numbers.
const SAMPLE_KEY: u64 = 0x5341_4d50_0000_0000;

/// The rate-group key of `commander`'s activity samples ([`runs_on`]).
pub(super) fn sample_key(commander: CommanderId) -> u64 {
    SAMPLE_KEY | u64::from(commander.0)
}

/// Whether any of `commander`'s holdings is Reinforced or under siege, which freezes the
/// window (D-156). **The S6b interface:** holding defence states arrive with S6b, which
/// answers from them here. Until then no defence is ever pending.
pub(super) fn defence_pending(_state: &UniverseState, _commander: CommanderId) -> bool {
    false
}

/// The window's length in milliseconds.
fn length_ms(state: &UniverseState) -> i64 {
    i64::from(state.rules.sieges.window_minutes) * 60_000
}

/// The start of the UTC day of `unix_ms`.
fn day_start(unix_ms: i64) -> i64 {
    unix_ms - unix_ms.rem_euclid(MILLIS_PER_DAY)
}

/// The occurrence of a window starting at `start` that is open at `tick`, else the next one
/// to open: `(opens, closes)` as ticks. An occurrence that began before the season start
/// opens at tick 0.
pub(super) fn occurrence_at(state: &UniverseState, tick: Tick, start: QuarterHour) -> (Tick, Tick) {
    let now_ms = state.clock.universe_time_ms(tick);
    let mut open_ms = day_start(now_ms) + start.time_of_day_ms().rem_euclid(MILLIS_PER_DAY);
    if open_ms > now_ms {
        open_ms -= MILLIS_PER_DAY;
    }
    if open_ms + length_ms(state) <= now_ms {
        open_ms += MILLIS_PER_DAY;
    }
    ticks_of(state, open_ms)
}

/// The first occurrence of a window starting at `start` that opens at or after Universe time
/// `from_ms`.
fn occurrence_from(state: &UniverseState, from_ms: i64, start: QuarterHour) -> (Tick, Tick) {
    let mut open_ms = day_start(from_ms) + start.time_of_day_ms().rem_euclid(MILLIS_PER_DAY);
    if open_ms < from_ms {
        open_ms += MILLIS_PER_DAY;
    }
    ticks_of(state, open_ms)
}

fn ticks_of(state: &UniverseState, open_ms: i64) -> (Tick, Tick) {
    let clock = state.clock;
    (
        clock.first_tick_at_or_after(open_ms),
        clock.first_tick_at_or_after(open_ms + length_ms(state)),
    )
}

/// The start a window of `length` quarter hours moves to from `current`, given the minutes
/// online per quarter hour (`totals`, one per quarter hour of the day), by at most
/// `max_step` quarter hours (D-155).
fn drifted_start(totals: &[u32], current: u8, length: u8, max_step: u8) -> u8 {
    let n = QuarterHour::PER_DAY;
    let current = current % n;
    let score = |start: u8| -> u32 {
        (0..length)
            .map(|i| {
                let quarter = (u16::from(start) + u16::from(i)) % u16::from(n);
                totals.get(usize::from(quarter)).copied().unwrap_or(0)
            })
            .fold(0, u32::saturating_add)
    };
    let best = (0..n).map(score).max().unwrap_or(0);
    if score(current) == best {
        return current;
    }
    // Forward and backward distances round the day.
    let forward = |to: u8| (to + n - current) % n;
    let backward = |to: u8| (current + n - to) % n;
    let distance = |to: u8| forward(to).min(backward(to));
    let Some(target) = (0..n)
        .filter(|&s| score(s) == best)
        .min_by_key(|&s| (distance(s), s))
    else {
        return current;
    };
    if forward(target) < backward(target) {
        (current + forward(target).min(max_step)) % n
    } else {
        (current + n - backward(target).min(max_step)) % n
    }
}

/// The window of `commander` closes on `now` (D-155): it moves, unless it is `frozen` because
/// a defence is pending ([`defence_pending`], D-156), and its next occurrence is scheduled.
pub(super) fn close(
    state: &mut UniverseState,
    now: Tick,
    commander: CommanderId,
    frozen: bool,
    announcements: &mut Vec<Announcement>,
) {
    let unix_ms = state.clock.universe_time_ms(now);
    let sieges = &state.rules.sieges;
    let (length, max_step, days) = (
        sieges.window_quarters(),
        sieges.drift_quarters(),
        sieges.activity_history_days,
    );
    let Some(record) = state.commanders.records.get_mut(&commander) else {
        return;
    };
    let current = record.window.start;
    let start = if frozen {
        current
    } else if let Some(chosen) = record.window.chosen.take() {
        chosen
    } else {
        let totals = record.activity.totals(unix_ms, days);
        QuarterHour::new(drifted_start(&totals, current.index(), length, max_step))
            .unwrap_or(current)
    };
    // The close falls on a quarter hour; the tick may lie a few milliseconds after it.
    let close_ms = unix_ms - unix_ms.rem_euclid(QuarterHour::MILLIS);
    let (opens, closes) = occurrence_from(state, close_ms, start);
    if let Some(record) = state.commanders.records.get_mut(&commander) {
        record.window.start = start;
        record.window.opens = opens;
        record.window.closes = closes;
    }
    state
        .commanders
        .deadlines
        .schedule(closes, commander, Deadline::WindowCloses);
    if start != current {
        announcements.push(Announcement {
            tick: now,
            to: Recipients::Public,
            kind: AnnouncementKind::WindowMoved {
                commander,
                start,
                opens,
                closes,
            },
        });
    }
}

/// Checks `SetExposureWindow` from `issuer` (D-157): a commander of this Universe, a quarter
/// hour of the day, and still inside the first 7 days on the tick the command applies.
pub(crate) fn validate_window_choice(
    state: &UniverseState,
    issuer: Owner,
    start: QuarterHour,
) -> Result<(), CommandRejection> {
    let record = issuer
        .commander()
        .and_then(|c| state.commanders.records.get(&c))
        .ok_or(CommandRejection::UnknownCommander)?;
    if !start.is_valid() {
        return Err(CommandRejection::InvalidWindow);
    }
    let applies_on = state.tick.next();
    if applies_on >= record.joined.after(free_choice(state)) {
        return Err(CommandRejection::WindowLocked);
    }
    Ok(())
}

/// Carries out a validated `SetExposureWindow`: the start applies when the window next
/// closes (D-157). Choosing the current start withdraws an earlier choice.
pub(crate) fn choose_window(state: &mut UniverseState, issuer: Owner, start: QuarterHour) {
    let Some(record) = issuer
        .commander()
        .and_then(|c| state.commanders.records.get_mut(&c))
    else {
        return;
    };
    record.window.chosen = (start != record.window.start).then_some(start);
}

/// Phase 9: the 60 s activity samples due on `now`, staggered per commander (D-154). Catch-up
/// minutes count as offline (D-171): nobody is online then.
pub(crate) fn sample_activity(state: &mut UniverseState, now: Tick) {
    let period = Ticks::from_secs(SAMPLE_INTERVAL_S);
    let unix_ms = state.clock.universe_time_ms(now);
    let days = state.rules.sieges.activity_history_days;
    for (id, record) in &mut state.commanders.records {
        if !runs_on(now, period, sample_key(*id)) {
            continue;
        }
        let online = matches!(record.presence, Presence::Online { .. });
        record.activity.sample(unix_ms, online, days);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn totals(active: &[(u8, u32)]) -> Vec<u32> {
        let mut totals = vec![0; 96];
        for &(quarter, minutes) in active {
            totals[usize::from(quarter)] = minutes;
        }
        totals
    }

    /// Activity in 16 quarter hours from `start`.
    fn block(start: u8) -> Vec<u32> {
        let mut t = vec![0; 96];
        for i in 0..16 {
            t[usize::from((start + i) % 96)] = 15;
        }
        t
    }

    #[test]
    fn drift_moves_at_most_one_hour_the_shorter_way() {
        // Window at 19:00 (76), activity at 13:00 (52): 6 h earlier, 4 quarters a day.
        assert_eq!(drifted_start(&block(52), 76, 16, 4), 72);
        assert_eq!(drifted_start(&block(52), 56, 16, 4), 52);
        // Activity at 01:00 (4) from 23:00 (92): forward round midnight.
        assert_eq!(drifted_start(&block(4), 92, 16, 4), 0);
        assert_eq!(drifted_start(&block(4), 0, 16, 4), 4);
        // Already there: no move.
        assert_eq!(drifted_start(&block(52), 52, 16, 4), 52);
    }

    #[test]
    fn ties_keep_the_current_then_the_nearest_then_the_earliest() {
        // No activity at all: every start ties, the window stays.
        assert_eq!(drifted_start(&vec![0; 96], 76, 16, 4), 76);
        // One active quarter hour (40) is covered by starts 25–40, all tied. The current
        // start 30 is among them: it stays.
        assert_eq!(drifted_start(&totals(&[(40, 5)]), 30, 16, 4), 30);
        // From 60 the nearest tied start is 40.
        assert_eq!(drifted_start(&totals(&[(40, 5)]), 60, 16, 4), 56);
        // Two equally active blocks, at 16 and at 48.
        let mut two = block(16);
        for (t, b) in two.iter_mut().zip(block(48)) {
            *t += b;
        }
        // From 32, the best starts 16 and 48 lie 16 quarters back and ahead: the earlier, 16.
        assert_eq!(drifted_start(&two, 32, 16, 4), 28);
        // Exactly half a day away: move earlier.
        assert_eq!(drifted_start(&block(28), 76, 16, 4), 72);
        // Malformed inputs do not panic.
        assert_eq!(
            drifted_start(&[1, 2, 3], 200, 16, 4),
            drifted_start(&[1, 2, 3], 8, 16, 4)
        );
    }
}
