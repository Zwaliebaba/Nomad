//! Season standings, the early win and the season end (Design doc 10 §6; D-020, D-041,
//! D-116, D-191 to D-198; S8 plan).
//!
//! - **The standing** of every commander (D-041): claimable systems controlled, holdings with
//!   a completed core and the deposit value at their sites (D-066), and *since*, the tick
//!   from which those three have been unchanged (D-193). Outposts count for nothing, and
//!   raider havens are not claimable. Commanders are ranked by the four keys in that order,
//!   more first and *since* earlier first; equal on all four, they share a rank (D-194).
//! - **When** (D-191): every code path that changes completed holdings or their owners marks
//!   the standings dirty ([`mark_dirty`]): an outpost's core completing (S4), and S6b's
//!   capture, emergency jump and landing. Phase 9 of a dirty tick (doc 11 §4.1) recounts the
//!   control map and the standings, compares them with the stored ones and announces what
//!   changed. The control map is cached here; [`UniverseState::control_state`] reads it.
//!   [`UniverseState::standings_now`] counts the current holdings at any point in a tick, for
//!   S6b's comeback grant after the fall (D-167), which comes before phase 9.
//! - **The early win** (D-192): on the first tick on which a commander controls at least
//!   ⌈60% × the claimable systems⌉ (6 of 9 at stage 1), the result is fixed and that tick is
//!   the final tick (D-116). The share is more than half, so at most one commander can reach
//!   it ([`crate::rules::season`]).
//! - **The scheduled end** (D-195): the step that computes [`SeasonConfig::final_tick`] fixes
//!   the result from the standings, unless an early win came first; the commanders ranked
//!   first win, jointly on a full tie (D-194). After the result [`crate::step`] changes
//!   nothing (S8 plan §4.4), and the server stops stepping (D-116).
//! - **Announcements**, all public (D-075), in this order within a tick: `ControlChanged` per
//!   system, `StandingsChanged` at most once, `NearEarlyWin` when a commander is one system
//!   short of the early win and `NearEarlyWinEnded` when they drop back (D-197), and
//!   `SeasonEnded`. No tie-break here favours the lower [`CommanderId`] (D-229).
//!
//! [`SeasonConfig::final_tick`]: crate::SeasonConfig::final_tick

mod standings;
#[cfg(test)]
mod tests;

pub use standings::Standings;

use std::collections::{BTreeMap, BTreeSet};

use nomad_types::{ControlState, SeasonEndReason, SeasonResult};
use serde::{Deserialize, Serialize};

use crate::announce::{Announcement, AnnouncementKind, Recipients};
use crate::galaxy::{SystemId, is_claimable};
use crate::holding;
use crate::id::CommanderId;
use crate::state::UniverseState;
use crate::time::Tick;

/// The season part of the Universe state (S8 plan §4.2).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SeasonRecord {
    /// The control state of every claimable system at the last recount (D-191).
    pub(crate) control: BTreeMap<SystemId, ControlState>,
    /// The standings at the last recount.
    pub(crate) standings: Standings,
    /// Commanders one system short of the early win, as announced (D-197).
    pub(crate) near_early_win: BTreeSet<CommanderId>,
    /// The result, once fixed (D-116).
    pub(crate) result: Option<SeasonResult>,
    /// Completed holdings or their owners changed since the last recount (D-191).
    pub(crate) dirty: bool,
}

/// Marks the standings for a recount in this tick's phase 9 (D-191). Every code path that
/// changes completed holdings or their owners calls it: today an outpost's core completing
/// (S4); S6b's capture, emergency jump and landing must too.
pub(crate) fn mark_dirty(state: &mut UniverseState) {
    state.season_record.dirty = true;
}

/// Fills the record at creation, without announcements: every commander has a standing
/// from the creation tick (S8 plan §4.1).
pub(crate) fn initialize(state: &mut UniverseState) {
    let (control, standings) = recount(state, state.tick);
    let near = near_early_win(&standings, state.early_win_threshold());
    let record = &mut state.season_record;
    record.control = control;
    record.standings = standings;
    record.near_early_win = near;
    record.dirty = false;
}

