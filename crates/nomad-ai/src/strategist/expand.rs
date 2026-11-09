//! *Expand*: claim a free site (doc 13 §5; D-054, D-213; A2 plan §4.4).
//!
//! **Score** of a candidate site (D-213), on the score scale ([`super::rules`]):
//! - **income:** its productive build-out's value per hour to the commander (after D-217 and
//!   D-281, [`super::model`]) per cost of the kit at today's price (D-189), the productive
//!   modules and its logistics (a storage module where the system has no depot), if it pays
//!   back within the time left in the season;
//! - **control** (D-041, D-020): a bonus towards a reachable strict majority in the system,
//!   a larger one for completing it, and a denial bonus where a rival is one site short;
//! - **risk:** the route time from the kit's holding, hostiles detected or remembered in the
//!   site's system, rival holdings there, and closeness to a raider haven;
//! - **crystal first:** while crystal income is zero, a crystal vein outranks everything (S4
//!   plan §9 item 2).
//!
//! **Plan** (A2 plan §4.4): queue the kit at the own holding nearest the site; make sure a
//! lone MULE is docked there (split one off a fleet, bring an idle one, or build one); load
//! it; `Launch`, `Jump` and `Move` to the site; deploy; return the MULE to the nearest
//! holding and release it. A risky trip takes an escort, which guards the outpost until it
//! completes (D-054) and then docks there. A kit priced above the pool cap is not queued:
//! *Build out* adds storage first (D-189's *Derived* note, D-282).
//!
//! **Sessions** (D-214, D-291): a loaded kit sets out while the commander is online, and the
//! trip continues as doctrine if the session ends first: the simulation keeps the fleet's
//! orders past the grace and deploys the kit on arrival (D-291). Only a fleet without a kit
//! (an empty MULE coming to the kit's holding) still waits for a session its hop fits, since
//! its orders lapse (D-162).
//!
//! *Proposed* readings:
//! - the candidates are the free sites nearest the commander's systems by jumps, then by
//!   identity, at most 60 (A2 plan §4.10);
//! - the kit's holding is the own complete holding nearest the site by jumps, then by
//!   identity;
//! - a lane transit cannot stall (D-042), so a MULE in transit is progress;
//! - "spare garrison ships" for an escort are any armed fleet docked at the kit's holding
//!   that no plan uses; it goes along only when the trip ends inside the session, since its
//!   own orders lapse (D-162) and only the claim fleet's continue (D-291).

use std::collections::BTreeSet;

use nomad_types::{
    BuildItem, Command, DepositKind, EconomyCommand, FleetId, FleetOrder, HoldingId, Location,
    ModuleKind, Queueing, ShipClass, SiteId, Slot, Stock, SystemId, Ticks,
};
use serde::{Deserialize, Serialize};

use super::model::{self, Features};
use super::view::{SiteInfo, StrategicView, StrategistParams};
use super::{Plan, StrategistStats, Turn};
use crate::view::{FleetPlace, RouteSpeed};

/// A running expansion.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Expansion {
    pub(crate) site: SiteId,
    pub(crate) system: SystemId,
    /// Where the kit is queued and loaded.
    pub(crate) holding: HoldingId,
    pub(crate) carrier: Option<FleetId>,
    pub(crate) escort: Option<FleetId>,
    pub(crate) stage: Stage,
}

/// Where an expansion stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Stage {
    /// Queueing the kit, finding a MULE, loading.
    Prepare,
    /// Travelling to the site.
    Travel,
    /// The deploy command is out.
    Deploying,
    /// Deployed: the MULE returns, the escort guards the outpost.
    Return,
}

impl Expansion {
    pub(crate) fn fleets(&self) -> Vec<FleetId> {
        self.carrier.into_iter().chain(self.escort).collect()
    }

    /// Whether the carrier holds the kit.
    pub(crate) fn carrying(&self, view: &impl StrategicView) -> bool {
        self.carrier
            .and_then(|c| view.own_fleet(c))
            .is_some_and(|fleet| carries_kit(view, &fleet))
    }

    /// Whether a refused command ends the plan: the deploy (the site was taken first).
    pub(crate) fn failed(&self, rejected: &[Command]) -> bool {
        rejected.iter().any(|c| {
            matches!(c, Command::Economy(EconomyCommand::DeployCoreKit { fleet, site })
                if Some(*fleet) == self.carrier && *site == self.site)
        })
    }
}

