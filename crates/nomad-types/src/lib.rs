//! Shared primitives of Nomad (Design doc 20 §3, D-086).
//!
//! The types that the simulation (`nomad-sim`), the wire protocol (`nomad-protocol`), the
//! server and the client all need, so that the client never depends on the simulation:
//!
//! - identities: [`SimId`], [`CommanderId`], [`FactionId`], [`UniverseId`], the typed
//!   identities that wrap a `SimId` ([`SectorId`], [`SystemId`], [`SiteId`], [`GateId`],
//!   [`LaneId`], [`HoldingId`], [`FleetId`], [`ShipId`]) and who owns something
//!   ([`Owner`], D-014);
//! - simulation time: [`Tick`], [`Ticks`], [`TICK_MILLIS`], [`TICKS_PER_SECOND`] and
//!   Universe time ([`SeasonClock`], doc 11 §2);
//! - units and places: [`PositionMetres`] and [`Location`] (D-099);
//! - small public enums: [`SystemKind`], [`LaneKind`], [`ShipClass`], deposits ([`Deposit`],
//!   [`DepositKind`], [`Richness`], [`DepositValue`]) and [`ControlState`];
//! - commanders: [`CommanderKind`], an AI's [`Difficulty`] (D-079) and the [`QuarterHour`]
//!   an exposure window starts at (doc 12 §4, D-155);
//! - amounts of alloy and crystal ([`Stock`], milli-units, D-049);
//! - doctrine ([`DoctrinePreset`], [`HoldingDoctrine`], [`FleetDoctrine`], [`FleetRole`],
//!   [`ReplacementEntry`], the preset values [`DoctrineValues`], [`DoctrineCommand`] and
//!   [`DirectControl`];
//!   doc 12 §5.2, D-082), which the wire carries and the AI reads (D-199);
//! - the shape of [`Command`]s (doc 15 §3.4), shared by the protocol and the simulation's
//!   validator, including economy commands and module shapes ([`EconomyCommand`],
//!   [`ModuleKind`], [`Slot`], [`BuildItem`], [`PowerProfile`]; doc 14) and station abilities
//!   ([`StationAbility`]; doc 15 §5.3), and the reasons a
//!   refused command reports to the client ([`InvalidReason`], D-123);
//! - blueprints ([`Blueprint`], [`BlueprintStep`]) and the [`SiteKind`] that chooses the
//!   AI's built-in one (D-067, D-215), which U3 reuses for player blueprints;
//! - the season's standings and result ([`Standing`], [`SeasonResult`],
//!   [`SeasonEndReason`]; D-041, D-191 to D-198), public (D-075).
//!
//! **Data only (D-086):** this crate holds shapes, not rules. How a control state or a
//! deposit value is derived, which systems can be claimed and what a command may do are
//! game rules and live in `nomad-sim`, so the client links none of them.
//!
//! The `SimId` allocator stays in `nomad-sim`: only the simulation hands out identities.
//! This crate depends only on `serde`. It holds no simulation state and no I/O.

mod blueprint;
mod command;
mod commander;
mod control;
mod deposit;
mod doctrine;
mod economy;
mod galaxy;
mod id;
mod position;
mod rejection;
mod season;
mod ship;
mod stock;
mod time;

pub use blueprint::{Blueprint, BlueprintStep, SiteKind};
pub use command::{Command, FleetOrder, HoldingOrder, Queueing, RulesOfEngagement, StationAbility};
pub use commander::{CommanderKind, Difficulty, QuarterHour};
pub use control::ControlState;
pub use deposit::{Deposit, DepositKind, DepositValue, Richness};
pub use doctrine::{
    DirectControl, DoctrineCommand, DoctrinePreset, DoctrineValues, FleetDoctrine, FleetRole,
    HoldingDoctrine, ReplacementEntry,
};
pub use economy::{BuildItem, EconomyCommand, ModuleKind, PowerProfile, Slot};
pub use galaxy::{LaneKind, SystemKind};
pub use id::{
    CommanderId, FactionId, FleetId, GateId, HoldingId, LaneId, Owner, SectorId, ShipId, SimId,
    SiteId, SystemId, UniverseId,
};
pub use position::{Location, PositionMetres};
pub use rejection::InvalidReason;
pub use season::{SeasonEndReason, SeasonResult, Standing};
pub use ship::ShipClass;
pub use stock::{MILLI, Stock};
pub use time::{SeasonClock, TICK_MILLIS, TICKS_PER_SECOND, Tick, Ticks};
