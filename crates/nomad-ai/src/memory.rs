//! AI memory: part of the Universe state (A1 plan §4.2; D-048, D-049).
//!
//! Decisions depend on memory, so it is in snapshots and the state hash: a snapshot and a
//! restore anywhere give identical decisions. During the AI phases the simulation moves it
//! out of the state, calls each layer with its view and its own memory, and moves it back.
//! The view stays strictly read-only.

use std::collections::BTreeMap;

use nomad_types::{CommanderId, FactionId};
use serde::{Deserialize, Serialize};

use crate::deputy::DeputyMemory;
use crate::director::DirectorMemory;

/// The memory of every AI layer, by owner.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Memory {
    commanders: BTreeMap<CommanderId, CommanderMemory>,
    /// Each NPC faction's raider director (A3).
    factions: BTreeMap<FactionId, DirectorMemory>,
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

    /// An NPC faction's director memory, created empty on first use.
    pub fn faction_mut(&mut self, id: FactionId) -> &mut DirectorMemory {
        self.factions.entry(id).or_default()
    }

    /// An NPC faction's director memory, if it has any.
    pub fn faction(&self, id: FactionId) -> Option<&DirectorMemory> {
        self.factions.get(&id)
    }

    /// Forgets an NPC faction's director memory once it is empty, so a faction with no raid
    /// out leaves no trace.
    pub fn tidy_faction(&mut self, id: FactionId) {
        if self.factions.get(&id).is_some_and(DirectorMemory::is_empty) {
            self.factions.remove(&id);
        }
    }
}

/// One commander's AI memory: its Deputy's, and from A2 its Strategist's.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CommanderMemory {
    /// The Deputy's memory.
    pub deputy: DeputyMemory,
}
