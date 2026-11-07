//! `nomad-ai`'s [`View`] over S7's [`CommanderView`] (D-199; S7 plan §4.7, A1 plan §4.1).
//!
//! Every answer comes from the observer's `CommanderView` or from its own entries in the
//! simulation's AI bookkeeping (its inbox). Nothing here answers about another commander's
//! presence, kind or doctrine (D-075, D-078; A1 plan §4.8).

use nomad_ai::{InboxEntry, View};
use nomad_types::{Owner, Tick};

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
}
