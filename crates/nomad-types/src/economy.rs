//! Economy shapes: modules, build slots, power profiles and economy commands (Design doc 14;
//! S4 plan §4; D-126 to D-138).
//!
//! Data only (D-086): what a module costs, where it may go and what a command does are rules
//! in `nomad-sim`. Decoding checks none of that; the simulation's single validator does
//! (D-077).

use serde::{Deserialize, Serialize};

use crate::id::{FleetId, HoldingId, SimId, SiteId};
use crate::ship::ShipClass;

/// The kinds of station module (doc 14 §9.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ModuleKind {
    /// A holding's core: the dome, docking, base storage and command (doc 14 §9.3). The
    /// mother station's comes with the start kit; other cores are deployed from core kits.
    Core,
    /// A shield pylon (doc 15 §5.1; D-140).
    Pylon,
    /// A solar array, the peripheral power source (doc 14 §7).
    Solar,
    /// A harvester on an ore field (doc 14 §6).
    Harvester,
    /// A refinery: ore to alloy (doc 14 §6).
    Refinery,
    /// A crystal extractor on a crystal vein (doc 14 §6).
    Extractor,
    /// A storage module (container rack): raises the pool cap and makes the holding a depot
    /// (doc 14 §4; D-132: FB-2).
    Storage,
    /// A turret (doc 15 §5.3).
    Turret,
    /// A dry dock: a parallel ship lane, frigates and docking room (D-068, D-102).
    DryDock,
}

/// A build slot of a holding (D-130, D-131). Indices count from 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Slot {
    /// The core, at the centre.
    Core,
    /// A pylon slot on the dome edge, one per shield sector.
    Pylon(u8),
    /// A standard inner slot inside the dome.
    Inner(u8),
    /// The dock slot inside the dome, for the dry dock only.
    Dock,
    /// The solar slot outside the dome.
    Solar,
    /// The slot on the site's deposit with this index, for its harvester or extractor.
    Deposit(u8),
}

/// A holding's power routing profile, the PWR ROUTE button (doc 15 §5.2; D-128).
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub enum PowerProfile {
    /// Regeneration and production share a tier; the building dry dock comes after.
    #[default]
    Balanced,
    /// Regeneration, then production, then the building dry dock.
    ShieldsFirst,
    /// Production and the building dry dock, then regeneration.
    ProductionFirst,
}

/// What a build queue item makes (D-068).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum BuildItem {
    /// A module in a slot of the holding.
    Module {
        /// The module.
        kind: ModuleKind,
        /// Its slot, reserved while the item waits.
        slot: Slot,
    },
    /// A ship.
    Ship {
        /// Its class.
        class: ShipClass,
    },
    /// A core kit, stored at the holding until a MULE loads it (doc 15 §8; D-135).
    CoreKit,
}

/// Economy commands (doc 14; S4 plan §4.7).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EconomyCommand {
    /// Adds an item to the end of a holding's build queue.
    Build {
        /// The holding.
        holding: HoldingId,
        /// What to build.
        item: BuildItem,
    },
    /// Cancels a queue item: a full refund before it starts, half after (D-068).
    Cancel {
        /// The holding.
        holding: HoldingId,
        /// The queue item.
        item: SimId,
    },
    /// Sets the holding's power profile (PWR ROUTE).
    SetPowerProfile {
        /// The holding.
        holding: HoldingId,
        /// The profile.
        profile: PowerProfile,
    },
    /// Sets the fleet new ships of this holding join (RALLY), or clears it.
    SetRally {
        /// The holding.
        holding: HoldingId,
        /// The fleet, if any.
        fleet: Option<FleetId>,
    },
    /// Turns the REPAIR ability on or off (doc 15 §5.3, D-071).
    SetRepair {
        /// The holding.
        holding: HoldingId,
        /// On or off.
        on: bool,
    },
    /// Gives a MULE fleet a convoy route: a loop of own holdings that ends at a depot
    /// (D-064, D-134). An empty route ends the convoy.
    Convoy {
        /// The fleet.
        fleet: FleetId,
        /// The stops, in order.
        route: Vec<HoldingId>,
    },
    /// Loads a core kit stored at the holding where the fleet is docked into one of its empty
    /// MULEs (D-133).
    LoadCoreKit {
        /// The fleet.
        fleet: FleetId,
    },
    /// Deploys a carried core kit on a free site near the fleet: the site becomes an outpost
    /// whose core takes 2 h (D-054, D-135).
    DeployCoreKit {
        /// The fleet carrying the kit.
        fleet: FleetId,
        /// The site.
        site: SiteId,
    },
}
