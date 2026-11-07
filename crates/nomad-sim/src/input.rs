//! External inputs to a tick (Design doc 11 §6).
//!
//! Everything that enters the simulation from outside arrives here and is journaled by the
//! server with the tick it applies on. The Deputy, AI commanders and NPC raiders are not
//! inputs: they run inside the deterministic step.

use nomad_types::Command;
use serde::{Deserialize, Serialize};

use crate::id::CommanderId;
use crate::time::Ticks;

/// All external inputs that apply on one tick.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TickInputs {
    /// Operator commands, applied first, in journal order.
    pub operator: Vec<OperatorInput>,
    /// Commander inputs, applied after operator commands, ordered by
    /// `(commander, client_seq)` whatever order they arrive in.
    pub commanders: Vec<CommanderInput>,
}

/// An input from one commander's client.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CommanderInput {
    /// Who sent it.
    pub commander: CommanderId,
    /// The client's sequence number. Orders a commander's inputs within a tick.
    pub client_seq: u64,
    /// What it is.
    pub kind: CommanderInputKind,
}

/// Kinds of commander input.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum CommanderInputKind {
    /// The commander's client connected. This is a real input: it drives the disconnect
    /// grace period, siege eligibility and the activity-derived exposure window (D-032).
    Connected,
    /// The commander's client disconnected.
    Disconnected,
    /// A command (doc 15 §3.4), checked by the single validator (D-077).
    Command(Command),
}

/// Operator commands (Design doc 11 §6).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperatorInput {
    /// Starts `count` diagnostic timers, each rescheduling itself after a random delay that
    /// averages `mean_interval`. Exercises timers and random streams for pacing and
    /// determinism tests. Not a game rule.
    SpawnDiagnostics {
        /// Number of diagnostic timers to start.
        count: u32,
        /// Average delay between firings.
        mean_interval: Ticks,
    },
    /// The server starts catching up after downtime (doc 11 §7, D-047): from this tick on,
    /// the tick runs in catch-up mode until [`OperatorInput::CatchUpEnd`]. Rules read the
    /// mode through [`crate::UniverseState::catching_up`]; NPC raiders, for example, start no
    /// new raids during catch-up (D-047). Journaled like every input, so replay sees the
    /// same mode (N2 plan §4.5). Rejected if the Universe is already catching up.
    CatchUpStart,
    /// The server has caught up with the wall clock: this tick and the following ones run
    /// live again. Rejected if the Universe is not catching up.
    CatchUpEnd,
    /// An AI commander's schedule says it is online, or not (S6 plan §4.1, D-080). AI
    /// schedules arrive with A5; until then the harness drives AI presence with this input,
    /// journaled like any input so that runs replay. A production server never sends it.
    /// Rejected for a human or unknown commander, and if it changes nothing. During catch-up
    /// it takes effect when catch-up ends: nobody is online before (D-047).
    SetPresence {
        /// The AI commander.
        commander: CommanderId,
        /// Whether its schedule has it online.
        online: bool,
    },
}
