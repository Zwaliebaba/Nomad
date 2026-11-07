//! `nomad-ai`'s [`View`] over S7's [`CommanderView`] (D-199; S7 plan §4.7, A1 plan §4.1).
//!
//! Every answer comes from the observer's `CommanderView` or from its own entries in the
//! simulation's AI bookkeeping (its inbox). Nothing here answers about another commander's
//! presence, kind or doctrine (D-075, D-078; A1 plan §4.8).

use nomad_ai::{InboxEntry, View};
use nomad_types::{
    DirectControl, DoctrinePreset, DoctrineValues, FleetDoctrine, FleetId, HoldingDoctrine,
    HoldingId, Owner, ReplacementEntry, Tick,
};

use crate::sensors::CommanderView;
use crate::state::UniverseState;

/// The AI's view of `observer` on the tick being computed.
pub(crate) struct AiView<'a> {
    view: CommanderView<'a>,
    now: Tick,
}

impl<'a> AiView<'a> {
    pub(crate) fn new(state: &'a UniverseState, observer: Owner, now: Tick) -> Self {
        Self {
            view: state.view(observer),
            now,
        }
    }

    fn state(&self) -> &'a UniverseState {
        self.view.universe()
    }
}

impl View for AiView<'_> {
    fn now(&self) -> Tick {
        self.now
    }

    fn observer(&self) -> Owner {
        self.view.observer()
    }

    fn inbox(&self) -> &[InboxEntry] {
        self.state().inbox(self.view.observer())
    }

    fn doctrine_preset(&self) -> Option<DoctrinePreset> {
        let me = self.view.observer().commander()?;
        Some(self.state().doctrine_preset(me))
    }

    fn preset_values(&self, preset: DoctrinePreset) -> DoctrineValues {
        *self.view.rules().ai.preset(preset)
    }

    fn holding_doctrine(&self, holding: HoldingId) -> Option<HoldingDoctrine> {
        let me = self.view.observer().commander()?;
        let own = self.view.holding(holding).is_some_and(|h| h.owner == me);
        if own {
            self.state().holding_doctrine(holding)
        } else {
            None
        }
    }

    fn fleet_doctrine(&self, fleet: FleetId) -> Option<FleetDoctrine> {
        self.view.own_fleet(fleet)?;
        self.state().fleet_doctrine(fleet)
    }

    fn replacement_list(&self) -> Vec<ReplacementEntry> {
        self.view
            .observer()
            .commander()
            .map(|me| self.state().doctrine(me).replacement)
            .unwrap_or_default()
    }

    fn direct_control(&self, fleet: FleetId) -> Option<DirectControl> {
        self.view.own_fleet(fleet)?.direct_control
    }
}
