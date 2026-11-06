//! Deterministic, headless Universe simulation core.
//!
//! Implements the simulation core of Design doc 11 (decisions D-047 to D-050):
//!
//! - the 10 Hz [`Tick`] and Universe time derived from it ([`SeasonClock`]);
//! - absolute deadline timers ([`TimerQueue`]) fired in `(deadline, SimId)` order;
//! - stable identities ([`SimId`]);
//! - seeded random streams per scope and purpose ([`RngStreams`]);
//! - canonical snapshots and a state hash ([`snapshot`]);
//! - the [`step`] function that advances the Universe by one tick.
//!
//! **Determinism contract (D-049):** the same build, the same snapshot and the same inputs
//! produce bit-identical state. This crate reads no clock, does no I/O and starts no threads
//! (D-050). Everything non-deterministic enters through [`TickInputs`].
//!
//! **No game rules yet (D-051).** Economy, combat and the Deputy arrive with Design docs 14
//! and 15. Until then the only scheduled work is a diagnostic workload that exercises the
//! timers and random streams for pacing and determinism tests.

mod id;
mod input;
mod rng;
pub mod snapshot;
mod state;
mod step;
mod time;
mod timers;
mod util;

pub use id::{CommanderId, SimId, SimIdAllocator};
pub use input::{CommanderInput, CommanderInputKind, OperatorInput, TickInputs};
pub use rng::{RngStreams, StreamKey, purpose};
pub use state::{ScheduledEvent, SeasonConfig, UniverseState};
pub use step::{TickOutputs, step};
pub use time::{SeasonClock, TICK_MILLIS, TICKS_PER_SECOND, Tick, Ticks, runs_on};
pub use timers::TimerQueue;
