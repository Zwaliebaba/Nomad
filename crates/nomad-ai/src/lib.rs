//! Nomad's AI: the Deputy, the Strategist and the raider director (Design doc 13; D-077,
//! D-199 to D-211; A1 plan §4).
//!
//! **The boundary (D-199).** This crate depends only on `nomad-types` and `serde`.
//! `nomad-sim` depends on it, implements [`View`] over its `CommanderView` (S7 plan §4.7)
//! and calls the AI from its phases. Cargo refuses dependency cycles, so nothing here can
//! name the simulation state: the AI sees exactly what its commander may know (D-027,
//! D-083 item 8). The manifest test below keeps the crate graph that way.
//!
//! **How the AI acts (D-200, D-201).** Entry points are plain functions of `&impl View`
//! and the layer's own memory. They return [`Issue`]s, ordinary commands with the tick of
//! the event that triggered them. The simulation puts them in the owner's outbox, where the
//! competence gate holds each one until its latency has passed and the owner's command rate
//! allows it, and then applies it through the single validator, like a human's (D-077). No
//! AI layer ever changes the state itself.
//!
//! **What the AI learns (D-202).** Besides the view's own, public and sensed information,
//! each owner has an inbox of the events it may know, kept for 60 s ([`InboxEntry`]), with
//! the outcome of every AI command it issued.
//!
//! **Memory (A1 plan §4.2).** [`Memory`] is part of the Universe state, in snapshots and the
//! state hash, so a restore anywhere gives the same decisions (D-048, D-049).
//!
//! **Determinism (D-049, A1 plan §4.6):** no clock, I/O or threads; ordered collections only;
//! integer scores. The same `clippy.toml` rules as `nomad-sim` apply.
//!
//! **Layers.** The Deputy ([`deputy`], A1) runs for every commander, online or offline, at
//! 2 Hz (D-203): threat and alert, fleet roles and engagement, rebuilding and convoy safety
//! (doc 13 §4, D-206 to D-210). The Strategist ([`strategist`], A2) runs only while its
//! commander is online;
//! until A2 it decides nothing. The raider director arrives with A3.

pub mod deputy;
mod inbox;
mod memory;
pub mod strategist;
mod view;

pub use deputy::{DeferReason, DeputyEvent, Rebuild};
pub use inbox::{CommandOutcome, DropReason, FleetHappening, InboxEntry, InboxEvent};
pub use memory::{CommanderMemory, Memory};
pub use view::{
    BuildCheck, DeputyParams, FleetPlace, HoldingDefence, OwnConvoy, OwnFleet, OwnHolding,
    RouteHop, RouteQuery, RouteSpeed, RouteSummary, RouteTo, Sector, SensedFleet, SensedShip, View,
};

use nomad_types::{Command, Tick};
use serde::{Deserialize, Serialize};

/// An AI layer (doc 13 §3; D-077). The order is the outbox's: at the same due tick a
/// Deputy command drains before a Strategist command (D-201).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Layer {
    /// The Deputy: executes doctrine (doc 12 §5, doc 13 §4).
    Deputy,
    /// The Strategist: plays an AI commander while it is online (doc 13 §5).
    Strategist,
    /// The raider director: controls NPC raid groups (doc 13 §8; A3).
    Director,
}

/// A command an AI layer wants applied (D-200).
#[derive(Clone, Debug, PartialEq)]
pub struct Issue {
    /// The tick of the event that triggered it; its latency counts from here (D-201). The
    /// simulation never applies it before this tick plus the latency, nor before the tick
    /// after the one it was issued on.
    pub trigger: Tick,
    /// The command, in the same words a client would use.
    pub command: Command,
}

/// What one evaluation of a layer produced.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Output {
    /// Commands for the outbox, in the order issued.
    pub commands: Vec<Issue>,
    /// What the Deputy reports for the away report (doc 12 §5.3).
    pub events: Vec<DeputyEvent>,
}

#[cfg(test)]
mod tests {
    /// The build-time fairness check (D-083 item 8, A1 plan §4.8): this crate must never
    /// depend on the simulation, the server or the protocol, or it could name what its
    /// commander may not know.
    #[test]
    fn manifest_lists_no_forbidden_crate() {
        let manifest = include_str!("../Cargo.toml");
        for forbidden in ["nomad-sim", "nomad-server", "nomad-protocol"] {
            let named = manifest.lines().any(|line| {
                let line = line.trim_start();
                !line.starts_with('#')
                    && (line.starts_with(forbidden) || line.contains(&format!("\"{forbidden}\"")))
            });
            assert!(!named, "nomad-ai's Cargo.toml must not list {forbidden}");
        }
    }
}
