//! The Deputy: executes a commander's doctrine, online or offline (doc 12 §5, doc 13 §4;
//! D-013, D-016, D-206; A1 plan §5).
//!
//! **Schedule (D-203):** the simulation calls [`evaluate`] for every commander on every tick
//! of phase 4. Each holding and fleet is evaluated at 2 Hz, on the tick where its `SimId`
//! modulo [`Stagger::of`] equals the slot, so a commander's work spreads over the five ticks
//! of a half second. The income ledger takes one sample in each hour of the season, at the
//! first evaluation in it. Every evaluation first reads the inbox entries of the ticks
//! completed since the last one (D-202). A quiet evaluation changes no memory.
//!
//! **Authority (D-016, PR-3):** it acts only through ordinary commands, and never attacks,
//! expands, claims, changes doctrine or starts a project. So far (A1a) it rebuilds losses
//! (`rebuild`, D-208) and reroutes or holds convoys at contested gates (`convoy`, D-210).
//! Threat, alert, abilities, engagement, roles and convoy evacuation arrive with A1b.

mod convoy;
mod rebuild;

use nomad_types::{FleetId, GateId, HoldingId, ModuleKind, ShipClass, Slot, Tick};
use serde::{Deserialize, Serialize};

use crate::Output;
use crate::inbox::InboxEntry;
use crate::view::View;

/// Which units are due this tick: those whose `SimId` modulo `of` is `slot` (D-203).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stagger {
    /// This tick's slot, `0..of`.
    pub slot: u64,
    /// The number of slots: the ticks between two evaluations of one unit.
    pub of: u64,
}

impl Stagger {
    /// Whether a unit with this raw `SimId` or `CommanderId` is due.
    pub const fn due(self, raw: u64) -> bool {
        self.of != 0 && raw % self.of == self.slot
    }
}

/// The Deputy's memory (A1 plan §4.2).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DeputyMemory {
    /// Inbox entries up to this tick have been read.
    pub(crate) read_until: Option<Tick>,
    /// Replacements, the income ledger and replacement spending (D-208).
    pub(crate) rebuild: rebuild::RebuildMemory,
    /// Convoys holding at a contested gate or on a detour around one (D-210).
    pub(crate) convoys: convoy::ConvoyMemory,
}

/// What the Deputy reports for the away report (doc 12 §5.3; A1 plan §5.7, §5.8).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeputyEvent {
    /// A replacement could not be queued; the Deputy retries when the budget changes
    /// (doc 12 §5.3: "decisions the Deputy deferred").
    Deferred {
        /// What it would have rebuilt.
        what: Rebuild,
        /// Why not.
        reason: DeferReason,
    },
    /// A convoy holds docked at its last stop: no route avoids the contested gate (D-210).
    ConvoyHeld {
        /// The convoy's fleet.
        fleet: FleetId,
        /// The contested gate.
        gate: GateId,
    },
    /// A convoy takes a detour around a contested gate (D-210).
    ConvoyRerouted {
        /// The convoy's fleet.
        fleet: FleetId,
        /// The contested gate.
        gate: GateId,
    },
    /// A held or rerouted convoy runs its route again.
    ConvoyResumed {
        /// The convoy's fleet.
        fleet: FleetId,
    },
}

/// What the Deputy rebuilds (D-208).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Rebuild {
    /// A ship for a replacement-list entry.
    Ship {
        /// The entry's fleet.
        fleet: FleetId,
        /// The class.
        class: ShipClass,
    },
    /// A destroyed module, in the same slot.
    Module {
        /// The holding.
        holding: HoldingId,
        /// The module.
        kind: ModuleKind,
        /// Its slot.
        slot: Slot,
    },
}

/// Why a replacement was deferred (A1 plan §5.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum DeferReason {
    /// The rolling 24 h budget does not cover it.
    BudgetExhausted,
    /// No own holding can build it.
    NoBuilder,
    /// Every holding that could build it has a full queue.
    QueueFull,
}

/// One evaluation of a commander's Deputy.
pub fn evaluate(view: &impl View, memory: &mut DeputyMemory, stagger: Stagger, out: &mut Output) {
    if view.observer().commander().is_none() {
        return;
    }
    let now = view.now();
    // Every entry of a completed tick is in; this tick's come later (D-202).
    let read = memory.read_until;
    // The inbox is in tick order: the fresh entries are one slice of it.
    let inbox = view.inbox();
    let from = inbox.partition_point(|e| read.is_some_and(|r| e.tick <= r));
    let to = inbox.partition_point(|e| e.tick < now).max(from);
    let fresh: Vec<&InboxEntry> = inbox[from..to].iter().collect();
    // Memory changes only when something happened, so a quiet tick leaves it as it was.
    if let Some(newest) = fresh.last() {
        memory.read_until = Some(newest.tick);
    }
    rebuild::read(view, &mut memory.rebuild, &fresh);
    rebuild::sample(view, &mut memory.rebuild);
    rebuild::act(view, &mut memory.rebuild, stagger, out);
    let mut failed = Vec::new();
    convoy::read(&mut memory.convoys, &fresh, &mut failed);
    convoy::act(view, &mut memory.convoys, &failed, stagger, out);
}

#[cfg(test)]
pub(crate) mod fake;
#[cfg(test)]
mod tests;
