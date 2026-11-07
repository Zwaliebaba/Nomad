//! The AI inside the step: phases 3–4, the outbox and the competence gate, the inbox and
//! the per-layer counters (Design doc 11 §4.1; D-077, D-199 to D-203; A1 plan §4).
//!
//! All AI logic lives in `nomad-ai`, which cannot name the state (D-199). This module is
//! the simulation's side of the boundary:
//!
//! - **Phases 3–4** ([`run`]): the memory moves out of the state, each layer is called with
//!   a [`View`](nomad_ai::View) over its owner's `CommanderView` and its own memory, and the
//!   memory moves back. The Strategist runs for AI commanders who are online, every 30, 15 or
//!   10 s by difficulty, staggered by commander (D-079, D-203, D-229); the Deputy runs for
//!   every commander on every tick, and evaluates each holding and fleet at 2 Hz staggered by
//!   `SimId` (D-203). During catch-up nobody is online, so only Deputies run (D-080).
//! - **The outbox and the gate** ([`gate`]): what a layer issues waits in its owner's outbox
//!   until its trigger tick plus the owner's latency (D-201), and is applied in phase 1 of a
//!   later tick, after the tick's external inputs, through the same `execute` and validator
//!   as a client's command (D-077, D-200), at most the owner's command rate in any 10
//!   consecutive ticks.
//! - **The inbox** ([`inbox`]): at the end of each step, what each owner may know of that
//!   tick's outputs, kept for 60 s (D-202).
//! - **Counters**: per owner and layer, commands issued, applied, rejected by reason and
//!   dropped (A1 plan §4.8), for the fairness check and the harness.
//!
//! AI decisions are not journaled: replaying the external inputs reproduces them (doc 13
//! §2, doc 11 §6).

#[cfg(test)]
mod deputy_tests;
pub(crate) mod gate;
pub(crate) mod inbox;
#[cfg(test)]
mod tests;
mod view;

use std::collections::{BTreeMap, VecDeque};

use nomad_ai::{InboxEntry, Layer};
use nomad_types::{Command, CommanderKind, Owner};
use serde::{Deserialize, Serialize};

pub use gate::LayerCounters;

use crate::id::CommanderId;
use crate::rules::ai::Competence;
use crate::state::UniverseState;
use crate::time::{Tick, Ticks};

/// The simulation's AI bookkeeping, part of the state (A1 plan §4.3, §4.4).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Runtime {
    /// Commands waiting to apply, per owner.
    pub(crate) outboxes: BTreeMap<Owner, Outbox>,
    /// What each owner may know, oldest first (D-202).
    pub(crate) inboxes: BTreeMap<Owner, Vec<InboxEntry>>,
    /// Per owner and layer (A1 plan §4.8).
    pub(crate) counters: BTreeMap<(Owner, Layer), LayerCounters>,
    /// The next outbox sequence number, so equal keys still drain in issue order.
    pub(crate) next_seq: u64,
}

/// One owner's outbox (D-200, D-201).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Outbox {
    /// Waiting commands, in drain order: `(not_before, layer, seq)`.
    pub(crate) waiting: Vec<Queued>,
    /// The ticks on which the owner's AI commands applied, within the last rate window.
    pub(crate) applied: VecDeque<Tick>,
}

/// An AI command in an outbox.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Queued {
    pub(crate) seq: u64,
    pub(crate) layer: Layer,
    /// The tick it was issued on.
    pub(crate) issued: Tick,
    /// The first tick it may apply on: its trigger plus the latency, and after `issued`.
    pub(crate) not_before: Tick,
    pub(crate) command: Command,
}

impl Queued {
    fn key(&self) -> (Tick, Layer, u64) {
        (self.not_before, self.layer, self.seq)
    }
}