fn carries_kit(view: &impl StrategicView, fleet: &crate::view::OwnFleet) -> bool {
    fleet.ships.iter().any(|&(ship, class)| {
        class == ShipClass::Mule && view.mule_cargo(ship).is_some_and(|c| c.core_kit)
    })
}

/// A scored candidate.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Candidate {
    pub(crate) site: SiteId,
    pub(crate) system: SystemId,
    pub(crate) holding: HoldingId,
    pub(crate) score: i64,
    pub(crate) risk: i64,
}

/// The goal's score: the running expansion's site, or the best candidate.
pub(crate) fn score(
    view: &impl StrategicView,
    f: &Features,
    running: Option<&Expansion>,
    params: &StrategistParams<'_>,
) -> i64 {
    if let Some(e) = running {
        if matches!(e.stage, Stage::Deploying | Stage::Return) || e.carrying(view) {
            // Finishing a trip: kept above the stand-in.
            return params.rules.weights.garrison.saturating_add(1);
        }
        return candidates(view, f, params)
            .iter()
            .find(|c| c.site == e.site)
            .map_or(0, |c| c.score);
    }
    candidates(view, f, params)
        .first()
        .map_or(0, |c| c.score.max(0))
}

/// Free sites the commander could claim, best first (A2 plan §4.4).
#[expect(
    clippy::too_many_lines,
    reason = "the terms of one score read best together"
)]
pub(crate) fn candidates(
    view: &impl StrategicView,
    f: &Features,
    params: &StrategistParams<'_>,
) -> Vec<Candidate> {
    let rules = params.rules;
    let w = &rules.weights;
    let own_jumps = f.own_jumps();
    let mut sites: Vec<(u32, &SiteInfo)> = f
        .sites
        .iter()
        .filter(|s| s.free)
        .filter_map(|s| own_jumps.get(&s.system).map(|&j| (j, s)))
        .collect();
    sites.sort_by_key(|&(j, s)| (j, s.id));
    sites.truncate(rules.max_candidates);
    let output = f.output();
    let starved = f.crystal_starved();
    let hours_left = f.hours_left();
    let kit = model::weighed(f.economy.core_kit_price, rules.crystal_weight);
    let mut out = Vec::new();
    for (_, site) in sites {
        let Some(holding) = origin(f, site.system) else {
            continue;
        };
        let (Some(from), to) = (
            view.holding_location(holding),
            Location {
                system: site.system,
                position: site.position,
            },
        ) else {
            continue;
        };
        let Some(route) = view.route_to_point(from, to, RouteSpeed::Class(ShipClass::Mule)) else {
            continue;
        };
        // Income: the productive build-out's value to the commander, for its cost.
        let productive = productive_modules(&site.deposits);
        let (alloy, crystal) = model::nominal(&f.numbers, &site.deposits, &[], &productive);
        let value = output.marginal(
            None,
            false,
            Stock { alloy, crystal },
            f.numbers.diminishing_yield_permille,
            rules.crystal_weight,
        );
        let mut cost = kit;
        let mut draw = 0;
        for &(kind, _) in &productive {
            cost += model::weighed(f.numbers.module_cost(kind), rules.crystal_weight);
            draw += f.numbers.module_power(kind).0;
        }
        if draw > f.numbers.core_supply_mw {
            cost += model::weighed(
                f.numbers.module_cost(ModuleKind::Solar),
                rules.crystal_weight,
            );
        }
        if !f.depot_in(site.system) {
            cost += model::weighed(
                f.numbers.module_cost(ModuleKind::Storage),
                rules.crystal_weight,
            );
        }
        let payback_h = u64::try_from(value)
            .ok()
            .filter(|&v| v > 0)
            .map_or(u64::MAX, |v| cost / v);
        let income = if payback_h <= hours_left {
            model::return_score(value, cost)
        } else {
            0
        };
        let control = control(f, site.system, rules.weights);
        let route_h = i64::try_from(route.duration_ms / 3_600_000).unwrap_or(i64::MAX);
        let mut risk = w.risk_per_route_hour.saturating_mul(route_h);
        let mut hostiles = u64::from(f.hostiles.get(&site.system).copied().unwrap_or(0));
        for hop in &route.hops {
            hostiles += u64::from(f.hostiles.get(&hop.to_system).copied().unwrap_or(0));
        }
        risk = risk.saturating_add(
            w.risk_per_hostile
                .saturating_mul(i64::try_from(hostiles).unwrap_or(i64::MAX)),
        );
        risk = risk.saturating_add(
            w.risk_per_rival_holding
                .saturating_mul(rival_holdings(f, site.system)),
        );
        if let Some(&j) = f.haven_jumps.get(&site.system)
            && j > 0
        {
            risk = risk.saturating_add(w.risk_haven >> (j - 1).min(62));
        }
        let vein = site
            .deposits
            .iter()
            .any(|d| d.kind == DepositKind::CrystalVein);
        let score = if starved && vein {
            w.crystal_first.saturating_sub(risk)
        } else {
            income.saturating_add(control).saturating_sub(risk)
        };
        out.push(Candidate {
            site: site.id,
            system: site.system,
            holding,
            score,
            risk,
        });
    }
    out.sort_by_key(|c| (std::cmp::Reverse(c.score), c.site));
    out
}

