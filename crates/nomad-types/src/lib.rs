//! Shared primitives of Nomad (Design doc 20 §3, D-086).
//!
//! The types that the simulation (`nomad-sim`), the wire protocol (`nomad-protocol`), the
//! server and the client all need, so that the client never depends on the simulation:
//!
//! - identities: [`SimId`], [`CommanderId`], [`UniverseId`] and the typed identities that
//!   wrap a `SimId` ([`SectorId`], [`SystemId`], [`SiteId`], [`GateId`], [`LaneId`],
//!   [`HoldingId`], [`FleetId`]);
//! - simulation time: [`Tick`], [`Ticks`], [`TICK_MILLIS`], [`TICKS_PER_SECOND`] and
//!   Universe time ([`SeasonClock`], doc 11 §2);
//! - units: [`PositionMetres`] (D-099);
//! - small public enums: [`SystemKind`], [`LaneKind`], deposits ([`Deposit`],
//!   [`DepositKind`], [`Richness`], [`DepositValue`]) and [`ControlState`];
//! - the shape of [`Command`]s (doc 15 §3.4), shared by the protocol and the simulation's
//!   validator.
//!
//! The identities, clocks and enums that `nomad-sim` already had moved here unchanged;
//! `nomad-sim` re-exports them under their old paths. Their encodings are the same, so
//! snapshots did not change (D-049). The `SimId` allocator stays in `nomad-sim`: only the
//! simulation hands out identities. [`UniverseId`], [`FleetId`], [`PositionMetres`] and the
//! command types are new, for the protocol (doc 20 §5); the simulation does not use them
//! yet.
//!
//! This crate depends only on `serde`. It holds no simulation state and no I/O.

mod command;
mod control;
mod deposit;
mod galaxy;
mod id;
mod position;
mod time;

pub use command::{Command, FleetOrder, RulesOfEngagement};
pub use control::{ControlState, majority_threshold};
pub use deposit::{Deposit, DepositKind, DepositValue, Richness};
pub use galaxy::{LaneKind, SystemKind};
pub use id::{
    CommanderId, FleetId, GateId, HoldingId, LaneId, SectorId, SimId, SiteId, SystemId, UniverseId,
};
pub use position::PositionMetres;
pub use time::{SeasonClock, TICK_MILLIS, TICKS_PER_SECOND, Tick, Ticks};
