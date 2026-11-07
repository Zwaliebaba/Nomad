//! Why the simulation refused a command, as the client may learn it (Design doc 20 §5;
//! D-077, D-123).
//!
//! The simulation's validator has its own, more detailed reasons (`CommandRejection` in
//! `nomad-sim`). The server maps them to [`InvalidReason`] before they go on the wire, and
//! the mapping is where information is withheld: "does not exist", "is not yours" and
//! "you do not see it" all become [`InvalidReason::NotFound`]. Otherwise a modified client
//! could probe identities and learn that undetected entities exist, which fog of war rules
//! out (doc 16 §6, doc 20 §9, D-123).
//!
//! The enum lives here, not in `nomad-protocol`, so that the client can show a message for
//! each reason without depending on `nomad-sim` (D-086). It is data only: when a reason
//! applies is the validator's rule.

use serde::{Deserialize, Serialize};

/// Why the simulation's validator refused a command, as sent to the client (D-123).
///
/// Adding a variant changes the wire encoding and increments the protocol version.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum InvalidReason {
    /// The fleet, ship, holding, target or system does not exist, is not the commander's,
    /// or is not visible to the commander. One reason for all of these, so a refusal reveals
    /// nothing about entities the commander has not detected (D-123).
    NotFound,
    /// A position is not finite, or lies outside the system.
    InvalidPosition,
    /// The target is not in the system the fleet will be in; another system is reached with
    /// a jump.
    WrongSystem,
    /// The point lies inside an obstacle, or no lane route leads there.
    Unreachable,
    /// A jump to the system the fleet will already be in.
    AlreadyThere,
    /// The fleet will be docked; it must launch first.
    Docked,
    /// A launch for a fleet that will not be docked.
    NotDocked,
    /// A fleet or ship in lane transit cannot be reorganised (D-042).
    InTransit,
    /// The holding cannot take every ship of the fleet (D-102).
    NoCapacity,
    /// The fleet has a frigate and the holding has no dry dock (D-102).
    NoDryDock,
    /// A retreat with no own holding that can take the fleet.
    NoHolding,
    /// The fleet has no armed ship.
    NoWeapons,
    /// The target is the commander's own.
    FriendlyTarget,
    /// The fleet would guard, follow or merge with itself.
    SelfTarget,
    /// A patrol without waypoints.
    EmptyRoute,
    /// A new fleet without ships.
    EmptyFleet,
    /// The same ship is listed twice.
    DuplicateShip,
    /// The ships or fleets are not in one place.
    NotTogether,
    /// Nothing of the commander's is docked there to launch.
    NothingToLaunch,
    /// The holding is an outpost: its core is not complete yet (D-041).
    Outpost,
    /// That item cannot be built (cores and NPC classes).
    NotBuildable,
    /// The holding has no such slot (D-130, D-131).
    NoSuchSlot,
    /// The module does not go into that slot, or the deposit there does not suit it.
    WrongSlot,
    /// The slot has a module, or one is queued for it.
    SlotTaken,
    /// The holding's build queue is full.
    QueueFull,
    /// The fleet has no MULE.
    NoMule,
    /// Every MULE of the fleet carries something.
    CargoFull,
    /// No core kit to load or deploy.
    NoKit,
    /// The site is in a system that cannot be claimed.
    NotClaimable,
    /// The site has a holding or an outpost.
    SiteTaken,
    /// The fleet is not close enough to the site.
    TooFar,
    /// A convoy route with more stops than the rules allow.
    RouteTooLong,
    /// A convoy route lists a holding twice.
    DuplicateStop,
    /// A convoy route does not end at a depot (D-134).
    NotDepot,
    /// The holding's dome has no such sector.
    NoSuchSector,
    /// The ability is cooling down.
    AbilityCooldown,
    /// SHLD SURGE on a sector that is open during a siege (doc 15 §5.3).
    SectorOpen,
    /// The holding cannot supply the ability's power draw (D-128).
    NoPower,
    /// The window start is not a quarter hour of the day (D-155).
    InvalidWindow,
    /// The free choice of the exposure window is over: the commander's first 7 days have
    /// passed, and only activity moves the window now (D-157).
    WindowLocked,
    /// A doctrine value is out of range, or the replacement list is too long or names a
    /// fleet and class twice (A1 plan §5.1).
    InvalidDoctrine,
    /// Release of a fleet that is not under direct control (RK-10).
    NotUnderDirectControl,
    /// The command may not come from this source: the Deputy may not change doctrine
    /// (PR-3). A client never sees it.
    NotPermitted,
}