/// A trip's estimate in ticks: its route time × the margin, plus the deploy margin (D-214).
fn trip_ticks(route_ms: u64, params: &StrategistParams<'_>) -> u64 {
    Ticks::from_millis(
        route_ms.saturating_mul(u64::from(params.rules.trip_margin_permille)) / 1_000,
    )
    .get()
    .saturating_add(Ticks::from_secs(params.rules.deploy_margin_s).get())
}

/// The productive modules a site's blueprint would build (A2 plan §4.5): a harvester or
/// extractor per deposit and a refinery per ore field.
pub(crate) fn productive_modules(deposits: &[super::view::DepositInfo]) -> Vec<(ModuleKind, Slot)> {
    let mut list = Vec::new();
    let mut inner = 0_u8;
    for (i, d) in deposits.iter().enumerate() {
        let slot = Slot::Deposit(u8::try_from(i).unwrap_or(u8::MAX));
        match d.kind {
            DepositKind::OreField => {
                list.push((ModuleKind::Harvester, slot));
                list.push((ModuleKind::Refinery, Slot::Inner(inner)));
                inner += 1;
            }
            DepositKind::CrystalVein => list.push((ModuleKind::Extractor, slot)),
        }
    }
    list
}

/// The own complete holding nearest `system` by jumps, then by identity.
fn origin(f: &Features, system: SystemId) -> Option<HoldingId> {
    let jumps = model::jumps_from(&f.systems, &[system]);
    f.complete()
        .filter_map(|h| jumps.get(&h.system).map(|&j| (j, h.id)))
        .min()
        .map(|(_, id)| id)
}

/// Holdings and outposts of other commanders in `system`.
fn rival_holdings(f: &Features, system: SystemId) -> i64 {
    let n = f
        .sites
        .iter()
        .filter(|s| s.system == system)
        .filter(|s| s.holder.is_some_and(|h| h.owner != f.me))
        .count();
    i64::try_from(n).unwrap_or(i64::MAX)
}

/// The control term of a site in `system` (D-041; A2 plan §4.4).
fn control(f: &Features, system: SystemId, w: super::rules::Weights) -> i64 {
    let Some(info) = f.system(system) else {
        return 0;
    };
    let sites = info.sites.len();
    let majority = sites / 2 + 1;
    let mut own = 0;
    let mut free = 0;
    let mut rivals: std::collections::BTreeMap<nomad_types::CommanderId, usize> =
        std::collections::BTreeMap::new();
    for site in f.sites.iter().filter(|s| s.system == system) {
        match site.holder {
            Some(h) if h.owner == f.me => own += 1,
            Some(h) => *rivals.entry(h.owner).or_default() += 1,
            None if site.free => free += 1,
            None => {}
        }
    }
    if own >= majority {
        return 0;
    }
    let mut term = 0;
    if own + 1 == majority {
        term += w.control_complete;
    } else if own + free >= majority {
        term += w.control_toward;
    }
    if rivals.values().any(|&n| n + 1 == majority) {
        term += w.control_deny;
    }
    term
}

