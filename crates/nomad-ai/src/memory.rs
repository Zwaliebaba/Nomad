//! AI memory: part of the Universe state (A1 plan §4.2; D-048, D-049).
//!
//! Decisions depend on memory, so it is in snapshots and the state hash: a snapshot and a
//! restore anywhere give identical decisions. During the AI phases the simulation moves it
//! out of the state, calls each layer with its view and its own memory, and moves it back.
//! The view stays strictly read-only.

use std::collections::BTreeMap;

use nomad_types::CommanderId;
use serde::{Deserialize, Serialize};

use crate::deputy::DeputyMemory;

/// The memory of every AI layer, by owner.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Memory {
    commanders: BTreeMap<CommanderId, CommanderMemory>,
}

impl Memory {
    /// A commander's memory, created empty on first use.
    pub fn commander_mut(&mut self, id: CommanderId) -> &mut CommanderMemory {
        self.commanders.entry(id).or_default()
    }

    /// A commander's memory, if it has any.
    pub fn commander(&self, id: CommanderId) -> Option<&CommanderMemory> {
        self.commanders.get(&id)
    }
}

/// One commander's AI memory: its Deputy's, and from A2 its Strategist's.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CommanderMemory {
    /// The Deputy's memory.
    pub deputy: DeputyMemory,
}
