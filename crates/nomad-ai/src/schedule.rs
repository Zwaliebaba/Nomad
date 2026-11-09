//! AI online schedules (Design doc 13 §6; D-080, D-157, D-204; A2 plan §4.8, A1 plan §4.7).
//!
//! The simulation generates one schedule per AI commander from the season seed when the
//! Universe is created ([`generate`], with the commander's `AI_SCHEDULE` stream), keeps it in
//! the commander's record (not the journal: replaying the inputs reproduces it, doc 11 §6),
//! and drives the commander's presence and initial exposure window from it (D-157, D-204).
//! The harness's `SetPresence` overrides it. A5 tunes it.
//!
//! **The shape** *(Agreed, D-080):* 2–3 sessions a day, 1.5–4 h in total, at consistent
//! times, with weekend variation. Times are UTC.
//!
//! *Proposed* readings, where D-080 gives no numbers:
//! - one pattern for Monday to Friday, and one for Saturday and Sunday ("consistent times",
//!   "weekend variation");
//! - each day has an evening session starting 18:00–21:00; with 3 sessions also a morning one
//!   starting 06:00–09:00 and a midday one starting 11:30–13:30; with 2, one of those two;
//! - the day's total is 1.5–4 h in quarter hours, each session at least 30 min, with at
//!   least 1 h between one session's end and the next one's start, and none past midnight;
//! - the weekend keeps the weekday's number of sessions, starts each of them 0–2 h later
//!   (the same shift for all) and draws its own total.
//!
//! Every draw is uniform. Days start at 00:00 UTC.

use serde::{Deserialize, Serialize};

use crate::stream::Stream;

const MINUTES_PER_DAY: u32 = 24 * 60;
const MILLIS_PER_MINUTE: i64 = 60_000;
const MILLIS_PER_DAY: i64 = 86_400_000;
/// Starts and lengths are whole quarter hours.
const QUARTER: u32 = 15;
/// Each session lasts at least this long, minutes.
const MIN_SESSION: u32 = 30;
/// Between two sessions of a day, minutes.
const MIN_GAP: u32 = 60;
/// A day's total online time, minutes (D-080).
const MIN_TOTAL: u32 = 90;
const MAX_TOTAL: u32 = 240;
/// The windows session starts are drawn from, as (earliest, latest) minutes of the day.
const MORNING: (u32, u32) = (6 * 60, 9 * 60);
const MIDDAY: (u32, u32) = (11 * 60 + 30, 13 * 60 + 30);
const EVENING: (u32, u32) = (18 * 60, 21 * 60);
/// The weekend's sessions start up to this much later, minutes.
const WEEKEND_SHIFT: u32 = 120;

/// One online session of a day, UTC.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Session {
    /// When it starts, minutes after 00:00 UTC.
    pub start_min: u32,
    /// How long it lasts, minutes.
    pub length_min: u32,
}

impl Session {
    /// When it ends, minutes after 00:00 UTC.
    pub const fn end_min(self) -> u32 {
        self.start_min + self.length_min
    }
}

/// An AI commander's weekly schedule (D-080).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Schedule {
    /// Monday to Friday, in time order.
    pub weekday: Vec<Session>,
    /// Saturday and Sunday, in time order.
    pub weekend: Vec<Session>,
}

/// A schedule from the commander's schedule stream.
pub fn generate(stream: &mut Stream) -> Schedule {
    let count = stream.between(2, 3);
    let mut windows = vec![EVENING];
    if count == 3 {
        windows.insert(0, MIDDAY);
        windows.insert(0, MORNING);
    } else if stream.below(2) == 0 {
        windows.insert(0, MORNING);
    } else {
        windows.insert(0, MIDDAY);
    }
    let starts: Vec<u32> = windows
        .iter()
        .map(|&(lo, hi)| draw_quarter(stream, lo, hi))
        .collect();
    let weekday = sessions(stream, &starts);
    let shift = draw_quarter(stream, 0, WEEKEND_SHIFT);
    let weekend_starts: Vec<u32> = starts.iter().map(|s| s + shift).collect();
    let weekend = sessions(stream, &weekend_starts);
    Schedule { weekday, weekend }
}

