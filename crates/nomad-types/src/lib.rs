//! Shared primitives of Nomad (Design doc 20 §3, D-086).
//!
//! The types that the simulation (`nomad-sim`), the wire protocol (`nomad-protocol`), the
//! server and the client all need, so that the client never depends on the simulation:
//!
//! - identities: [`SimId`], [`CommanderId`] and the typed identities that wrap a `SimId`
//!   ([`SectorId`], [`SystemId`], [`SiteId`], [`GateId`], [`LaneId`], [`HoldingId`]);
//! - simulation time: [`Tick`], [`Ticks`], [`TICK_MILLIS`], [`TICKS_PER_SECOND`] and
//!   Universe time ([`SeasonClock`], doc 11 §2);
//! - small public enums: [`SystemKind`], [`LaneKind`], deposits ([`Deposit`],
//!   [`DepositKind`], [`Richness`], [`DepositValue`]) and [`ControlState`].
//!
//! The types moved here from `nomad-sim` unchanged; `nomad-sim` re-exports them under their
//! old paths. Their encodings are the same, so snapshots did not change (D-049). The `SimId`
//! allocator stays in `nomad-sim`: only the simulation hands out identities.
//!
//! This crate depends only on `serde`. It holds no simulation state and no I/O.

mod control;
mod deposit;
mod galaxy;
mod id;
mod time;

pub use control::{ControlState, majority_threshold};
pub use deposit::{Deposit, DepositKind, DepositValue, Richness};
pub use galaxy::{LaneKind, SystemKind};
pub use id::{CommanderId, GateId, HoldingId, LaneId, SectorId, SimId, SiteId, SystemId};
pub use time::{SeasonClock, TICK_MILLIS, TICKS_PER_SECOND, Tick, Ticks};