/// Advances an expansion by one step. Returns whether the plan is done.
pub(crate) fn advance(
    view: &impl StrategicView,
    turn: &mut Turn<'_>,
    plan: &mut Plan,
    stats: &mut StrategistStats,
) -> bool {
    let params = view.strategist();
    let f = turn.f;
    if plan.expansion.is_none() {
        let Some(best) = candidates(view, f, &params)
            .into_iter()
            .find(|c| c.score > 0)
        else {
            return true;
        };
        plan.expansion = Some(Expansion {
            site: best.site,
            system: best.system,
            holding: best.holding,
            carrier: None,
            escort: None,
            stage: Stage::Prepare,
        });
        let _ = best.risk;
    }
    let Some(mut e) = plan.expansion.take() else {
        return true;
    };
    // Before the trip, a target that is no longer worth it (taken, out of reach) gives way
    // to the best one; a kit already made or loaded goes there instead.
    if e.stage == Stage::Prepare {
        let list = candidates(view, f, &params);
        if !list.iter().any(|c| c.site == e.site && c.score > 0) {
            if let Some(best) = list.into_iter().find(|c| c.score > 0) {
                e.site = best.site;
                e.system = best.system;
            } else {
                plan.expansion = Some(e);
                return true;
            }
        }
    }
    let done = step(view, turn, plan, &mut e, stats, &params);
    plan.expansion = Some(e);
    done
}

fn step(
    view: &impl StrategicView,
    turn: &mut Turn<'_>,
    plan: &mut Plan,
    e: &mut Expansion,
    stats: &mut StrategistStats,
    params: &StrategistParams<'_>,
) -> bool {
    let f = turn.f;
    let now = f.now;
    let Some(site) = f.site(e.site).cloned() else {
        return true;
    };
    let carrier = e.carrier.and_then(|c| view.own_fleet(c));
    if e.carrier.is_some() && carrier.is_none() {
        // The MULE is gone, and with it any kit (D-064).
        return true;
    }
    let target = Location {
        system: site.system,
        position: site.position,
    };
    // The site is ours now: the MULE returns.
    if site.holder.is_some_and(|h| h.owner == f.me) {
        if e.stage != Stage::Return {
            stats.deployed += 1;
            e.stage = Stage::Return;
        }
        return finish(view, turn, plan, e, &site);
    }
    if !site.free {
        return true;
    }
    for fleet in e.fleets() {
        turn.busy.insert(fleet);
    }
    let carrying = carrier.as_ref().is_some_and(|c| carries_kit(view, c));
    match e.stage {
        Stage::Prepare if carrying => {
            let Some(c) = carrier else {
                return true;
            };
            let from = location_of(view, &c.place);
            let Some(from) = from else {
                plan.observe(marker(&c.place, 2), true, now);
                return false;
            };
            let trip = view.route_to_point(from, target, RouteSpeed::Fleet(c.id));
            match trip {
                Some(route) => {
                    // The trip goes on offline if the session ends first (D-291).
                    travel(turn, &c.place, c.id, target);
                    if ends_in_session(view, params, now, route.duration_ms)
                        && let Some(escort) = escort_for(view, turn, e, &site)
                    {
                        travel(turn, &FleetPlace::Docked(e.holding), escort, target);
                        e.escort = Some(escort);
                        turn.busy.insert(escort);
                    }
                    e.stage = Stage::Travel;
                    plan.observe(marker(&c.place, 3), false, now);
                }
                None => plan.observe(marker(&c.place, 5), false, now),
            }
            false
        }
        Stage::Prepare => prepare(view, turn, plan, e, carrier.as_ref()),
        Stage::Travel => {
            let Some(c) = carrier else {
                return true;
            };
            if !carrying {
                return true;
            }
            match c.place {
                FleetPlace::Travelling => plan.observe(marker(&c.place, 6), true, now),
                FleetPlace::Docked(_) => {
                    e.stage = Stage::Prepare;
                    plan.observe(marker(&c.place, 7), false, now);
                }
                FleetPlace::InSpace(at) if !c.busy => {
                    if at.system == site.system {
                        turn.send(Command::Economy(EconomyCommand::DeployCoreKit {
                            fleet: c.id,
                            site: e.site,
                        }));
                        e.stage = Stage::Deploying;
                    } else {
                        travel(turn, &c.place, c.id, target);
                    }
                    plan.observe(marker(&c.place, 8), false, now);
                }
                FleetPlace::InSpace(_) => plan.observe(marker(&c.place, 9), false, now),
            }
            false
        }
        Stage::Deploying => {
            // The outpost appears once the command applies; until then, wait. A refusal ends
            // the plan (`Expansion::failed`).
            plan.observe(
                carrier.as_ref().map_or(0, |c| marker(&c.place, 10)),
                false,
                now,
            );
            if !carrying {
                e.stage = Stage::Return;
            }
            false
        }
        Stage::Return => finish(view, turn, plan, e, &site),
    }
}

