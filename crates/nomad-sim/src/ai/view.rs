//! `nomad-ai`'s [`View`] over S7's [`CommanderView`] (D-199; S7 plan §4.7, A1 plan §4.1).
//!
//! Every answer comes from the observer's `CommanderView` or from its own entries in the
//! simulation's AI bookkeeping (its inbox), and for an NPC faction from its own raid records
//! (A3). Nothing here answers about another commander's presence, kind or doctrine (D-075,
//! D-078; A1 plan §4.8), nor about anyone's fleet value: only the launch rule reads that
//! (D-219).

use std::collections::BTreeSet;

use nomad_ai::{
    BuildCheck, DeputyParams, FleetPlace, HoldingDefence, InboxEntry, OwnConvoy, OwnFleet,
    OwnHolding, RaidOrder, RaidRules, RouteHop, RouteQuery, RouteSpeed, RouteSummary, RouteTo,
    Sector, SensedFleet, SensedHolding, SensedModule, SensedShip, View,
};
use nomad_types::{
    BuildItem, Command, DirectControl, DoctrinePreset, DoctrineValues, EconomyCommand,
    FleetDoctrine, FleetId, GateId, HoldingDoctrine, HoldingId, Location, ModuleKind, Owner,
    PowerProfile, ReplacementEntry, ShipClass, Stock, SystemId, Tick,
};

use crate::command::CommandRejection;
use crate::fleet::Place;
use crate::fleet::route::{RouteEnd, plan_avoiding};
use crate::geometry::Vec2;
use crate::rules::combat::{SizeClass, TargetClass};
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

    pub(super) fn state(&self) -> &'a UniverseState {
        self.view.universe()
    }

    /// The observer's `CommanderView`.
    pub(super) const fn view(&self) -> &CommanderView<'a> {
        &self.view
    }

    fn owns_holding(&self, holding: HoldingId) -> bool {
        let me = self.view.observer().commander();
        self.view
            .holding(holding)
            .is_some_and(|h| Some(h.owner) == me)
    }
}

/// Metres to whole metres, rounded; rules distances are finite and non-negative.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to 0..=u32::MAX first, so the cast is exact for whole values"
)]
fn whole_metres(metres: f64) -> u32 {
    libm::round(metres).clamp(0.0, f64::from(u32::MAX)) as u32
}

