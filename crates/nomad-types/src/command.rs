//! Commands: the shape of what a commander asks for (Design doc 15 §3.4, doc 20 §5; D-077,
//! D-087).
//!
//! **Why here and not in `nomad-protocol`:** the same command type is sent by players over
//! the wire and issued inside the simulation by the Deputy, AI commanders and the raider
//! director (doc 15 §3.4; D-013, D-016, D-077). All of them go through the simulation's
//! single validator. `nomad-sim` depends only on `nomad-types` (doc 20 §3), and the protocol
//! must not depend on `nomad-sim`, so the type lives in the crate both share.
//!
//! This module defines the **wire shape only**: which order, to which fleet or holding, with
//! which target. What an order does, when it is legal and what it costs are rules for the
//! simulation (doc 15, the S3 validator). Decoding checks none of that; for example a
//! [`FleetOrder::Patrol`] route may be empty or contain non-finite points, and the validator
//! rejects it.
//!
//! Commands are grouped by what they act on (doc 20 §5): a fleet, a holding, or fleet
//! management. Economy and doctrine commands arrive with their work packages (docs 12–14).

use serde::{Deserialize, Serialize};

use crate::id::{FleetId, HoldingId, ShipId, SimId, SystemId};
use crate::position::Location;

/// A command from a commander, in the client's or the Deputy's words (doc 20 §5).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Command {
    /// An order to one fleet (doc 15 §3.4).
    Fleet {
        /// The fleet that receives the order.
        fleet: FleetId,
        /// What it is ordered to do.
        order: FleetOrder,
        /// Whether the order replaces the fleet's orders or joins the end of its queue
        /// (S3 plan §4.4).
        queue: Queueing,
    },
    /// An order to one holding (doc 15 §5.3).
    Holding {
        /// The holding.
        holding: HoldingId,
        /// What it is ordered to do.
        order: HoldingOrder,
    },
    /// Forms a new fleet from these ships, taking them out of their fleets. With ships from
    /// one fleet this is a split; with every ship of several fleets it is a merge
    /// (doc 15 §3.4, doc 17 §6 fleet panel).
    CreateFleet {
        /// The ships of the new fleet.
        ships: Vec<ShipId>,
    },
    /// Moves every ship of `fleet` into `into`, which keeps its orders and rules of
    /// engagement. `fleet` ceases to exist.
    MergeFleets {
        /// The fleet that is absorbed.
        fleet: FleetId,
        /// The fleet that absorbs it.
        into: FleetId,
    },
}

/// How a fleet order relates to the orders the fleet already has (S3 plan §4.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Queueing {
    /// The order replaces the current order and the queue.
    Replace,
    /// The order runs after every order already queued.
    Append,
}

/// The fleet order set (doc 15 §3.4). Points name their system (S3 plan §6 task 0); moving
/// to another system is [`FleetOrder::Jump`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum FleetOrder {
    /// Move to a point, for example a site or a gate.
    Move {
        /// Where to.
        to: Location,
    },
    /// Attack one target.
    Attack {
        /// The target entity.
        target: SimId,
    },
    /// Move to a point, engaging what is met on the way.
    AttackMove {
        /// Where to.
        to: Location,
    },
    /// Patrol a route of points, in a loop.
    Patrol {
        /// The route, in order.
        route: Vec<Location>,
    },
    /// Guard an object: stay near it and follow it.
    Guard {
        /// The entity to guard.
        object: SimId,
    },
    /// Hold position (station-keeping): where the fleet is, or at its present offset from a
    /// target, following it.
    StationKeep {
        /// The entity to keep station on, if any.
        relative_to: Option<SimId>,
    },
    /// Dock at a holding.
    Dock {
        /// The holding.
        at: HoldingId,
    },
    /// Undock.
    Launch,
    /// Retreat to a holding and dock there.
    Retreat {
        /// Where to. `None` means the nearest owned holding (S3 plan §6 task 0).
        to: Option<HoldingId>,
    },
    /// Jump to another system. The simulation computes the route over lanes (doc 15 §3.4).
    Jump {
        /// The destination system.
        system: SystemId,
    },
    /// Set the fleet's rules of engagement.
    SetRulesOfEngagement {
        /// The new rules.
        rules: RulesOfEngagement,
    },
}

/// Orders to a holding (doc 15 §5.3, the phase-1 command card).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum HoldingOrder {
    /// LAUNCH (L), "6 READY": undock the ships that are ready.
    LaunchReady,
}

/// Rules of engagement, set per fleet (doc 15 §3.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum RulesOfEngagement {
    /// Weapons free.
    WeaponsFree,
    /// Return fire.
    ReturnFire,
    /// Hold fire.
    HoldFire,
}