/// Phase 9 (doc 11 §4.1): recounts on a dirty tick, then fixes the result on an early win
/// (D-192) or at the scheduled final tick (D-195).
pub(crate) fn run(state: &mut UniverseState, now: Tick, announcements: &mut Vec<Announcement>) {
    if state.season_record.result.is_some() {
        return;
    }
    if state.season_record.dirty {
        settle(state, now, announcements);
    }
    if state.season_record.result.is_none() && now >= state.season.final_tick {
        let winners = state
            .season_record
            .standings
            .leaders()
            .map(|s| s.commander)
            .collect();
        fix(
            state,
            SeasonResult {
                tick: now,
                reason: SeasonEndReason::FinalTick,
                winners,
            },
            announcements,
        );
    }
}

/// Whose standings are counted: every commander with a record, every owner of a holding
/// and everyone who already has a standing, so nobody drops out.
fn commanders(state: &UniverseState) -> BTreeSet<CommanderId> {
    let mut commanders: BTreeSet<CommanderId> = state.commanders.records.keys().copied().collect();
    commanders.extend(state.holdings.values().map(|h| h.owner));
    commanders.extend(state.season_record.standings.iter().map(|s| s.commander));
    commanders
}

/// The control map and the standings on tick `now`, counted from the current holdings.
fn recount(state: &UniverseState, now: Tick) -> (BTreeMap<SystemId, ControlState>, Standings) {
    let (control, tallies) = standings::tally(&state.galaxy, &state.holdings, &commanders(state));
    let ranked = standings::rank(&tallies, &state.season_record.standings, now);
    (control, ranked)
}

/// The commanders one system short of the early win (D-197). None while the threshold is a
/// single system: one short of it is no system at all.
fn near_early_win(standings: &Standings, threshold: u32) -> BTreeSet<CommanderId> {
    standings
        .iter()
        .filter(|s| threshold >= 2 && s.systems.saturating_add(1) == threshold)
        .map(|s| s.commander)
        .collect()
}

/// The recount of a dirty tick, with its announcements and the early win.
fn settle(state: &mut UniverseState, now: Tick, announcements: &mut Vec<Announcement>) {
    let (control, standings) = recount(state, now);
    let threshold = state.early_win_threshold();
    let mut public = |kind| {
        announcements.push(Announcement {
            tick: now,
            to: Recipients::Public,
            kind,
        });
    };
    let record = &mut state.season_record;
    for (&system, &to) in &control {
        let from = record
            .control
            .get(&system)
            .copied()
            .unwrap_or(ControlState::Open);
        if from != to {
            public(AnnouncementKind::ControlChanged { system, from, to });
        }
    }
    if standings != record.standings {
        public(AnnouncementKind::StandingsChanged {
            standings: standings.ranked().to_vec(),
        });
    }
    record.control = control;
    record.dirty = false;
    let early: BTreeSet<CommanderId> = standings
        .iter()
        .filter(|s| s.systems >= threshold)
        .map(|s| s.commander)
        .collect();
    let near = near_early_win(&standings, threshold);
    record.standings = standings;
    if !early.is_empty() {
        // The decisive tick announces the result, not who was one system short.
        record.near_early_win.clear();
        fix(
            state,
            SeasonResult {
                tick: now,
                reason: SeasonEndReason::EarlyWin,
                winners: early.into_iter().collect(),
            },
            announcements,
        );
        return;
    }
    for &commander in near.difference(&record.near_early_win) {
        let systems = record.standings.of(commander).map_or(0, |s| s.systems);
        public(AnnouncementKind::NearEarlyWin {
            commander,
            systems,
            threshold,
        });
    }
    for &commander in record.near_early_win.difference(&near) {
        public(AnnouncementKind::NearEarlyWinEnded { commander });
    }
    record.near_early_win = near;
}

/// Fixes the result and announces it (D-116).
fn fix(state: &mut UniverseState, result: SeasonResult, announcements: &mut Vec<Announcement>) {
    announcements.push(Announcement {
        tick: result.tick,
        to: Recipients::Public,
        kind: AnnouncementKind::SeasonEnded {
            result: result.clone(),
            standings: state.season_record.standings.ranked().to_vec(),
        },
    });
    state.season_record.result = Some(result);
}