/// Before the kit is loaded: queue it, find a MULE, load.
fn prepare(
    view: &impl StrategicView,
    turn: &mut Turn<'_>,
    plan: &mut Plan,
    e: &mut Expansion,
    carrier: Option<&crate::view::OwnFleet>,
) -> bool {
    let f = turn.f;
    let now = f.now;
    let Some(holding) = f.holding(e.holding).filter(|h| h.complete) else {
        return true;
    };
    let kit_queued = holding.queue.iter().find(|q| q.item == BuildItem::CoreKit);
    let kits = holding.kits;
    let mut marker_parts = u64::from(kits) << 32;
    let mut waiting = false;
    if kits == 0 {
        if let Some(q) = kit_queued {
            marker_parts |= u64::from(q.started);
            // Building, progress comes when it finishes (15 min); waiting, it is paid when
            // the pool has its price (D-068).
            waiting = true;
            if !q.started {
                turn.spend_or_reserve(q.price);
            }
        } else {
            let price = f.economy.core_kit_price;
            waiting = true;
            if f.economy.cap.covers(price)
                && view.build_check(e.holding, BuildItem::CoreKit) == crate::view::BuildCheck::Ok
            {
                turn.build(e.holding, BuildItem::CoreKit);
                turn.spend_or_reserve(price);
            }
        }
    }
    // A MULE for the kit, while it is built.
    let ready = carrier.is_some_and(|c| {
        Features::docked_at(c) == Some(e.holding) && f.is_free_mule(view, c, &BTreeSet::new())
    });
    if ready {
        if kits > 0 {
            if let Some(c) = carrier {
                turn.send(Command::Economy(EconomyCommand::LoadCoreKit {
                    fleet: c.id,
                }));
            }
            marker_parts |= 1 << 8;
        }
    } else if carrier.is_some_and(|c| c.busy || matches!(c.place, FleetPlace::Travelling)) {
        // On its way here.
        waiting = true;
    } else if let Some(c) = carrier.filter(|c| f.is_free_mule(view, c, &BTreeSet::new())) {
        // The plan's MULE, idle elsewhere: it comes here when the trip fits the session.
        let params = view.strategist();
        let hop = location_of(view, &c.place)
            .zip(view.holding_location(e.holding))
            .and_then(|(from, to)| view.route_to_point(from, to, RouteSpeed::Fleet(c.id)));
        let fits = hop.is_some_and(|r| ends_in_session(view, &params, now, r.duration_ms));
        if fits {
            go_dock(turn, &c.place, c.id, e.holding, view);
        }
        waiting = true;
    } else {
        e.carrier = None;
        find_mule(view, turn, e);
        marker_parts |= 1 << 9;
    }
    plan.observe(
        marker_parts ^ e.carrier.map_or(0, |c| c.sim_id().get()),
        waiting,
        now,
    );
    false
}

