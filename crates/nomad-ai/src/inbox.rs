//! The inbox: the events an owner may know, kept for a while (D-202; A1 plan §4.4).
//!
//! The AI runs in phases 3–4, but most events arise in phases 5–8, so a layer reads the
//! events of earlier ticks here. The simulation fills each owner's inbox at the end of every
//! step from that tick's outputs, with only what the owner may know, and drops entries
//! older than the retention (60 s, the alert window of doc 13 §4). Each layer remembers what
//! it has read.
//!
//! *Proposed* (A1a): announcements other than lapsed orders, and public season events, join
//! the inbox when a layer reads them (A2, A3). Attackers are named by owner; which of their
//! fleets fired is not in the simulation's events yet (A1b).

use nomad_types::{
    Command, FleetId, GateId, HoldingId, ModuleKind, Owner, ShipClass, ShipId, SimId, Slot,
    SystemId, Tick,
};
use serde::{Deserialize, Serialize};

use crate::Layer;

/// One inbox entry: what happened, and on which tick.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InboxEntry {
    /// The tick it happened on.
    pub tick: Tick,
    /// What happened.
    pub event: InboxEvent,
}

/// What an owner learns (D-202).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum InboxEvent {
    /// Own ship damage, summed per ship and attacking owner over one tick.
    ShipDamaged {
        /// The ship.
        ship: ShipId,
        /// Its fleet.
        fleet: FleetId,
        /// Whose weapons hit it.
        by: Owner,
        /// Hull points lost.
        hull: u64,
    },
    /// Damage to an own holding's shield sector, summed per sector and attacking owner over
    /// one tick.
    SectorDamaged {
        /// The holding.
        holding: HoldingId,
        /// The sector.
        sector: u8,
        /// Whose weapons hit it.
        by: Owner,
        /// Strength lost, milli-points.
        amount_milli: u64,
    },
    /// Damage to an own module, summed per module and attacking owner over one tick.
    ModuleDamaged {
        /// The holding.
        holding: HoldingId,
        /// The module.
        module: SimId,
        /// Whose weapons hit it.
        by: Owner,
        /// Hull points lost.
        hull: u64,
    },
    /// An own ship was destroyed.
    ShipLost {
        /// The ship.
        ship: ShipId,
        /// Its class.
        class: ShipClass,
        /// The fleet it was in.
        fleet: FleetId,
        /// Whose hit destroyed it.
        by: Owner,
    },
    /// An own fleet lost its last ship.
    FleetLost {
        /// The fleet.
        fleet: FleetId,
    },
    /// Something happened to an own fleet's orders or place.
    Fleet {
        /// The fleet.
        fleet: FleetId,
        /// What.
        what: FleetHappening,
    },
    /// A ship was built at an own holding.
    ShipBuilt {
        /// Where.
        holding: HoldingId,
        /// The ship.
        ship: ShipId,
        /// Its class.
        class: ShipClass,
        /// The fleet it joined.
        fleet: FleetId,
    },
    /// An item of an own holding's build queue finished.
    ItemCompleted {
        /// The holding.
        holding: HoldingId,
        /// The queue item.
        item: SimId,
    },
    /// An own module was knocked offline.
    ModuleDisabled {
        /// The holding.
        holding: HoldingId,
        /// The module.
        module: SimId,
    },
    /// An own disabled module came back online.
    ModuleRestored {
        /// The holding.
        holding: HoldingId,
        /// The module.
        module: SimId,
    },
    /// An own module was destroyed and its slot freed.
    ModuleDestroyed {
        /// The holding.
        holding: HoldingId,
        /// The module.
        module: SimId,
        /// What it was.
        kind: ModuleKind,
        /// The slot it was in.
        slot: Slot,
    },
    /// An own convoy holds at its last stop for a contested gate (D-055).
    ConvoyHeld {
        /// The fleet.
        fleet: FleetId,
        /// The gate.
        gate: GateId,
    },
    /// An own fleet's convoy route ended.
    ConvoyEnded {
        /// The fleet.
        fleet: FleetId,
    },
    /// The owner's direct fleet orders lapsed after the disconnect grace (D-162).
    OrdersLapsed {
        /// The fleets whose orders lapsed.
        fleets: Vec<FleetId>,
    },
    /// What became of a command an AI layer of this owner issued (A1 plan §4.3).
    Outcome {
        /// The layer that issued it.
        layer: Layer,
        /// The command.
        command: Command,
        /// What became of it.
        outcome: CommandOutcome,
    },
}

/// Something that happened to a fleet's orders or place (from the simulation's fleet
/// events).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FleetHappening {
    /// It launched from a holding.
    Launched {
        /// The holding.
        holding: HoldingId,
    },
    /// It docked at a holding.
    Docked {
        /// The holding.
        holding: HoldingId,
    },
    /// Its active order finished.
    OrderCompleted,
    /// Its attack target went out of detection (D-180).
    TargetLost,
    /// Its active order could no longer be carried out.
    OrderDropped,
    /// It arrived in a system through a gate.
    Arrived {
        /// The system.
        system: SystemId,
    },
}

/// What became of an AI command (A1 plan §4.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommandOutcome {
    /// The validator accepted it and it was carried out.
    Applied,
    /// The validator refused it. *Proposed:* the reason is withheld, because the
    /// validator's reasons tell apart what a client's refusals may not (D-123); the
    /// simulation's per-layer counters keep it.
    Rejected,
    /// It never reached the validator.
    Dropped(DropReason),
}

/// Why an AI command was dropped without being applied (D-200, D-201).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum DropReason {
    /// It waited for the command rate longer than the expiry (10 s).
    Expired,
    /// A Deputy command for a fleet under direct control (RK-10, D-200).
    DirectControl,
}
