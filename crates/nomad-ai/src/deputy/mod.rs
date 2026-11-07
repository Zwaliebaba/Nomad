//! The Deputy: executes a commander's doctrine, online or offline (doc 12 §5, doc 13 §4;
//! D-013, D-016, D-206; A1 plan §5).
//!
//! **Schedule (D-203):** the simulation calls [`evaluate`] for every commander on every tick
//! of phase 4. Holdings and fleets are each evaluated at 2 Hz, on the tick where their
//! `SimId` modulo [`Stagger::of`] equals the slot, so a commander's work spreads over the
//! five ticks of a half second. Commander-wide work runs on the commander's own slot.
//!
//! **Authority (D-016, PR-3):** it acts only through ordinary commands, never attacks,
//! expands, claims, changes doctrine or starts a project.

use nomad_types::Tick;
use serde::{Deserialize, Serialize};

use crate::Output;
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
    /// The tick of the newest inbox entry already read.
    pub(crate) read_until: Option<Tick>,
}

/// One evaluation of a commander's Deputy.
pub fn evaluate(view: &impl View, memory: &mut DeputyMemory, stagger: Stagger, out: &mut Output) {
    let _ = (stagger, out);
    let read = memory.read_until;
    let newest = view
        .inbox()
        .iter()
        .rev()
        .map(|entry| entry.tick)
        .find(|&tick| read.is_none_or(|r| tick > r));
    if let Some(newest) = newest {
        memory.read_until = Some(newest);
    }
}
