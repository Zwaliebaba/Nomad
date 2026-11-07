//! The command outbox and the competence gate (D-077, D-200, D-201; A1 plan §4.3).
//!
//! - **Submitting:** a command an AI layer issues on tick `now`, triggered by an event on
//!   tick `trigger`, waits until `not_before = max(trigger + latency, now + 1)`. A trigger
//!   in the future counts as `now`. So no AI command applies before its trigger plus the
//!   latency, nor on the tick it was issued (doc 11 §4.1).
//! - **Applying** (phase 1, after the tick's external inputs): owners in identity order;
//!   each owner's due commands oldest first, the Deputy's before the Strategist's at the same
//!   due tick, then in issue order (D-201). At most the owner's command rate applies in any
//!   window of 10 consecutive ticks, across all its layers. Each goes through the same
//!   `execute`, and so the same validator, as a client's command (D-077).
//! - **Expiry:** a due command still waiting after 10 s is dropped and counted.
//! - *Proposed:* a rejected command counts against the rate like an applied one: the
//!   validator ran for it. A dropped one does not.
//! - **Outcomes** go to the owner's inbox for the layer that issued them, and into the
//!   counters, but not into `TickOutputs::commands`, from which the server answers clients
//!   (A1 plan §4.3).

use std::collections::BTreeMap;

use nomad_ai::{CommandOutcome, DropReason, InboxEntry, InboxEvent, Issue, Layer};
use nomad_types::Owner;
use serde::{Deserialize, Serialize};

use super::{Queued, competence};
use crate::command::{CommandRejection, execute};
use crate::state::UniverseState;
use crate::step::TickOutputs;
use crate::time::{Tick, Ticks};

/// What one owner's layer did with the command path (A1 plan §4.8). Issued commands are
/// applied, rejected, dropped or still waiting.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayerCounters {
    /// Commands the layer issued into the outbox.
    pub issued: u64,
    /// Commands the validator accepted and the simulation carried out.
    pub applied: u64,
    /// Commands the validator refused, by reason.
    pub rejected: BTreeMap<CommandRejection, u64>,
    /// Commands dropped before the validator, by reason.
    pub dropped: BTreeMap<DropReason, u64>,
}

impl LayerCounters {
    /// Every refused command, whatever the reason.
    pub fn rejected_total(&self) -> u64 {
        self.rejected.values().sum()
    }

    /// Every dropped command, whatever the reason.
    pub fn dropped_total(&self) -> u64 {
        self.dropped.values().sum()
    }
}

/// Puts what `layer` of `owner` issued on `now` into the owner's outbox.
pub(crate) fn submit(
    state: &mut UniverseState,
    owner: Owner,
    layer: Layer,
    now: Tick,
    issue: Issue,
) {
    let latency = Ticks::new(competence(state, owner).latency_ticks);
    let trigger = issue.trigger.min(now);
    let not_before = trigger.after(latency).max(now.next());
    let runtime = &mut state.ai_runtime;
    let seq = runtime.next_seq;
    runtime.next_seq = seq.wrapping_add(1);
    runtime.counters.entry((owner, layer)).or_default().issued += 1;
    let outbox = runtime.outboxes.entry(owner).or_default();
    let queued = Queued {
        seq,
        layer,
        issued: now,
        not_before,
        command: issue.command,
    };
    let at = outbox
        .waiting
        .partition_point(|other| other.key() <= queued.key());
    outbox.waiting.insert(at, queued);
}

/// Phase 1, after the external inputs: applies every owner's due AI commands within its
/// rate, and drops those that waited too long.
pub(crate) fn apply_due(state: &mut UniverseState, now: Tick, out: &mut TickOutputs) {
    let owners: Vec<Owner> = state.ai_runtime.outboxes.keys().copied().collect();
    let window = state.rules.ai.rate_window_ticks;
    let expiry = Ticks::from_secs(state.rules.ai.command_expiry_s);
    for owner in owners {
        let rate = competence(state, owner).commands_per_window;
        let Some(mut outbox) = state.ai_runtime.outboxes.remove(&owner) else {
            continue;
        };
        // The window: this tick and the window − 1 before it.
        while outbox
            .applied
            .front()
            .is_some_and(|&t| now.get().saturating_sub(t.get()) >= window)
        {
            outbox.applied.pop_front();
        }
        let mut kept = Vec::with_capacity(outbox.waiting.len());
        for queued in std::mem::take(&mut outbox.waiting) {
            if queued.not_before > now {
                kept.push(queued);
                continue;
            }
            if now.get().saturating_sub(queued.not_before.get()) >= expiry.get() {
                record(
                    state,
                    owner,
                    now,
                    queued,
                    CommandOutcome::Dropped(DropReason::Expired),
                    None,
                );
                continue;
            }
            if outbox.applied.len() >= usize::try_from(rate).unwrap_or(usize::MAX) {
                kept.push(queued);
                continue;
            }
            let result = execute(
                state,
                owner,
                &queued.command,
                &mut out.fleet_events,
                &mut out.economy_events,
            );
            outbox.applied.push_back(now);
            match result {
                Ok(()) => record(state, owner, now, queued, CommandOutcome::Applied, None),
                Err(reason) => record(
                    state,
                    owner,
                    now,
                    queued,
                    CommandOutcome::Rejected,
                    Some(reason),
                ),
            }
        }
        outbox.waiting = kept;
        if !outbox.waiting.is_empty() || !outbox.applied.is_empty() {
            state.ai_runtime.outboxes.insert(owner, outbox);
        }
    }
}

/// Counts an outcome and tells the owner's inbox.
fn record(
    state: &mut UniverseState,
    owner: Owner,
    now: Tick,
    queued: Queued,
    outcome: CommandOutcome,
    reason: Option<CommandRejection>,
) {
    let runtime = &mut state.ai_runtime;
    let counters = runtime.counters.entry((owner, queued.layer)).or_default();
    match (outcome, reason) {
        (CommandOutcome::Applied, _) => counters.applied += 1,
        (CommandOutcome::Rejected, Some(reason)) => {
            *counters.rejected.entry(reason).or_default() += 1;
        }
        (CommandOutcome::Rejected, None) => {}
        (CommandOutcome::Dropped(why), _) => *counters.dropped.entry(why).or_default() += 1,
    }
    runtime.inboxes.entry(owner).or_default().push(InboxEntry {
        tick: now,
        event: InboxEvent::Outcome {
            layer: queued.layer,
            command: queued.command,
            outcome,
        },
    });
}

/// Puts a command straight into `owner`'s outbox, for tests of the gate.
#[cfg(test)]
pub(crate) fn submit_for_test(
    state: &mut UniverseState,
    owner: Owner,
    layer: Layer,
    now: Tick,
    trigger: Tick,
    command: nomad_types::Command,
) {
    submit(state, owner, layer, now, Issue { trigger, command });
}
