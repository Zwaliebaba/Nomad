//! Commands: the shape of what a commander asks for (Design doc 15 §3.4, doc 20 §5; D-077,
//! D-087).
//!
//! **Why here and not in `nomad-protocol`:** the same command type is sent by players over
//! the wire and issued inside the simulation by the Deputy, AI commanders and the raider
//! director (doc 15 §3.4; D-013, D-016, D-077). All of them go through the simulation's
//! single validator. `nomad-sim` depends only on `nomad-types` (doc 20 §3), and the protocol
//! must not depend on `nomad-sim`, so the type lives in the crate both share.
//!
//! This module defines the **wire shape only**: which order, to which fleet, with which
//! target. What an order does, when it is legal and what it costs are rules for the
//! simulation (doc 15, and its validator once designed). Decoding checks none of that; for
//! example a [`FleetOrder::Patrol`] route may be empty or contain non-finite points, and the
//! validator rejects it.
//!
//! *The order set is Proposed in doc 15 §3.4.* Economy and doctrine commands arrive with
//! their work packages (docs 12–14).

use serde::{Deserialize, Serialize};

use crate::id::{FleetId, SimId, SystemId};
use crate::position::PositionMetres;

/// A command from a commander, in the client's or the Deputy's words (doc 20 §5).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Command {
    /// An order to one fleet (doc 15 §3.4).
    Fleet {
        /// The fleet that receives the order.
        fleet: FleetId,
        /// What it is ordered to do.
        order: FleetOrder,
    },
}

/// The fleet order set (doc 15 §3.4). Positions are in the frame of the system the fleet is
/// in (doc 10 §8, D-099); moving to another system is [`FleetOrder::Jump`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum FleetOrder {
    /// Move to a point.
    Move {
        /// Where to.
        to: PositionMetres,
    },
    /// Attack one target.
    Attack {
        /// The target entity.
        target: SimId,
    },
    /// Move to a point, engaging what is met on the way.
    AttackMove {
        /// Where to.
        to: PositionMetres,
    },
    /// Patrol a route of points.
    Patrol {
        /// The route, in order.
        route: Vec<PositionMetres>,
    },
    /// Guard an object.
    Guard {
        /// The entity to guard.
        object: SimId,
    },
    /// Hold position (station-keeping).
    StationKeep,
    /// Dock.
    Dock {
        /// The entity to dock at, for example a holding.
        at: SimId,
    },
    /// Undock.
    Launch,
    /// Retreat.
    Retreat,
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