impl UniverseState {
    /// The standings after the last completed tick (D-191), best first. Public (D-075).
    pub fn standings(&self) -> &Standings {
        &self.season_record.standings
    }

    /// The standings counted from the current holdings, at any point in a tick: what phase 9
    /// would store if it ran now (S8 plan §4.2). S6b's comeback grant reads the standings
    /// leader from it after the fall, before phase 9 (D-167).
    pub fn standings_now(&self) -> Standings {
        recount(self, self.tick).1
    }

    /// The season's result, once fixed: at an early win or the final tick (D-116, D-192).
    pub fn season_result(&self) -> Option<&SeasonResult> {
        self.season_record.result.as_ref()
    }

    /// Whether the season is over: its result is fixed, and the Universe no longer advances
    /// (D-116).
    pub fn season_over(&self) -> bool {
        self.season_record.result.is_some()
    }

    /// The season's final tick: the tick its result was fixed on, or else the scheduled one
    /// (D-195). Public, as the season clock (doc 17 §6).
    pub fn final_tick(&self) -> Tick {
        self.season_record
            .result
            .as_ref()
            .map_or(self.season.final_tick, |r| r.tick)
    }

    /// How many claimable systems the galaxy has (doc 10 §6): raider havens are not.
    pub fn claimable_systems(&self) -> usize {
        self.galaxy
            .systems()
            .filter(|(_, system)| is_claimable(system.kind))
            .count()
    }

    /// The systems a commander must control to win early: ⌈60% × claimable systems⌉ (D-020,
    /// D-192), 6 of 9 at stage 1.
    pub fn early_win_threshold(&self) -> u32 {
        self.rules
            .season
            .early_win_threshold(self.claimable_systems())
    }

    /// The commanders one system short of the early win, as announced (D-197).
    pub fn near_early_win(&self) -> &BTreeSet<CommanderId> {
        &self.season_record.near_early_win
    }

    /// Where the cached control map or the standings differ from a recount of the holdings:
    /// empty unless a code path changed completed holdings or owners without
    /// [`mark_dirty`] (S8 plan §9 item 4). Checked by tests and the harness.
    pub fn season_problems(&self) -> Vec<String> {
        let record = &self.season_record;
        if record.dirty {
            return Vec::new();
        }
        let mut problems = Vec::new();
        let (control, standings) = recount(self, self.tick);
        for (system, state) in &control {
            let cached = record
                .control
                .get(system)
                .copied()
                .unwrap_or(ControlState::Open);
            if cached != *state {
                problems.push(format!(
                    "system {system:?}: cached {cached:?}, recount {state:?}"
                ));
            }
        }
        let triple = |s: &nomad_types::Standing| (s.systems, s.holdings, s.deposit_value);
        for standing in standings.iter() {
            match record.standings.of(standing.commander) {
                Some(cached) if triple(cached) == triple(standing) => {}
                cached => problems.push(format!(
                    "commander {:?}: cached {cached:?}, recount {standing:?}",
                    standing.commander
                )),
            }
        }
        problems
    }

    /// The control state of `system` from the cache, recounted while the standings are dirty.
    pub(crate) fn cached_control_state(&self, system: SystemId) -> Option<ControlState> {
        if self.season_record.dirty {
            return holding::control_state(&self.galaxy, &self.holdings, system);
        }
        self.galaxy
            .system(system)
            .filter(|s| is_claimable(s.kind))?;
        Some(
            self.season_record
                .control
                .get(&system)
                .copied()
                .unwrap_or(ControlState::Open),
        )
    }

    /// Every claimable system's control state from the cache, recounted while dirty.
    pub(crate) fn cached_control_states(&self) -> BTreeMap<SystemId, ControlState> {
        if self.season_record.dirty {
            return holding::control_states(&self.galaxy, &self.holdings);
        }
        self.galaxy
            .systems()
            .filter(|(_, system)| is_claimable(system.kind))
            .map(|(id, _)| {
                let state = self
                    .season_record
                    .control
                    .get(&id)
                    .copied()
                    .unwrap_or(ControlState::Open);
                (id, state)
            })
            .collect()
    }
}
