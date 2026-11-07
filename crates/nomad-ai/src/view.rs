//! The AI's whole world: read-only queries by information class (D-077, D-199; S7 plan
//! §4.7, A1 plan §4.1).
//!
//! `nomad-sim` implements this trait over its `CommanderView`. Every query answers only what
//! the observer may know: its own assets, public data, what its sensors detect, and its own
//! inbox. No query says whether another commander is human or AI, whether they are online,
//! or what their doctrine is (D-075, D-078; A1 plan §4.8). Calls are monomorphised, so there
//! is no dynamic dispatch, and the AI can be tested against a fake view.

use nomad_types::{
    DirectControl, DoctrinePreset, DoctrineValues, FleetDoctrine, FleetId, HoldingDoctrine,
    HoldingId, Owner, ReplacementEntry, Tick,
};

use crate::inbox::InboxEntry;

/// What one observer may know of the Universe, as the AI reads it.
pub trait View {
    /// The tick being computed. What the AI issues now applies on a later tick (doc 11
    /// §4.1, D-200).
    fn now(&self) -> Tick;

    /// Whose view this is.
    fn observer(&self) -> Owner;

    /// The observer's inbox, oldest first (D-202).
    fn inbox(&self) -> &[InboxEntry];

    // --- Doctrine: the observer's own (doc 12 §5.2; D-075), and the public presets.

    /// The observer's doctrine preset; `None` for an NPC faction.
    fn doctrine_preset(&self) -> Option<DoctrinePreset>;

    /// What a preset sets: rules data, the same for everyone (D-082, D-211).
    fn preset_values(&self, preset: DoctrinePreset) -> DoctrineValues;

    /// The doctrine that applies to an own holding: its override or the preset's.
    fn holding_doctrine(&self, holding: HoldingId) -> Option<HoldingDoctrine>;

    /// The doctrine that applies to an own fleet: its override or the preset's.
    fn fleet_doctrine(&self, fleet: FleetId) -> Option<FleetDoctrine>;

    /// The observer's replacement list, in order (D-208).
    fn replacement_list(&self) -> Vec<ReplacementEntry>;

    /// Whether an own fleet is under direct control (RK-10).
    fn direct_control(&self, fleet: FleetId) -> Option<DirectControl>;
}
