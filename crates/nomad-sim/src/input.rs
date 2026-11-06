//! External inputs to a tick (Design doc 11 §6).
//!
//! Everything that enters the simulation from outside arrives here and is journaled by the
//! server with the tick it applies on. The Deputy, AI commanders and NPC raiders are not
//! inputs: they run inside the deterministic step.

use serde::{Deserialize, Serialize};

use crate::id::CommanderId;
use crate::time::Ticks;

/// All external inputs that apply on one tick.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TickInputs {
    /// Operator commands, applied first, in journal order.
    pub operator: Vec<OperatorInput>,
    /// Commander inputs, applied after operator commands, ordered by
    /// `(commander, client_seq)` whatever order they arrive in.
    pub commanders: Vec<CommanderInput>,
}

/// An input from one commander's client.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommanderInput {
    /// Who sent it.
    pub commander: CommanderId,
    /// The client's sequence number. Orders a commander's inputs within a tick.
    pub client_seq: u64,
    /// What it is.
    pub kind: CommanderInputKind,
}

/// Kinds of commander input. Game commands arrive with Design docs 14 and 15 (D-051).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommanderInputKind {
    /// The commander's client connected. This is a real input: it drives the disconnect
    /// grace period, siege eligibility and the activity-derived exposure window (D-032).
    Connected,
    /// The commander's client disconnected.
    Disconnected,
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
}
