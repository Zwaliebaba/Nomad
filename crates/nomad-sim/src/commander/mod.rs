//! Commanders: records, presence, the disconnect grace, activity and exposure windows
//! (Design doc 12 §4, §5.3; doc 11 §2, §4.1, §6, §7; S6 plan §4.1–§4.3, §4.8; S6a).
//!
//! **Records** *(S6 plan §4.1; layout Proposed there):* every commander of the Universe has
//! one, made at creation from the roster: kind (human or AI), presence, the pending end of
//! the disconnect grace, the exposure window and the activity history.
//!
//! **Presence** *(Derived from doc 11 §6, doc 12 §5.3, D-047, D-080; [`presence`]):*
//! - a human is online while connected, between the journaled `Connected` and
//!   `Disconnected` inputs;
//! - an AI is online by its schedule, which A5 supplies. Until then the harness sets it with
//!   the operator input `SetPresence`, which only an AI commander accepts;
//! - during catch-up nobody is online, whatever the schedule says (D-047, D-080).
//!
//! **The disconnect grace** *(doc 12 §5.3; D-160 to D-162):* a commander who goes offline
//! keeps their direct fleet orders for 120 s, a deadline tick. If they are still offline
//! then, the orders lapse and every fleet falls back to doctrine, which until the Deputy (A1)
//! means station-keeping. Inside the grace the commander still counts as online for siege
//! rules (D-160), except during catch-up.
//!
//! **Activity** *(D-154, D-171; [`activity`]):* every 60 s, staggered per commander (doc 11
//! §4.1 phase 9), a commander who is online adds a minute to the current quarter hour of the
//! current UTC day. 14 days are kept. Catch-up minutes count as offline.
//!
//! **The exposure window** *(D-032, D-155 to D-157; [`window`]):* 4 h starting on a quarter
//! hour, UTC. It moves only when it closes: to a start chosen in the commander's first 7
//! days, or else towards the most active 4 h, by at most 1 h. It does not move while a
//! defence is pending (D-156). It is public (D-075); presence, activity and a chosen start
//! are private.
//!
//! Deadlines (window closes, grace ends) are absolute ticks in this module's own queue, fired
//! in phase 2 after the Universe's other timers, in `(deadline, CommanderId, kind)` order
//! (doc 11 §3, §4.1). Commanders are identified by their `CommanderId`, so the queue needs no
//! `SimId`, and the identities of everything else in the Universe are as they were before
//! S6a.

mod activity;
mod presence;
#[cfg(test)]
mod tests;
mod window;

pub(crate) use presence::{counts_as_online, set_presence, sync_presence};
pub(crate) use window::{choose_window, sample_activity, validate_window_choice};

use std::collections::{BTreeMap, BTreeSet};

use nomad_types::{CommanderKind, QuarterHour};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::id::CommanderId;
use crate::state::UniverseState;
use crate::step::TickOutputs;
use crate::time::{Tick, Ticks};

use activity::ActivityHistory;

/// The window start of AI commanders until A5 gives them schedules, and of the harness:
/// 19:00 UTC. *Proposed* (S6 plan §4.3 asks for "a harness default" without a value); the
/// human's default is local 19:00, chosen by the server's `create` (D-157).
pub const DEFAULT_WINDOW_START: QuarterHour = match QuarterHour::at(19, 0) {
    Some(start) => start,
    None => panic!("19:00 is a quarter hour"),
};

/// Every commander of the Universe and their deadlines.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Commanders {
    pub(crate) records: BTreeMap<CommanderId, Commander>,
    /// Window closes and grace ends, by deadline (doc 11 §3).
    pub(crate) deadlines: Deadlines,
}

/// What falls due at a commander deadline.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) enum Deadline {
    /// The exposure window closes; it may move now (D-155).
    WindowCloses,
    /// The disconnect grace ends; direct orders lapse (doc 12 §5.3, D-162).
    GraceEnds,
}

/// Commander deadlines, fired in `(tick, commander, kind)` order: total, and therefore
/// deterministic (doc 11 §3, D-049).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Deadlines(BTreeSet<(Tick, CommanderId, Deadline)>);

impl Deadlines {
    pub(crate) fn schedule(&mut self, tick: Tick, commander: CommanderId, kind: Deadline) {
        self.0.insert((tick, commander, kind));
    }

    pub(crate) fn cancel(&mut self, tick: Tick, commander: CommanderId, kind: Deadline) {
        self.0.remove(&(tick, commander, kind));
    }

