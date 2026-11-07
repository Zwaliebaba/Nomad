//! The two decisions S6 takes over (S5 plan §4.6; S6 plan §4.4, §4.7).
//!
//! S5b calls these functions and follows their answer; S6b gives their rules (the owner's
//! exposure window, the attacker, protection, the siege state; [`crate::siege`]).

use nomad_types::{ModuleKind, Owner, Slot};

use crate::economy::Module;
use crate::holding::HoldingId;
use crate::id::SimId;
use crate::state::UniverseState;
use crate::time::Tick;

/// What happens to a structure whose hull reaches 0, or that is hit again at 0 (D-169).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StructureOutcome {
    /// Knocked offline; it recovers under D-149.
    Disabled,
    /// Destroyed: the module is gone and its slot is free.
    Destroyed,
    /// The core is down: the holding falls, or an outpost under construction is lost. S6
    /// acts on [`super::DefenceEvent::CoreDown`]; S5b leaves the core at 0 hull.
    CoreDown,
}

/// The outcome for module `module` at 0 hull on tick `now`, under a hit of `by` (S5 plan §4.6;
/// S6 plan §4.7):
///
/// - a peripheral structure (the solar array, a harvester or an extractor, D-138) is
///   **destroyed** if `now` lies inside its owner's exposure window and **disabled**
///   otherwise (doc 12 §3), disabled ones hit again included (D-169); NPC raiders only ever
///   disable (D-223) — see [`crate::siege::peripheral_outcome`];
/// - an inner module is **destroyed**: it can only be hit through a sector opened for a
///   siege, where destroyed modules are lost (doc 15 §6);
/// - the core is **down**: during a siege the holding falls; an outpost under construction
///   is destroyed, at any time (D-054). S6b acts on [`super::DefenceEvent::CoreDown`].
pub(crate) fn structure_outcome_at_zero(
    state: &UniverseState,
    module: SimId,
    by: Owner,
    now: Tick,
) -> StructureOutcome {
    let Some(m) = state.entities.get::<Module>(module) else {
        return StructureOutcome::Disabled;
    };
    match (m.kind, m.slot) {
        (ModuleKind::Core, _) => StructureOutcome::CoreDown,
        (_, Slot::Solar | Slot::Deposit(_)) => {
            crate::siege::peripheral_outcome(state, m.holding, by, now)
        }
        (_, Slot::Core | Slot::Pylon(_) | Slot::Inner(_) | Slot::Dock) => {
            StructureOutcome::Destroyed
        }
    }
}

/// Whether a hostile hit that leaves a sector of `holding` at 0 on tick `now` is a breach
/// (doc 15 §5.1): the holding is breachable, Nominal and not protected (S6b, D-015, D-159).
/// During protection a sector can be worn to 0 without a breach, and the next hostile hit
/// after protection ends is one.
pub(crate) fn breach_may_occur(state: &UniverseState, holding: HoldingId, now: Tick) -> bool {
    crate::siege::breachable(state, holding, now)
}