/// Finds or makes a lone MULE for the kit at its holding.
fn find_mule(view: &impl StrategicView, turn: &mut Turn<'_>, e: &mut Expansion) {
    let f = turn.f;
    // A free lone MULE docked there.
    if let Some(fleet) = f
        .fleets
        .iter()
        .filter(|fl| Features::docked_at(fl) == Some(e.holding))
        .find(|fl| f.is_free_mule(view, fl, &turn.busy))
    {
        e.carrier = Some(fleet.id);
        turn.busy.insert(fleet.id);
        return;
    }
    // An empty MULE in a mixed fleet docked there: split it off. The new fleet is found
    // at the next evaluation.
    for fleet in f
        .fleets
        .iter()
        .filter(|fl| Features::docked_at(fl) == Some(e.holding))
        .filter(|fl| !turn.busy.contains(&fl.id) && !f.convoys.contains(&fl.id))
    {
        let mule = fleet.ships.iter().find(|&&(ship, class)| {
            class == ShipClass::Mule
                && view
                    .mule_cargo(ship)
                    .is_some_and(|c| c.stock.is_zero() && !c.core_kit)
        });
        if let Some(&(ship, _)) = mule
            && fleet.ships.len() > 1
        {
            turn.send(Command::CreateFleet { ships: vec![ship] });
            return;
        }
    }
    // An idle lone MULE elsewhere: bring it.
    if let Some(fleet) = f
        .fleets
        .iter()
        .filter(|fl| Features::docked_at(fl).is_some())
        .find(|fl| f.is_free_mule(view, fl, &turn.busy))
    {
        go_dock(turn, &fleet.place, fleet.id, e.holding, view);
        e.carrier = Some(fleet.id);
        turn.busy.insert(fleet.id);
        return;
    }
    // None: build one, unless one is queued there already.
    let queued = f.holding(e.holding).is_some_and(|h| {
        h.queue.iter().any(|q| {
            q.item
                == BuildItem::Ship {
                    class: ShipClass::Mule,
                }
        })
    });
    if !queued
        && let Some((price, command)) = f.numbers.ship(ShipClass::Mule)
        && f.economy.command_used + turn.command_planned + command <= f.economy.command_cap
        && turn.spend_or_reserve(price)
    {
        turn.build(
            e.holding,
            BuildItem::Ship {
                class: ShipClass::Mule,
            },
        );
        turn.command_planned += command;
    }
}

/// After the deploy: the MULE docks at the nearest holding and is released; the escort
/// guards the outpost until it completes, then docks there and is released.
fn finish(
    view: &impl StrategicView,
    turn: &mut Turn<'_>,
    plan: &mut Plan,
    e: &mut Expansion,
    site: &SiteInfo,
) -> bool {
    let now = turn.f.now;
    let mut done = true;
    let mut marker_parts = 11_u64;
    if let Some(c) = e.carrier.and_then(|c| view.own_fleet(c)) {
        match c.place {
            FleetPlace::Docked(_) => {
                if view.direct_control(c.id).is_some() {
                    turn.send(Command::Doctrine(nomad_types::DoctrineCommand::Release {
                        fleet: c.id,
                    }));
                }
                e.carrier = None;
            }
            FleetPlace::InSpace(_) if !c.busy => {
                turn.fleet_order(c.id, FleetOrder::Retreat { to: None }, Queueing::Replace);
                done = false;
            }
            _ => done = false,
        }
        marker_parts ^= marker(&c.place, 12);
    }
    if let Some(escort) = e.escort.and_then(|c| view.own_fleet(c)) {
        let outpost = site.holder.filter(|h| h.owner == turn.f.me);
        match (escort.place, outpost) {
            (FleetPlace::Docked(_), _) => {
                if view.direct_control(escort.id).is_some() {
                    turn.send(Command::Doctrine(nomad_types::DoctrineCommand::Release {
                        fleet: escort.id,
                    }));
                }
                e.escort = None;
            }
            (FleetPlace::InSpace(_), Some(h)) if h.complete && !escort.busy => {
                turn.fleet_order(
                    escort.id,
                    FleetOrder::Dock { at: h.holding },
                    Queueing::Replace,
                );
                done = false;
            }
            (FleetPlace::InSpace(_), Some(_)) => {
                // Guarding the outpost under construction (D-054): two hours, not stuck.
                plan.observe(marker_parts, true, now);
                return false;
            }
            (FleetPlace::InSpace(_), None) if !escort.busy => {
                turn.fleet_order(
                    escort.id,
                    FleetOrder::Retreat { to: None },
                    Queueing::Replace,
                );
                done = false;
            }
            _ => done = false,
        }
        marker_parts ^= marker(&escort.place, 13);
    }
    plan.observe(marker_parts, false, now);
    done
}

