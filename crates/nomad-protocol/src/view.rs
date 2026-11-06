//! The commander's view: a full snapshot, then deltas (Design doc 16 §2, §6; doc 20 §5;
//! D-075, D-076, D-087).
//!
//! The view is split by information class into separate collections keyed by [`SimId`]:
//! public data, the commander's own assets, entities sensed right now, and last-known
//! entries. An entity can be in more than one collection; each says what the commander
//! knows from that source. The server never sends an entity the commander has not detected
//! (doc 16 §6).
//!
//! Entries are deliberately minimal for now: kind, owner, location and, for last-known
//! entries, when the entity was seen. Further fields arrive with the rules that produce them
//! (docs 14–16); each one increments the protocol version.

use std::collections::{BTreeMap, BTreeSet};

use nomad_types::{
    CommanderId, ControlState, LaneKind, PositionMetres, SimId, SystemId, SystemKind, Tick,
};
use serde::{Deserialize, Serialize};

/// One collection of the view, keyed by identity.
pub type Entities = BTreeMap<SimId, EntityEntry>;

/// The commander's whole view at one tick (doc 16 §6).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ViewSnapshot {
    /// The tick the view reflects.
    pub tick: Tick,
    /// Public data: galaxy topology and holding ownership (D-044, D-075).
    pub public: Entities,
    /// The control state of every claimable system; public (D-041, D-075).
    pub control: BTreeMap<SystemId, ControlState>,
    /// The commander's own assets.
    pub own: Entities,
    /// What the commander's sensors detect now (D-039).
    pub sensed: Entities,
    /// Earlier sightings, each with the tick it was seen (D-076).
    pub last_known: Entities,
}

/// Changes to the view (doc 20 §5: creates, updates and removes).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ViewChanges {
    /// Changes to public data.
    pub public: EntityChanges,
    /// Systems whose control state changed, with the new state.
    pub control: BTreeMap<SystemId, ControlState>,
    /// Changes to the commander's own assets.
    pub own: EntityChanges,
    /// Changes to what is sensed now.
    pub sensed: EntityChanges,
    /// Changes to last-known entries.
    pub last_known: EntityChanges,
}

/// Changes to one collection of the view.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct EntityChanges {
    /// Entries created or updated, each in full.
    pub upserted: Entities,
    /// Entries that left the collection.
    pub removed: BTreeSet<SimId>,
}

/// One entity as the commander knows it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct EntityEntry {
    /// What it is.
    pub kind: EntityKind,
    /// Its owner, if it has one. Ownership of holdings is public (D-044).
    pub owner: Option<CommanderId>,
    /// Where it is, if it has a place in a system frame. Sectors, systems and lanes have
    /// none, and neither does a fleet in lane transit (doc 16 §4).
    pub location: Option<Location>,
    /// For a last-known entry, the tick it was seen; the client shows its age (D-076).
    pub last_seen: Option<Tick>,
}

/// What kind of entity an entry describes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntityKind {
    /// A sector (doc 10 §2).
    Sector,
    /// A star system (doc 10 §4.1).
    System(SystemKind),
    /// A site (doc 10 §5).
    Site,
    /// A gate (doc 10 §4.1).
    Gate,
    /// A lane between two gates (doc 10 §4.2).
    Lane(LaneKind),
    /// A holding or outpost (doc 10 §5).
    Holding,
    /// A fleet (doc 15 §3.4).
    Fleet,
}

/// A place in a system: which system, and where in its frame (doc 10 §8).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Location {
    /// The system.
    pub system: SystemId,
    /// The point in the system frame, in metres (D-099 item 2).
    pub position: PositionMetres,
}