/// A whole quarter hour in `lo..=hi`, minutes.
fn draw_quarter(stream: &mut Stream, lo: u32, hi: u32) -> u32 {
    let steps = u64::from((hi - lo) / QUARTER);
    lo + u32::try_from(stream.between(0, steps)).unwrap_or(0) * QUARTER
}

/// Sessions at `starts` (ascending), sharing a drawn day's total.
fn sessions(stream: &mut Stream, starts: &[u32]) -> Vec<Session> {
    // The room each session has before the next one (or midnight), minus the gap.
    let room: Vec<u32> = starts
        .iter()
        .enumerate()
        .map(|(i, &start)| match starts.get(i + 1) {
            Some(&next) => next.saturating_sub(start + MIN_GAP),
            None => MINUTES_PER_DAY.saturating_sub(start),
        })
        .collect();
    let mut lengths: Vec<u32> = room.iter().map(|&r| r.min(MIN_SESSION)).collect();
    let total = draw_quarter(stream, MIN_TOTAL, MAX_TOTAL);
    let mut left = total.saturating_sub(lengths.iter().sum());
    // Hand out the rest a quarter hour at a time, each to a session with room for it.
    while left >= QUARTER {
        let open: Vec<usize> = (0..lengths.len())
            .filter(|&i| lengths[i] + QUARTER <= room[i])
            .collect();
        if open.is_empty() {
            break;
        }
        let pick =
            usize::try_from(stream.below(u64::try_from(open.len()).unwrap_or(1))).unwrap_or(0);
        lengths[open[pick]] += QUARTER;
        left -= QUARTER;
    }
    starts
        .iter()
        .zip(lengths)
        .map(|(&start_min, length_min)| Session {
            start_min,
            length_min,
        })
        .collect()
}

/// Monday is 0. 1970-01-01, day 0 of Unix time, was a Thursday.
fn weekday(day: i64) -> i64 {
    (day + 3).rem_euclid(7)
}

impl Schedule {
    /// The sessions of the UTC day that contains `unix_ms`.
    fn day(&self, day: i64) -> &[Session] {
        if weekday(day) >= 5 {
            &self.weekend
        } else {
            &self.weekday
        }
    }