/// An escort for a risky trip, if one is spare.
fn escort_for(
    view: &impl StrategicView,
    turn: &Turn<'_>,
    e: &Expansion,
    site: &SiteInfo,
) -> Option<FleetId> {
    let params = view.strategist();
    let f = turn.f;
    let risk = candidates(view, f, &params)
        .iter()
        .find(|c| c.site == site.id)
        .map_or(0, |c| c.risk);
    if risk <= params.rules.weights.escort_above {
        return None;
    }
    f.fleets
        .iter()
        .filter(|fl| Features::docked_at(fl) == Some(e.holding))
        .filter(|fl| !turn.busy.contains(&fl.id) && !f.convoys.contains(&fl.id) && !fl.busy)
        .find(|fl| {
            !fl.ships.is_empty() && fl.ships.iter().all(|&(_, class)| class != ShipClass::Mule)
        })
        .map(|fl| fl.id)
}

/// Whether a trip of `route_ms` started now ends before the sign-off (D-214).
fn ends_in_session(
    view: &impl StrategicView,
    params: &StrategistParams<'_>,
    now: nomad_types::Tick,
    route_ms: u64,
) -> bool {
    view.session().ends.is_none_or(|ends| {
        now.get().saturating_add(trip_ticks(route_ms, params))
            <= ends
                .get()
                .saturating_sub(params.interval_ticks.saturating_mul(2))
    })
}

/// Where a fleet is, for a route query.
fn location_of(view: &impl StrategicView, place: &FleetPlace) -> Option<Location> {
    match *place {
        FleetPlace::InSpace(at) => Some(at),
        FleetPlace::Docked(h) => view.holding_location(h),
        FleetPlace::Travelling => None,
    }
}

/// `Launch` if docked, `Jump` if in another system, then `Move` to the point.
fn travel(turn: &mut Turn<'_>, place: &FleetPlace, fleet: FleetId, to: Location) {
    let mut queue = Queueing::Replace;
    let system = match *place {
        FleetPlace::Docked(h) => {
            turn.fleet_order(fleet, FleetOrder::Launch, Queueing::Replace);
            queue = Queueing::Append;
            turn.f.holding(h).map(|x| x.system)
        }
        FleetPlace::InSpace(at) => Some(at.system),
        FleetPlace::Travelling => return,
    };
    if system != Some(to.system) {
        turn.fleet_order(fleet, FleetOrder::Jump { system: to.system }, queue);
        queue = Queueing::Append;
    }
    turn.fleet_order(fleet, FleetOrder::Move { to }, queue);
}

/// `Launch` if docked elsewhere, `Jump` if in another system, then `Dock`.
fn go_dock(
    turn: &mut Turn<'_>,
    place: &FleetPlace,
    fleet: FleetId,
    at: HoldingId,
    view: &impl StrategicView,
) {
    let Some(system) = turn.f.holding(at).map(|h| h.system) else {
        return;
    };
    let mut queue = Queueing::Replace;
    let here = match *place {
        FleetPlace::Docked(h) if h == at => return,
        FleetPlace::Docked(h) => {
            turn.fleet_order(fleet, FleetOrder::Launch, Queueing::Replace);
            queue = Queueing::Append;
            turn.f.holding(h).map(|x| x.system)
        }
        FleetPlace::InSpace(loc) => Some(loc.system),
        FleetPlace::Travelling => return,
    };
    let _ = view;
    if here != Some(system) {
        turn.fleet_order(fleet, FleetOrder::Jump { system }, queue);
        queue = Queueing::Append;
    }
    turn.fleet_order(fleet, FleetOrder::Dock { at }, queue);
}

/// A step's observable state: where its fleet is, and a tag.
fn marker(place: &FleetPlace, tag: u64) -> u64 {
    let place = match *place {
        FleetPlace::Docked(h) => h.sim_id().get(),
        FleetPlace::InSpace(at) => {
            // Whole kilometres: drifting in place is not progress.
            let km = |m: f64| {
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "clamped to a small range first"
                )]
                let k = (m / 1_000.0).clamp(-1.0e9, 1.0e9) as i64;
                k.cast_unsigned()
            };
            at.system.sim_id().get()
                ^ km(at.position.x).rotate_left(21)
                ^ km(at.position.y).rotate_left(42)
        }
        FleetPlace::Travelling => 0x7472_6176_656c,
    };
    place.rotate_left(7) ^ tag
}