/// The competence cap of `owner`'s AI layers (doc 12 §5.4; D-079, D-201): a human's Deputy
/// has its own; an AI commander's follows its difficulty. *Proposed:* an NPC faction drains
/// at Normal's values (A3 §4.6).
pub(crate) fn competence(state: &UniverseState, owner: Owner) -> Competence {
    let ai = &state.rules.ai;
    let record = owner
        .commander()
        .and_then(|id| state.commanders.records.get(&id));
    match record {
        Some(record) if record.kind == CommanderKind::Human => ai.human_deputy,
        Some(record) => ai.difficulty(record.difficulty).competence,
        None => ai.difficulty(nomad_types::Difficulty::Normal).competence,
    }
}

/// Phases 3 and 4: the Strategists, then the Deputies (doc 11 §4.1; D-203).
pub(crate) fn run(
    state: &mut UniverseState,
    now: Tick,
    events: &mut Vec<(CommanderId, nomad_ai::DeputyEvent)>,
) {
    let mut memory = std::mem::take(&mut state.ai);
    let mut issued: Vec<(Owner, Layer, nomad_ai::Issue)> = Vec::new();
    let commanders = state.commander_ids();
    {
        let state: &UniverseState = state;
        // Phase 3: the Strategist of every AI commander who is online, on its interval.
        for &id in &commanders {
            if !strategist_due(state, id, now) {
                continue;
            }
            let view = view::AiView::new(state, Owner::Commander(id), now);
            let mut out = nomad_ai::Output::default();
            nomad_ai::strategist::evaluate(&view, &mut out);
            issued.extend(
                out.commands
                    .into_iter()
                    .map(|issue| (Owner::Commander(id), Layer::Strategist, issue)),
            );
        }
        // Phase 4: every commander's Deputy, online or offline (D-013, D-080).
        let interval = state.rules.ai.deputy_interval_ticks;
        let stagger = nomad_ai::deputy::Stagger {
            slot: now.get() % interval,
            of: interval,
        };
        for &id in &commanders {
            let view = view::AiView::new(state, Owner::Commander(id), now);
            let mut out = nomad_ai::Output::default();
            nomad_ai::deputy::evaluate(
                &view,
                &mut memory.commander_mut(id).deputy,
                stagger,
                &mut out,
            );
            issued.extend(
                out.commands
                    .into_iter()
                    .map(|issue| (Owner::Commander(id), Layer::Deputy, issue)),
            );
            events.extend(out.events.into_iter().map(|event| (id, event)));
        }
    }
    state.ai = memory;
    for (owner, layer, issue) in issued {
        gate::submit(state, owner, layer, now, issue);
    }
}

/// Whether `id`'s Strategist evaluates on `now`: an AI commander who is online, on the tick
/// of its interval given by its identity (D-079, D-203). Nobody is online during catch-up
/// (D-080).
fn strategist_due(state: &UniverseState, id: CommanderId, now: Tick) -> bool {
    let Some(record) = state.commanders.records.get(&id) else {
        return false;
    };
    if record.kind != CommanderKind::Ai || state.catching_up || !record.presence.is_online() {
        return false;
    }
    let interval = Ticks::from_secs(
        state
            .rules
            .ai
            .difficulty(record.difficulty)
            .strategist_interval_s,
    )
    .get()
    .max(1);
    now.get() % interval == u64::from(id.0) % interval
}

impl UniverseState {
    /// The AI commands each owner's layers issued, applied, had rejected (by reason) and
    /// had dropped, per owner and layer (A1 plan §4.8).
    pub fn ai_counters(&self) -> &BTreeMap<(Owner, Layer), LayerCounters> {
        &self.ai_runtime.counters
    }

    /// AI commands waiting in `owner`'s outbox.
    pub fn ai_waiting(&self, owner: Owner) -> usize {
        self.ai_runtime
            .outboxes
            .get(&owner)
            .map_or(0, |outbox| outbox.waiting.len())
    }

    /// `owner`'s inbox, oldest first (D-202), as its AI layers read it.
    pub fn inbox(&self, owner: Owner) -> &[InboxEntry] {
        self.ai_runtime
            .inboxes
            .get(&owner)
            .map_or(&[], Vec::as_slice)
    }

    /// The AI memory (A1 plan §4.2), for the harness and tests.
    pub fn ai_memory(&self) -> &nomad_ai::Memory {
        &self.ai
    }
}
