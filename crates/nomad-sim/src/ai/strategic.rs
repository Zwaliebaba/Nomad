//! `nomad-ai`'s [`StrategicView`] over S7's `CommanderView`: what the Strategist reads
//! besides the Deputy's view (A2 plan §4.2; D-077, D-199).
//!
//! Every answer is the observer's own (its economy, holdings, fleets, schedule), public
//! (sites, systems, ownership, the season clock, rules values; D-044, D-075) or sensed and
//! remembered (hostiles; doc 16 §3–§5). Nothing answers about another commander's presence,
//! kind or doctrine (D-075, D-078).

use std::collections::BTreeSet;

use nomad_ai::strategist::{
    DepositInfo, EconomyNumbers, EconomyStatus, HoldingStatus, ModuleStatus, MuleCargo, QueuedItem,
    SessionTimes, SiteHolder, SiteInfo, StrategicView, StrategistParams, SystemInfo,
};
use nomad_ai::{RouteHop, RouteSpeed, RouteSummary, View};
use nomad_types::{
    CommanderKind, FleetId, GateId, HoldingId, Location, ModuleKind, ShipClass, ShipId, Stock,
    SystemId, Tick,
};

use super::view::{AiView, millis};
use crate::fleet::route::{RouteEnd, plan_avoiding};
use crate::galaxy::is_claimable;
use crate::geometry::Vec2;
use crate::time::Ticks;

/// Every module kind a holding can build (the core is deployed, not built).
const MODULES: [ModuleKind; 8] = [
    ModuleKind::Pylon,
    ModuleKind::Solar,
    ModuleKind::Harvester,
    ModuleKind::Refinery,
    ModuleKind::Extractor,
    ModuleKind::Storage,
    ModuleKind::Turret,
    ModuleKind::DryDock,
];

const SHIPS: [ShipClass; 4] = [
    ShipClass::Vx,
    ShipClass::Lancer,
    ShipClass::Warden,
    ShipClass::Mule,
];

fn mw(value: u64) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