    /// Every session of `days` days starting with the UTC day of `unix_ms`, as `(start, end)`
    /// in Unix milliseconds, in time order.
    fn spans(&self, unix_ms: i64, days: i64) -> impl Iterator<Item = (i64, i64)> + '_ {
        let first = unix_ms.div_euclid(MILLIS_PER_DAY);
        (first..first + days).flat_map(move |day| {
            let base = day * MILLIS_PER_DAY;
            self.day(day).iter().map(move |s| {
                (
                    base + i64::from(s.start_min) * MILLIS_PER_MINUTE,
                    base + i64::from(s.end_min()) * MILLIS_PER_MINUTE,
                )
            })
        })
    }

    /// The session open at `unix_ms`, as `(start, end)` in Unix milliseconds.
    pub fn session_at(&self, unix_ms: i64) -> Option<(i64, i64)> {
        self.spans(unix_ms - MILLIS_PER_DAY, 2)
            .find(|&(start, end)| start <= unix_ms && unix_ms < end)
    }

    /// The first session that starts after `unix_ms`, as `(start, end)`.
    pub fn next_session(&self, unix_ms: i64) -> Option<(i64, i64)> {
        self.spans(unix_ms, 9).find(|&(start, _)| start > unix_ms)
    }

    /// When the schedule next changes from online to offline or back after `unix_ms`.
    pub fn next_change(&self, unix_ms: i64) -> Option<i64> {
        match self.session_at(unix_ms) {
            Some((_, end)) => Some(end),
            None => self.next_session(unix_ms).map(|(start, _)| start),
        }
    }

    /// The quarter hour of the UTC day (0–95) a `length_min` window should start at to cover
    /// the most scheduled minutes over a week; the earliest of equals. *Proposed* reading of
    /// D-157's "an AI's from its schedule", like the activity-derived window (D-032).
    pub fn window_start(&self, length_min: u32) -> u8 {
        let mut minutes = vec![0_u32; MINUTES_PER_DAY as usize];
        for (days, sessions) in [(5, &self.weekday), (2, &self.weekend)] {
            for s in sessions {
                for m in s.start_min..s.end_min().min(MINUTES_PER_DAY) {
                    minutes[m as usize] += days;
                }
            }
        }
        let covered = |start: u32| -> u32 {
            (start..start + length_min)
                .map(|m| minutes[(m % MINUTES_PER_DAY) as usize])
                .sum()
        };
        let mut best = (0_u32, 0_u8);
        for q in 0..96_u8 {
            let c = covered(u32::from(q) * QUARTER);
            if c > best.0 {
                best = (c, q);
            }
        }
        best.1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check_day(sessions: &[Session]) {
        assert!((2..=3).contains(&sessions.len()), "{sessions:?}");
        let total: u32 = sessions.iter().map(|s| s.length_min).sum();
        assert!((MIN_TOTAL..=MAX_TOTAL).contains(&total), "{sessions:?}");
        for pair in sessions.windows(2) {
            assert!(
                pair[0].end_min() + MIN_GAP <= pair[1].start_min,
                "{sessions:?}"
            );
        }
        for s in sessions {
            assert!(s.length_min >= MIN_SESSION && s.end_min() <= MINUTES_PER_DAY);
            assert_eq!(s.start_min % QUARTER, 0);
            assert_eq!(s.length_min % QUARTER, 0);
        }
    }

    #[test]
    fn schedules_keep_d_080s_shape() {
        for seed in 0..200_u8 {
            let schedule = generate(&mut Stream::from_seed([seed; 32]));
            check_day(&schedule.weekday);
            check_day(&schedule.weekend);
            assert_eq!(schedule.weekday.len(), schedule.weekend.len());
            assert!(
                schedule
                    .weekday
                    .last()
                    .is_some_and(|s| s.start_min >= EVENING.0)
            );
        }
    }

    #[test]
    fn sessions_and_changes_follow_the_week() {
        let schedule = Schedule {
            weekday: vec![
                Session {
                    start_min: 7 * 60 + 30,
                    length_min: 60,
                },
                Session {
                    start_min: 19 * 60,
                    length_min: 90,
                },
            ],
            weekend: vec![Session {
                start_min: 10 * 60,
                length_min: 120,
            }],
        };
        // 2026-11-09 is a Monday.
        let monday = 1_794_182_400_000_i64;
        assert_eq!(weekday(monday / MILLIS_PER_DAY), 0);
        let at = |h: i64, m: i64| monday + (h * 60 + m) * MILLIS_PER_MINUTE;
        assert_eq!(schedule.session_at(at(7, 0)), None);
        assert_eq!(schedule.next_change(at(7, 0)), Some(at(7, 30)));
        assert_eq!(schedule.session_at(at(8, 0)), Some((at(7, 30), at(8, 30))));
        assert_eq!(schedule.next_change(at(8, 0)), Some(at(8, 30)));
        assert_eq!(
            schedule.next_session(at(8, 0)),
            Some((at(19, 0), at(20, 30)))
        );
        // Friday evening to Saturday's late morning session.
        let friday = monday + 4 * MILLIS_PER_DAY;
        let saturday_10 = friday + MILLIS_PER_DAY + 10 * 60 * MILLIS_PER_MINUTE;
        assert_eq!(
            schedule.next_session(friday + 21 * 60 * MILLIS_PER_MINUTE),
            Some((saturday_10, saturday_10 + 120 * MILLIS_PER_MINUTE))
        );
        // A 4 h window from 07:30 covers 5 × 60 weekday and 2 × 90 weekend minutes, more than
        // the evening's 5 × 90.
        assert_eq!(schedule.window_start(240), 30);
    }
}