    /// Removes and returns the earliest deadline due on or before `now`.
    pub(crate) fn pop_due(&mut self, now: Tick) -> Option<(Tick, CommanderId, Deadline)> {
        let &first = self.0.first()?;
        if first.0 > now {
            return None;
        }
        self.0.pop_first()
    }

    /// The earliest pending deadline.
    #[cfg(test)]
    pub(crate) fn next(&self) -> Option<Tick> {
        self.0.first().map(|&(tick, ..)| tick)
    }

    /// Every pending deadline, in firing order.
    #[cfg(test)]
    pub(crate) fn pending(&self) -> impl Iterator<Item = (Tick, CommanderId, Deadline)> + '_ {
        self.0.iter().copied()
    }
}

/// One commander's record (S6 plan §4.1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Commander {
    pub(crate) kind: CommanderKind,
    /// When the commander joined: the season start in the MVP (late joiners are outside it).
    pub(crate) joined: Tick,
    /// An AI's schedule says it is online (D-080). Set by `SetPresence` until A5.
    pub(crate) scheduled_online: bool,
    pub(crate) presence: Presence,
    /// While offline inside the disconnect grace: the tick it ends, a deadline.
    pub(crate) grace_until: Option<Tick>,
    pub(crate) window: Window,
    pub(crate) activity: ActivityHistory,
}

/// Whether a commander is online, and since when (S6 plan §4.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Presence {
    /// Online since this tick.
    Online {
        /// When the commander came online.
        since: Tick,
    },
    /// Offline since this tick.
    Offline {
        /// When the commander went offline, or joined.
        since: Tick,
    },
}

impl Presence {
    /// Whether this is [`Presence::Online`].
    pub const fn is_online(self) -> bool {
        matches!(self, Self::Online { .. })
    }
}

/// A commander's exposure window: its start and its current or next occurrence (D-155).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Window {
    pub(crate) start: QuarterHour,
    /// The tick the current or next occurrence opens.
    pub(crate) opens: Tick,
    /// The tick it closes, the window's end deadline.
    pub(crate) closes: Tick,
    /// A start chosen with `SetExposureWindow`, applied when the window next closes (D-157).
    pub(crate) chosen: Option<QuarterHour>,
}

/// Who a commander is at creation (S6 plan §4.1, §4.3; D-157).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommanderSetup {
    /// The commander.
    pub id: CommanderId,
    /// Human or AI.
    pub kind: CommanderKind,
    /// The initial exposure window's start, UTC.
    pub window_start: QuarterHour,
}

impl CommanderSetup {
    /// A human commander whose window starts at `window_start`, UTC: from `create
    /// --window`, by default the player's local 19:00 (D-157).
    pub const fn human(id: CommanderId, window_start: QuarterHour) -> Self {
        Self {
            id,
            kind: CommanderKind::Human,
            window_start,
        }
    }

    /// An AI commander with [`DEFAULT_WINDOW_START`], until A5 gives AI schedules (D-157).
    pub const fn ai(id: CommanderId) -> Self {
        Self {
            id,
            kind: CommanderKind::Ai,
            window_start: DEFAULT_WINDOW_START,
        }
    }
}

/// Why a commander could not be added.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum CommanderError {
    /// The commander already has a record.
    #[error("commander {0:?} already exists")]
    Duplicate(CommanderId),
    /// The window start is not a quarter hour of the day.
    #[error("the window start of commander {0:?} is not a quarter hour of the day")]
    InvalidWindow(CommanderId),
}

/// A commander as the simulation holds it: a copy for views, the Deputy and AI (S6 plan
/// §4.8), the harness and tests.
///
/// The exposure window is public (D-075, D-170 for protections later). Presence, the grace,
/// a chosen window and activity are private to the commander (D-075): S7's `CommanderView`
/// decides who sees them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommanderRecord {
    /// The commander.
    pub id: CommanderId,
    /// Human or AI.
    pub kind: CommanderKind,
    /// When the commander joined.
    pub joined: Tick,
    /// Online or offline, and since when.
    pub presence: Presence,
    /// While offline inside the disconnect grace: the tick it ends and direct orders lapse.
    pub grace_until: Option<Tick>,
    /// The exposure window.
    pub window: ExposureWindow,
    /// A start chosen with `SetExposureWindow`, applied when the window next closes.
    pub chosen_window: Option<QuarterHour>,
    /// The first tick on which `SetExposureWindow` is refused (D-157).
    pub free_choice_until: Tick,
}