impl StrategicView for AiView<'_> {
    fn strategist(&self) -> StrategistParams<'_> {
        let state = self.state();
        let ai = &state.rules.ai;
        let difficulty = self
            .view()
            .observer()
            .commander()
            .and_then(|id| state.commanders.records.get(&id))
            .filter(|r| r.kind == CommanderKind::Ai)
            .map(|r| r.difficulty)
            .unwrap_or_default();
        let row = ai.difficulty(difficulty);
        StrategistParams {
            rules: &ai.strategist,
            difficulty,
            noise_permille: row.noise_permille,
            blueprints: row.blueprints,
            interval_ticks: Ticks::from_secs(row.strategist_interval_s).get().max(1),
        }
    }

    fn session(&self) -> SessionTimes {
        let Some(me) = self.view().observer().commander() else {
            return SessionTimes::default();
        };
        let (ends, next_starts, longest_ticks) =
            crate::commander::session_times(self.state(), me, self.now());
        SessionTimes {
            ends,
            next_starts,
            longest_ticks,
        }
    }

    fn final_tick(&self) -> Tick {
        self.view().final_tick()
    }

    fn economy(&self) -> Option<EconomyStatus> {
        let e = self.view().economy()?;
        Some(EconomyStatus {
            pool: e.pool,
            cap: e.cap,
            income_last_hour: e.income_last_hour,
            command_used: e.command_used,
            command_cap: e.command_cap,
            core_kit_price: e.core_kit_price,
        })
    }

    fn holding_status(&self, holding: HoldingId) -> Option<HoldingStatus> {
        let view = self.view();
        let public = view.holding(holding)?;
        if view.observer().commander() != Some(public.owner) {
            return None;
        }
        let state = self.state();
        let h = state.holding(holding)?;
        let e = view.holding_economy(holding)?;
        Some(HoldingStatus {
            id: holding,
            site: public.site,
            system: public.system,
            mother: h.mother_station,
            complete: h.core_complete,
            slots: e.slots.iter().map(|(slot, _)| *slot).collect(),
            modules: e
                .slots
                .iter()
                .filter_map(|(slot, m)| {
                    m.filter(|m| m.kind != ModuleKind::Core)
                        .map(|m| ModuleStatus {
                            slot: *slot,
                            kind: m.kind,
                            building: m.building_ms.is_some(),
                            disabled: m.disabled,
                        })
                })
                .collect(),
            queue: e
                .queue
                .iter()
                .map(|q| QueuedItem {
                    item: q.item,
                    price: q.price,
                    started: q.building.is_some(),
                })
                .collect(),
            buffer: e.buffer,
            buffer_cap: e.buffer_cap,
            delivers_to_pool: e.delivers_to_pool,
            kits: e.kits,
            docked: state.docked_ships(holding),
            docking_capacity: state.docking_capacity(holding),
            rally: state.economy.holdings.get(&holding).and_then(|x| x.rally),
        })
    }

    fn mule_cargo(&self, ship: ShipId) -> Option<MuleCargo> {
        let cargo = self.view().own_cargo(ship)?;
        Some(MuleCargo {
            stock: cargo.stock,
            core_kit: cargo.core_kit,
        })
    }

    fn sites(&self) -> Vec<SiteInfo> {
        let state = self.state();
        let galaxy = self.view().galaxy();
        galaxy
            .sites()
            .map(|(id, site)| {
                let holder = state.holding_at(id).map(|(hid, h)| SiteHolder {
                    owner: h.owner,
                    holding: hid,
                    complete: h.core_complete,
                });
                let claimable = galaxy
                    .system(site.system)
                    .is_some_and(|s| is_claimable(s.kind));
                SiteInfo {
                    id,
                    system: site.system,
                    position: site.position,
                    deposits: site
                        .deposits
                        .iter()
                        .map(|d| DepositInfo {
                            kind: d.kind,
                            richness_permille: u32::try_from(crate::economy::richness_permille(
                                d.richness,
                            ))
                            .unwrap_or(0),
                        })
                        .collect(),
                    holder,
                    free: claimable && holder.is_none() && !crate::siege::site_reserved(state, id),
                }
            })
            .collect()
    }

    fn systems(&self) -> Vec<SystemInfo> {
        let galaxy = self.view().galaxy();
        galaxy
            .systems()
            .map(|(id, system)| SystemInfo {
                id,
                kind: system.kind,
                claimable: is_claimable(system.kind),
                sites: system.sites.clone(),
                neighbours: galaxy.neighbours(id).into_iter().collect(),
            })
            .collect()
    }

    fn hostile_ships_in(&self, system: SystemId) -> u32 {
        let view = self.view();
        let me = view.observer();
        let sensed = view.sensed_fleets_in(system);
        let seen: BTreeSet<FleetId> = sensed.iter().map(|f| f.id).collect();
        let now: usize = sensed
            .iter()
            .filter(|f| f.owner != me)
            .map(|f| f.ships.len())
            .sum();
        let remembered: u32 = view
            .last_known_fleets_in(system)
            .filter(|f| f.owner != me && !seen.contains(&f.fleet))
            .map(|f| f.classes.iter().map(|c| c.count).sum::<u32>())
            .sum();
        u32::try_from(now)
            .unwrap_or(u32::MAX)
            .saturating_add(remembered)
    }

    fn route_to_point(
        &self,
        from: Location,
        to: Location,
        speed: RouteSpeed,
    ) -> Option<RouteSummary> {
        let state = self.state();
        let speed = match speed {
            RouteSpeed::Class(class) => state.rules.class(class).cruise_speed_m_per_s,
            RouteSpeed::Fleet(fleet) => {
                self.view().own_fleet(fleet)?;
                let fleet = state.entities.get::<crate::fleet::Fleet>(fleet.sim_id())?;
                crate::command::fleet_cruise_speed(state, &fleet)
            }
        };
        let route = plan_avoiding(
            &state.galaxy,
            from.system,
            Vec2::from(from.position),
            RouteEnd::Point(to.system, Vec2::from(to.position)),
            speed,
            Ticks::from_secs(state.rules.gates.spool_up_s),
            &BTreeSet::<GateId>::new(),
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

    fn numbers(&self) -> EconomyNumbers {
        let economy = &self.state().rules.economy;
        let stock = |c: crate::economy::rules::Cost| Stock::units(c.alloy, c.crystal);
        EconomyNumbers {
            harvester_ore_per_h: economy.production.harvester_ore_per_h,
            refinery_alloy_per_h: economy.production.refinery_alloy_per_h,
            extractor_crystal_per_h: economy.production.extractor_crystal_per_h,
            diminishing_yield_permille: economy.diminishing_yield_permille,
            core_supply_mw: mw(economy.core.supply_mw),
            mother_supply_mw: mw(economy.core.mother_supply_mw),
            modules: MODULES
                .iter()
                .map(|&kind| {
                    let m = economy.module(kind);
                    (kind, stock(m.cost), mw(m.draw_mw), mw(m.supply_mw))
                })
                .collect(),
            ships: SHIPS
                .iter()
                .filter_map(|&class| {
                    economy
                        .ship(class)
                        .map(|s| (class, stock(s.cost), s.command))
                })
                .collect(),
            rack: Stock::units(economy.storage.rack_alloy, economy.storage.rack_crystal),
            max_route_stops: economy.logistics.max_route_stops,
            queue_max: economy.queue.max_items,
        }
    }
}
