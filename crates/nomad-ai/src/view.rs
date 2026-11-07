//! The AI's whole world: read-only queries by information class (D-077, D-199; S7 plan
//! §4.7, A1 plan §4.1).
//!
//! `nomad-sim` implements this trait over its `CommanderView`. Every query answers only what
//! the observer may know: its own assets, public data, what its sensors detect, and its own
//! inbox. No query says whether another commander is human or AI, whether they are online,
//! or what their doctrine is (D-075, D-078; A1 plan §4.8). Calls are monomorphised, so there
//! is no dynamic dispatch, and the AI can be tested against a fake view.

use nomad_types::{Owner, Tick};

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
}
