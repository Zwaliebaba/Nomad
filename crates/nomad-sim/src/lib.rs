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
//!   build, copied into the Universe at creation (D-104);
//! - ships and fleets ([`fleet`], doc 15 §3; S3): `hecs` entities ordered by `SimId`, the
//!   start kit, fleet management, 2D kinematics with formations, cruise mode, obstacles and
//!   separation, the fleet orders, routing over lanes and jumps (D-042, D-055);
//! - the single command validator ([`validate_command`], D-077).
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
//! (doc 10), deposit data and values (doc 14 §5, D-066), system control (D-041), and
//! movement, fleets and orders (doc 15 §3, §9; D-101 to D-105). Claims, economy flows,
//! sensors and the Deputy are not here yet. Ship combat is ([`combat`], S5a: weapons, lock,
//! projectiles, the multiplier table, engagement); the dome, turrets and abilities follow
//! with S5b. Until sensors exist (S7) every entity counts as visible.

pub mod combat;
mod command;
mod entities;
pub mod fleet;
pub mod galaxy;
pub mod geometry;
mod holding;
mod id;
mod input;
mod rng;
pub mod rules;
pub mod scenario;
pub mod snapshot;
mod state;
mod step;
#[cfg(test)]
mod testkit;
mod time;
mod timers;
mod util;

pub use combat::{CombatEvent, ProjectileId};
pub use command::{CommandRejection, validate_command};
pub use fleet::{
    FleetEvent, FleetId, FleetView, MotionMode, Place, ShipClass, ShipId, ShipView, TravelStage,
};
pub use galaxy::{
    Galaxy, GateId, Lane, LaneId, LaneKind, SectorId, Site, SiteId, System, SystemId, SystemKind,
};
pub use geometry::Vec2;
pub use holding::{
    ControlState, Holding, HoldingError, HoldingId, control_from_counts, majority_threshold,
};
pub use id::{CommanderId, SimId, SimIdAllocator};
pub use input::{CommanderInput, CommanderInputKind, OperatorInput, TickInputs};
pub use nomad_types::{Command, FleetOrder, HoldingOrder, Owner, Queueing, RulesOfEngagement};
pub use nomad_types::{Location, PositionMetres};
pub use rng::{RngStreams, StreamKey, purpose};
pub use rules::{Rules, RulesError};
pub use state::{ScheduledEvent, SeasonConfig, UniverseState};
pub use step::{CommandResult, TickOutputs, step};
pub use time::{SeasonClock, TICK_MILLIS, TICKS_PER_SECOND, Tick, Ticks, runs_on};
pub use timers::TimerQueue;