/// Seconds to whole milliseconds, rounded; routes are finite and non-negative.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to 0..=u64::MAX first, so the cast is exact for whole values"
)]
pub(super) fn millis(seconds: f64) -> u64 {
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

    fn deputy_params(&self) -> DeputyParams {
        self.view.rules().ai.deputy
    }

    fn own_fleet_ids(&self) -> Vec<FleetId> {
        let mut ids = self.view.own_fleet_ids();
        ids.sort_unstable();
        ids
    }

    fn defence(&self, holding: HoldingId) -> Option<HoldingDefence> {
        let d = self.view.defence(holding)?;
        Some(HoldingDefence {
            dome: d.dome,
            sectors: d.sectors.map(|s| Sector {
                strength_milli: s.strength_milli,
                max_milli: s.max_milli,
                open: s.open,
            }),
        })
    }

    fn power_profile(&self, holding: HoldingId) -> Option<PowerProfile> {
        self.view.holding_economy(holding).map(|e| e.profile)
    }

    fn can_dock(&self, fleet: FleetId, holding: HoldingId) -> bool {
        self.view.own_fleet(fleet).is_some()
            && self
                .state()
                .entities
                .get::<crate::fleet::Fleet>(fleet.sim_id())
                .is_some_and(|f| crate::command::dock_check(self.state(), &f, holding).is_ok())
    }

    fn accepts(&self, command: &Command) -> bool {
        crate::command::validate_command(self.state(), self.view.observer(), command).is_ok()
    }

    fn picture_tick(&self) -> Tick {
        self.view.picture_tick()
    }

    fn anything_sensed_in(&self, system: SystemId) -> bool {
        !self.view.sensed_ship_ids_in(system).is_empty()
    }

    fn sensed_fleets_in(&self, system: SystemId) -> Vec<SensedFleet> {
        self.view
            .sensed_fleets_in(system)
            .into_iter()
            .map(|f| SensedFleet {
                id: f.id,
                owner: f.owner,
                position: f.position,
                ships: f
                    .ships
                    .iter()
                    .map(|s| SensedShip {
                        id: s.id,
                        class: s.class,
                        position: s.position,
                        shrouded: s.shrouded,
                    })
                    .collect(),
            })
            .collect()
    }

    fn class_armed(&self, class: ShipClass) -> bool {
        self.view.rules().class(class).weapon_range_m.is_some()
    }

    fn class_hull(&self, class: ShipClass) -> u64 {
        u64::from(self.view.rules().combat.class(class).hull)
    }

    fn class_light(&self, class: ShipClass) -> bool {
        self.view.rules().combat.class(class).size == SizeClass::Light
    }

    fn multiplier_percent(&self, shooter: ShipClass, target: ShipClass) -> u32 {
        let combat = &self.view.rules().combat;
        combat.class(shooter).weapons.first().map_or(0, |weapon| {
            combat
                .multipliers(weapon.kind)
                .against(TargetClass::Ship(combat.class(target).size))
        })
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
        let (hull, max_hull) = view
            .ships
            .iter()
            .filter_map(|&ship| self.state().ship_hull(ship))
            .fold((0_u64, 0_u64), |(h, m), (left, max)| {
                (h + u64::from(left), m + u64::from(max))
            });
        Some(OwnFleet {
            id: fleet,
            place,
            ships,
            busy: view.order.is_some() || !view.queued.is_empty(),
            hull,
            max_hull,
            rules: view.rules_of_engagement,
            engaged: view.engaged,
            order: view.order,
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
                centre: self
                    .view
                    .galaxy()
                    .site(h.site)
                    .map_or(nomad_types::PositionMetres::ORIGIN, |s| s.position),
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

    fn on_claim_trip(&self, fleet: FleetId) -> bool {
        self.state().economy.claims.contains_key(&fleet) && self.own_fleet(fleet).is_some()
    }

    fn raid_orders(&self) -> Vec<RaidOrder> {
        let Owner::Npc(faction) = self.view.observer() else {
            return Vec::new();
        };
        let Some(f) = self.state().raids.factions.get(&faction) else {
            return Vec::new();
        };
        f.groups
            .iter()
            .map(|(&fleet, g)| RaidOrder {
                fleet,
                haven: f.haven,
                target: g.target,
                system: g.system,
                value: g.value,
                launched: g.launched,
            })
            .collect()
    }

    fn raid_rules(&self) -> Option<RaidRules> {
        let Owner::Npc(_) = self.view.observer() else {
            return None;
        };
        let rules = self.view.rules();
        let raids = &rules.ai.raids;
        Some(RaidRules {
            clock_ticks: Ticks::from_secs(raids.clock_s).get(),
            loss_limit_permille: raids.loss_limit_permille,
            withdraw_ratio_permille: raids.withdraw_ratio_permille,
            withdraw_radius_m: raids.withdraw_radius_m,
            turret_reach_m: whole_metres(rules.defence.turret.weapon.range_m),
            shroud_ticks: Ticks::from_secs(rules.gates.shroud_s).get(),
            interval_ticks: raids.director_interval_ticks,
        })
    }

    fn ship_value(&self, class: ShipClass) -> u64 {
        crate::raids::class_value(self.view.rules(), class)
    }

    fn holdings_in(&self, system: SystemId) -> Vec<nomad_ai::PublicHolding> {
        let galaxy = self.view.galaxy();
        self.view
            .holdings_in(system)
            .into_iter()
            .filter_map(|h| {
                Some(nomad_ai::PublicHolding {
                    holding: h.id,
                    owner: h.owner,
                    outpost: h.outpost,
                    position: galaxy.site(h.site)?.position,
                })
            })
            .collect()
    }

    fn sensed_holdings_in(&self, system: SystemId) -> Vec<SensedHolding> {
        let state = self.state();
        let galaxy = self.view.galaxy();
        self.view
            .sensed_holdings_in(system)
            .into_iter()
            .filter_map(|h| {
                let modules = h
                    .modules
                    .iter()
                    .filter(|m| m.kind != ModuleKind::Pylon)
                    .filter_map(|m| {
                        let (_, position, _) = state.module_position(m.id)?;
                        Some(SensedModule {
                            id: m.id,
                            kind: m.kind,
                            slot: m.slot,
                            position,
                            disabled: m.disabled,
                        })
                    })
                    .collect();
                Some(SensedHolding {
                    holding: h.holding,
                    owner: h.owner,
                    outpost: h.outpost,
                    position: galaxy.site(h.site)?.position,
                    modules,
                })
            })
            .collect()
    }
}