/// A commander's exposure window, public (D-075): its start and the occurrence that is open
/// now or opens next, as deadlines (doc 11 §3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExposureWindow {
    /// The start, a quarter hour of the UTC day.
    pub start: QuarterHour,
    /// The tick the current or next occurrence opens.
    pub opens: Tick,
    /// The tick it closes.
    pub closes: Tick,
}

impl ExposureWindow {
    /// Whether the window is open on `tick`. It closes at the start of `closes`.
    pub fn is_open(&self, tick: Tick) -> bool {
        self.opens <= tick && tick < self.closes
    }
}

/// Adds a commander who joins now (the season start, at creation): offline, with its window
/// and an empty activity history.
pub(crate) fn add_commander(
    state: &mut UniverseState,
    setup: CommanderSetup,
) -> Result<(), CommanderError> {
    if state.commanders.records.contains_key(&setup.id) {
        return Err(CommanderError::Duplicate(setup.id));
    }
    if !setup.window_start.is_valid() {
        return Err(CommanderError::InvalidWindow(setup.id));
    }
    let now = state.tick;
    let (opens, closes) = window::occurrence_at(state, now, setup.window_start);
    state
        .commanders
        .deadlines
        .schedule(closes, setup.id, Deadline::WindowCloses);
    state.commanders.records.insert(
        setup.id,
        Commander {
            kind: setup.kind,
            joined: now,
            scheduled_online: false,
            presence: Presence::Offline { since: now },
            grace_until: None,
            window: Window {
                start: setup.window_start,
                opens,
                closes,
                chosen: None,
            },
            activity: ActivityHistory::default(),
        },
    );
    Ok(())
}

/// Fires the commander deadlines due on `now` (doc 11 §4.1 phase 2).
pub(crate) fn fire_due(state: &mut UniverseState, now: Tick, out: &mut TickOutputs) {
    while let Some((_deadline, commander, kind)) = state.commanders.deadlines.pop_due(now) {
        match kind {
            Deadline::WindowCloses => {
                let frozen = window::defence_pending(state, commander);
                window::close(state, now, commander, frozen, &mut out.announcements);
            }
            Deadline::GraceEnds => {
                presence::grace_ends(
                    state,
                    now,
                    commander,
                    &mut out.fleet_events,
                    &mut out.announcements,
                );
            }
        }
        out.timers_fired = out.timers_fired.saturating_add(1);
    }
}

impl UniverseState {
    /// Every commander, in identity order.
    pub fn commander_ids(&self) -> Vec<CommanderId> {
        self.commanders.records.keys().copied().collect()
    }

    /// A commander's record, or `None` if there is no such commander (S6 plan §4.8).
    pub fn commander(&self, id: CommanderId) -> Option<CommanderRecord> {
        let record = self.commanders.records.get(&id)?;
        Some(CommanderRecord {
            id,
            kind: record.kind,
            joined: record.joined,
            presence: record.presence,
            grace_until: record.grace_until,
            window: public_window(&record.window),
            chosen_window: record.window.chosen,
            free_choice_until: record.joined.after(free_choice(self)),
        })
    }

    /// A commander's exposure window, public (D-075): the start and the occurrence open now
    /// or next.
    pub fn exposure_window(&self, id: CommanderId) -> Option<ExposureWindow> {
        self.commanders
            .records
            .get(&id)
            .map(|record| public_window(&record.window))
    }

    /// Whether `id` counts as online for siege rules after the last completed tick: online,
    /// or offline inside the disconnect grace; nobody during catch-up (D-160, D-047).
    pub fn counts_as_online(&self, id: CommanderId) -> bool {
        counts_as_online(self, id, self.tick)
    }

    /// A commander's minutes online per quarter hour of the UTC day, summed over the activity
    /// history (D-154, D-155), index 0 being 00:00 UTC. Private to the commander (D-075).
    pub fn activity_minutes(&self, id: CommanderId) -> Option<Vec<u32>> {
        let record = self.commanders.records.get(&id)?;
        Some(record.activity.totals(
            self.clock.universe_time_ms(self.tick),
            self.rules.sieges.activity_history_days,
        ))
    }
}

fn public_window(window: &Window) -> ExposureWindow {
    ExposureWindow {
        start: window.start,
        opens: window.opens,
        closes: window.closes,
    }
}

/// How long after joining a commander may choose their window (D-157).
pub(crate) fn free_choice(state: &UniverseState) -> Ticks {
    Ticks::from_days(u64::from(state.rules.sieges.free_choice_days))
}
