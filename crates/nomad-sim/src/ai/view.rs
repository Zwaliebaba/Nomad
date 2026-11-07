//! `nomad-ai`'s [`View`] over S7's [`CommanderView`] (D-199; S7 plan §4.7, A1 plan §4.1).
//!
//! Every answer comes from the observer's `CommanderView` or from its own entries in the
//! simulation's AI bookkeeping (its inbox). Nothing here answers about another commander's
//! presence, kind or doctrine (D-075, D-078; A1 plan §4.8).

use std::collections::BTreeSet;

use nomad_ai::{
    BuildCheck, FleetPlace, InboxEntry, OwnConvoy, OwnFleet, OwnHolding, RouteHop, RouteQuery,
    RouteSpeed, RouteSummary, RouteTo, View,
};
use nomad_types::{
    BuildItem, Command, DirectControl, DoctrinePreset, DoctrineValues, EconomyCommand,
    FleetDoctrine, FleetId, GateId, HoldingDoctrine, HoldingId, Location, Owner, ReplacementEntry,
    Stock, Tick,
};

use crate::command::CommandRejection;
use crate::fleet::Place;
use crate::fleet::route::{RouteEnd, plan_avoiding};
use crate::geometry::Vec2;
use crate::sensors::CommanderView;
use crate::state::UniverseState;
use crate::time::Ticks;

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

    fn owns_holding(&self, holding: HoldingId) -> bool {
        let me = self.view.observer().commander();
        self.view
            .holding(holding)
            .is_some_and(|h| Some(h.owner) == me)
    }
}

/// Seconds to whole milliseconds, rounded; routes are finite and non-negative.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to 0..=u64::MAX first, so the cast is exact for whole values"
)]
fn millis(seconds: f64) -> u64 {
    libm::round(seconds * 1_000.0).clamp(0.0, u64::MAX as f64) as u64
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

    fn own_fleet(&self, fleet: FleetId) -> Option<OwnFleet> {
        let view = self.view.own_fleet(fleet)?;
        let place = match view.place {
            Place::InSystem(system) => FleetPlace::InSpace(Location {
                system,
                position: view.position,
            }),
            Place::Docked(holding) => FleetPlace::Docked(holding),
            Place::InTransit { .. } | Place::EmergencyJump { .. } => FleetPlace::Travelling,
        };
        let ships = view
            .ships
            .iter()
            .filter_map(|&ship| self.view.own_ship(ship).map(|s| (ship, s.class)))
            .collect();
        Some(OwnFleet {
            id: fleet,
            place,
            ships,
            busy: view.order.is_some() || !view.queued.is_empty(),
        })
    }

    fn own_holdings(&self) -> Vec<OwnHolding> {
        self.view
            .own_holdings()
            .into_iter()
            .map(|h| OwnHolding {
                id: h.id,
                system: h.system,
                complete: !h.outpost,
            })
            .collect()
    }

    fn own_convoys(&self) -> Vec<OwnConvoy> {
        let me = self.view.observer();
        let state = self.state();
        state
            .economy
            .convoys
            .iter()
            .filter(|(fleet, _)| {
                state
                    .entities
                    .with::<crate::fleet::Fleet, _>(fleet.sim_id(), |f| f.owner == me)
                    == Some(true)
            })
            .map(|(&fleet, convoy)| OwnConvoy {
                fleet,
                route: convoy.route.clone(),
                next: convoy.next,
                held_at: convoy.held,
            })
            .collect()
    }

    fn income_last_hour(&self) -> Option<Stock> {
        self.view.economy().map(|e| e.income_last_hour)
    }

    fn rally(&self, holding: HoldingId) -> Option<FleetId> {
        if !self.owns_holding(holding) {
            return None;
        }
        self.state().economy.holdings.get(&holding)?.rally
    }

    fn build_cost(&self, holding: HoldingId, item: BuildItem) -> Option<Stock> {
        if !self.owns_holding(holding) {
            return None;
        }
        crate::economy::item_price(self.state(), holding, item)
    }

    fn build_check(&self, holding: HoldingId, item: BuildItem) -> BuildCheck {
        let command = Command::Economy(EconomyCommand::Build { holding, item });
        match crate::command::validate_command(self.state(), self.view.observer(), &command) {
            Ok(()) => BuildCheck::Ok,
            Err(CommandRejection::QueueFull) => BuildCheck::QueueFull,
            Err(CommandRejection::SlotTaken) => BuildCheck::SlotTaken,
            Err(_) => BuildCheck::CannotBuild,
        }
    }

    fn gate_location(&self, gate: GateId) -> Option<Location> {
        let gate = self.view.galaxy().gate(gate)?;
        Some(Location {
            system: gate.system,
            position: gate.position,
        })
    }

    fn holding_location(&self, holding: HoldingId) -> Option<Location> {
        let system = self.view.holding(holding)?.system;
        let position = crate::fleet::station::launch_point(self.state(), holding)?;
        Some(Location { system, position })
    }

    fn route(&self, query: &RouteQuery) -> Option<RouteSummary> {
        let state = self.state();
        let end = match query.to {
            RouteTo::System(system) => RouteEnd::System(system),
            RouteTo::Holding(holding) => {
                let at = self.holding_location(holding)?;
                RouteEnd::Point(at.system, Vec2::from(at.position))
            }
        };
        let avoid: BTreeSet<GateId> = query.avoid.iter().copied().collect();
        let speed = match query.speed {
            RouteSpeed::Class(class) => state.rules.class(class).cruise_speed_m_per_s,
            RouteSpeed::Fleet(fleet) => {
                self.view.own_fleet(fleet)?;
                let fleet = state.entities.get::<crate::fleet::Fleet>(fleet.sim_id())?;
                crate::command::fleet_cruise_speed(state, &fleet)
            }
        };
        let route = plan_avoiding(
            &state.galaxy,
            query.from.system,
            Vec2::from(query.from.position),
            end,
            speed,
            Ticks::from_secs(state.rules.gates.spool_up_s),
            &avoid,
        )?;
        let hops = route
            .hops
            .iter()
            .filter_map(|hop| {
                Some(RouteHop {
                    from_gate: hop.from_gate,
                    to_gate: hop.to_gate,
                    to_system: state.galaxy.gate(hop.to_gate)?.system,
                })
            })
            .collect();
        Some(RouteSummary {
            duration_ms: millis(route.duration_s),
            hops,
        })
    }
}
