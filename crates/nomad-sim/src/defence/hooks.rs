//! The two decisions S6 takes over (S5 plan §4.6; S6 plan §4.4, §4.7).
//!
//! S5b calls these functions and follows their answer; S6 replaces their bodies with its
//! rules (the owner's exposure window, protection, the siege state) and keeps their
//! signatures. Until then they give the S5 plan's defaults.

use nomad_types::{ModuleKind, Slot};

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

/// The outcome for module `module` at 0 hull on tick `now` (S5 plan §4.6).
///
/// **Default until S6** (S5 plan §4.6, doc 12 §3, doc 15 §6):
///
/// - a peripheral structure (the solar array, a harvester or an extractor, D-138) is
///   **disabled**: the outside-the-window rule, which is the safe default;
/// - an inner module is **destroyed**: it can only be hit through a sector that S6 opened
///   for a siege, where destroyed modules are lost (doc 15 §6);
/// - the core is **down**.
///
/// S6 replaces this with the window rule: a peripheral structure inside its owner's window
/// is destroyed (doc 12 §3, D-169), and an outpost under construction is destroyed at any
/// time (D-054).
pub(crate) fn structure_outcome_at_zero(
    state: &UniverseState,
    module: SimId,
    _now: Tick,
) -> StructureOutcome {
    let Some(m) = state.entities.get::<Module>(module) else {
        return StructureOutcome::Disabled;
    };
    match (m.kind, m.slot) {
        (ModuleKind::Core, _) => StructureOutcome::CoreDown,
        (_, Slot::Solar | Slot::Deposit(_)) => StructureOutcome::Disabled,
        (_, Slot::Core | Slot::Pylon(_) | Slot::Inner(_) | Slot::Dock) => {
            StructureOutcome::Destroyed
        }
    }
}

/// Whether a hostile hit that leaves a sector of `holding` at 0 on tick `now` is a breach
/// (doc 15 §5.1). **Default until S6:** always. S6 replaces it with "the holding is
/// breachable": Nominal and not protected (D-159: during protection a sector can be worn to
/// 0 without a breach, and the next hostile hit after protection ends is one).
pub(crate) fn breach_may_occur(_state: &UniverseState, _holding: HoldingId, _now: Tick) -> bool {
    true
}
