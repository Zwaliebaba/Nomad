//! Deterministic, headless Universe simulation core.
//!
//! Implements the simulation core of Design doc 11 (decisions D-047 to D-050):
//!
//! - the 10 Hz [`Tick`] and Universe time derived from it ([`SeasonClock`]);
//! - absolute deadline timers ([`TimerQueue`]) fired in `(deadline, SimId)` order;
//! - stable identities ([`SimId`]);
//! - seeded random streams per scope and purpose ([`RngStreams`]);
//! - canonical snapshots and a state hash ([`snapshot`]);
//! - the [`step`] function that advances the Universe by one tick;
//! - the season galaxy ([`galaxy`], Design doc 10): topology, site deposits (doc 14 §5),
//!   the MVP stage-1 generator and its invariant validator;
//! - holdings and the system control rule ([`ControlState`], D-041);
//! - the catch-up mode ([`UniverseState::catching_up`]), set and cleared by journaled
//!   operator inputs while the server fast-forwards through downtime (doc 11 §7, D-047);
//! - the season's [`Rules`]: unit and movement numbers from a rules file embedded in the
//!   build, copied into the Universe at creation (D-104).
//!
//! The shared primitives (`SimId`, `CommanderId`, `Tick`, `Ticks`, `SeasonClock`, the typed
//! galaxy and holding identities, `SystemKind`, `LaneKind`, the deposit types and
//! `ControlState`) live in `nomad-types`, so that the protocol and the client can use them
//! without depending on this crate (D-086, Design doc 20 §3). They are re-exported here
//! under their old paths. `nomad-types` holds data only: the rules that derive control
//! states and deposit values, and which systems can be claimed, are here.
//!
//! **Determinism contract (D-049):** the same build, the same snapshot and the same inputs
//! produce bit-identical state. This crate reads no clock, does no I/O and starts no threads
//! (D-050). Everything non-deterministic enters through [`TickInputs`].
//!
//! **Game rules arrive as their design is agreed (D-051).** So far: the Universe structure
//! (doc 10), deposit data and values (doc 14 §5, D-066) and system control (D-041). Claims,
//! economy flows, combat and the Deputy are not here yet, so the only scheduled work is a
//! diagnostic workload that exercises the timers and random streams for pacing and
//! determinism tests.

pub mod galaxy;
mod holding;
mod id;
mod input;
mod rng;
pub mod rules;
pub mod snapshot;
mod state;
mod step;
mod time;
mod timers;
mod util;

pub use galaxy::{
    Galaxy, GateId, Lane, LaneId, LaneKind, PositionKm, SectorId, Site, SiteId, System, SystemId,
    SystemKind,
};
pub use holding::{
    ControlState, Holding, HoldingError, HoldingId, control_from_counts, majority_threshold,
};
pub use id::{CommanderId, SimId, SimIdAllocator};
pub use input::{CommanderInput, CommanderInputKind, OperatorInput, TickInputs};
pub use rng::{RngStreams, StreamKey, purpose};
pub use rules::{Rules, RulesError};
pub use state::{ScheduledEvent, SeasonConfig, UniverseState};
pub use step::{TickOutputs, step};
pub use time::{SeasonClock, TICK_MILLIS, TICKS_PER_SECOND, Tick, Ticks, runs_on};
pub use timers::TimerQueue;
