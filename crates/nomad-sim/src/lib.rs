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
//! - the single command validator ([`validate_command`], D-077);
//! - commander records (S6a; doc 12 §4, §5.3): human or AI, presence with the 120 s
//!   disconnect grace and the lapse of direct orders after it (D-160 to D-162), the activity
//!   history and the activity-derived exposure window (D-032, D-154 to D-157, D-171), see
//!   [`UniverseState::commander`];
//! - announcements to commanders, as data ([`Announcement`], S6 plan §4.8);
//! - the season's standings, the early win and the season end ([`season`], S8: D-191 to
//!   D-198), with the final tick in [`SeasonConfig`] (D-195);
//! - the siege lifecycle (S6b; doc 12 §3–§4, doc 15 §6–§8): breach, Reinforced with its
//!   stagger slot, the siege and its clock, Held, capture and the emergency jumps of docked
//!   ships and of a fallen mother station, the comeback grant, protections, the window rule
//!   for peripheral structures and the outpost exception (D-015, D-033, D-034, D-054, D-057,
//!   D-138, D-158 to D-170, D-223, D-229), see [`UniverseState::siege`].
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
//! movement, fleets and orders (doc 15 §3, §9; D-101 to D-105), the economy with claims
//! (doc 14; S4, D-126 to D-138, D-189, D-217) and the season standings (S8). Sensors (S7),
//! the siege lifecycle (S6b) and the AI (A1–A3) follow. Ship combat is ([`combat`], S5a: weapons, lock,
//! projectiles, the multiplier table, engagement), and so is station combat ([`defence`],
//! S5b: dome sectors, regeneration, breaches, damage to modules and its recovery, turrets,
//! SHLD SURGE and FLAK SCREEN, the damage ledger). Until sensors exist (S7) every entity
//! counts as visible. The offline rules (doc 12) are here: presence, the disconnect grace and
//! exposure windows (S6a), and the siege lifecycle (S6b).

mod announce;
pub mod combat;
mod command;
mod commander;
pub mod defence;
pub mod economy;
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
pub mod season;
mod siege;
pub mod snapshot;
mod state;
mod step;
#[cfg(test)]
mod testkit;
mod time;
mod timers;
mod util;

pub use announce::{Announcement, AnnouncementKind, HeldReason, Recipients};
pub use combat::{CombatEvent, ProjectileId};
pub use command::{CommandRejection, validate_command};
pub use commander::{
    CommanderError, CommanderRecord, CommanderSetup, DEFAULT_WINDOW_START, ExposureWindow, Presence,
};
pub use defence::{DefenceEvent, DefenceView, LedgerView, SectorView};
pub use economy::{
    BuildLane, Cargo, CommanderEconomy, EconomyEvent, HoldingEconomyView, ModuleView,
    PowerAllocation, QueueItemView, Shortage, Stock,
};
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
pub use nomad_types::{
    BuildItem, Command, CommanderKind, EconomyCommand, FleetOrder, HoldingOrder, ModuleKind, Owner,
    PowerProfile, QuarterHour, Queueing, RulesOfEngagement, Slot, StationAbility,
};
pub use nomad_types::{Location, PositionMetres};
pub use rng::{RngStreams, StreamKey, purpose};
pub use rules::{Rules, RulesError};
pub use siege::{DefenceState, SiegeView, StationJumpView, StationLeg};
pub use state::{ScheduledEvent, SeasonConfig, UniverseState, final_tick_after_weeks};

// S8: season standings and the result.
pub use nomad_types::{SeasonEndReason, SeasonResult, Standing};
pub use season::Standings;
pub use step::{CommandResult, TickOutputs, step};
pub use time::{SeasonClock, TICK_MILLIS, TICKS_PER_SECOND, Tick, Ticks, runs_on};
pub use timers::TimerQueue;
