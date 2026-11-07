//! Emergency jumps: the old owner's docked ships after a capture, and a fallen mother
//! station with its docked ships (doc 15 §7; D-017, D-034, D-057, D-164 to D-167; S6 plan
//! §4.5–§4.6).
//!
//! **The jump** (D-166): an uninterruptible transit along the fastest route (S3's router:
//! in-system legs at cruise speed, spool-ups and lanes), lasting that route's time,
//! untargetable and invisible like lane transit ([`Place::EmergencyJump`]). On arrival a
//! fleet docks at its owner's holding at the destination if there is room (D-102), and
//! otherwise station-keeps by the core, at the holding's launch point. A fleet's speed is its
//! slowest ship's; ships that jump with their mother station arrive with it.
//!
//! **Docked ships after a capture** (D-057) jump to the old owner's nearest completed holding
//! by route time, ties to the lower holding identity. *Proposed:* if the old owner has no
//! completed holding (their mother station is itself in an emergency jump), they launch
//! where they are and station-keep.
//!
//! **A fallen mother station** (D-034, D-164 to D-167):
//! - the site's other modules are destroyed and its builds cancelled; the site passes to the
//!   capturer as an outpost with its core under construction, at no kit cost (D-057, D-224);
//! - the station keeps its holding identity, its core and its docked ships, leaves the
//!   holdings (it is no holding for control or the standings, D-041, and
//!   `holding::counts_for_control` never sees it), and stays its commander's mother station
//!   for the command cap (S6 plan §4.6); leaving and landing mark the standings for a recount
//!   (`season::mark_dirty`, D-191);
//! - the destination (D-164): a free site in the system of the commander's nearest holding by
//!   route time; if there is none, the free site in the claimable system with the fewest
//!   holdings, ties to the shortest route from the fallen site and then the lowest `SiteId`
//!   (D-165); if there is none, it jumps to its nearest holding and waits there until a site
//!   frees up. *Derived:* within the nearest holding's system the free site with the shortest
//!   route, then the lowest `SiteId`, as D-165 orders sites; *Proposed:* "fewest holdings"
//!   counts every site taken, outposts and reserved sites included, and a commander with no
//!   holding at all waits where the station fell;
//! - the destination is reserved for the jump ([`super::site_reserved`]; *Proposed*, so the
//!   site is still free when it lands);
//! - on landing it is a holding again with a mother-station core (24 docking places, the
//!   command cap base of 120), its core at 25% hull (*Proposed*), a minimal dome (the core's
//!   25% per sector, D-140) and 72 h of protection from the landing (D-034);
//! - the commander receives the comeback grant at the fall ([`super::grant`], D-167).

use nomad_types::{FleetOrder, ModuleKind, Owner, Queueing, Slot};

use super::{Deadline, DefenceState, HoldingSiege, StationJump, StationLeg, capture, grant};
use crate::announce::{Announcement, AnnouncementKind, Recipients};
use crate::command::{dock_check, fleet_cruise_speed};
use crate::economy::{self, EconomyEvent, Hull};
use crate::fleet::route::{self, RouteEnd};
use crate::fleet::{Fleet, FleetEvent, FleetId, Motion, Place, slot_position, station};
use crate::galaxy::{SiteId, SystemId, is_claimable};
use crate::geometry::Vec2;
use crate::holding::{Holding, HoldingId};
use crate::id::CommanderId;
use crate::state::UniverseState;
use crate::step::TickOutputs;
use crate::time::{TICKS_PER_SECOND, Tick, Ticks};

/// A route's duration in whole ticks, rounded up, at least one tick.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a route's duration is finite, positive and far below u64::MAX ticks"
)]
fn ticks_of(duration_s: f64) -> Ticks {
    let ticks = duration_s * TICKS_PER_SECOND as f64;
    if ticks.is_finite() && ticks > 1.0 {
        Ticks::new(libm::ceil(ticks) as u64)
    } else {
        Ticks::new(1)
    }
}

/// The fastest route's time from `from` to `to` at `speed_m_per_s` (D-166).
fn route_ticks(
    state: &UniverseState,
    from: (SystemId, Vec2),
    to: (SystemId, Vec2),
    speed_m_per_s: f64,
) -> Option<Ticks> {
    route::plan(
        &state.galaxy,
        from.0,
        from.1,
        RouteEnd::Point(to.0, to.1),
        speed_m_per_s,
        Ticks::from_secs(state.rules.gates.spool_up_s),
    )
    .map(|r| ticks_of(r.duration_s))
}

/// A site's system and position.
fn site_point(state: &UniverseState, site: SiteId) -> Option<(SystemId, Vec2)> {
    let s = state.galaxy.site(site)?;
    Some((s.system, s.position.into()))
}

/// Where ships appear by a holding on `site`: its launch point (S3 plan §4.4).
fn launch_point_at(state: &UniverseState, site: SiteId) -> Option<(SystemId, Vec2)> {
    let (system, centre) = site_point(state, site)?;
    let layout = &state.rules.mother_station;
    Some((
        system,
        centre + station::grid_to_system(layout, layout.launch_point_voxels),
    ))
}

/// `owner`'s fleets docked at `holding`, in identity order.
fn docked_fleets(state: &UniverseState, holding: HoldingId, owner: CommanderId) -> Vec<FleetId> {
    state
        .fleet_ids()
        .into_iter()
        .filter(|&id| {
            state.entities.with::<Fleet, _>(id.sim_id(), |f| {
                f.place == Place::Docked(holding) && f.owner == Owner::Commander(owner)
            }) == Some(true)
        })
        .collect()
}

/// The old owner's docked ships at the captured `holding` emergency-jump to their nearest
/// holding (D-057, D-166).
pub(super) fn docked_fleets_jump(
    state: &mut UniverseState,
    now: Tick,
    holding: HoldingId,
    owner: CommanderId,
    out: &mut TickOutputs,
) {
    let Some(site) = state.holdings.get(&holding).map(|h| h.site) else {
        return;
    };
    let Some(from) = launch_point_at(state, site) else {
        return;
    };
    let mut jumped = Vec::new();
    for fleet in docked_fleets(state, holding, owner) {
        let Some(speed) = state
            .entities
            .get::<Fleet>(fleet.sim_id())
            .map(|f| fleet_cruise_speed(state, &f))
        else {
            continue;
        };
        let nearest = state
            .holdings
            .iter()
            .filter(|&(&id, h)| id != holding && h.owner == owner && h.core_complete)
            .filter_map(|(&id, h)| {
                let to = launch_point_at(state, h.site)?;
                Some((route_ticks(state, from, to, speed)?, id, h.site))
            })
            .min_by_key(|&(ticks, id, _)| (ticks, id));
        match nearest {
            Some((ticks, _, to)) => {
                let arrive = now.after(ticks);
                start_jump(state, fleet, to, arrive);
                jumped.push((fleet, to, arrive));
            }
            None => place_in_space(state, fleet, from, &mut out.fleet_events),
        }
    }
    announce_jumps(owner, jumped, now, out);
}

fn announce_jumps(
    owner: CommanderId,
    fleets: Vec<(FleetId, SiteId, Tick)>,
    now: Tick,
    out: &mut TickOutputs,
) {
    if !fleets.is_empty() {
        out.announcements.push(Announcement {
            tick: now,
            to: Recipients::Commanders(vec![owner]),
            kind: AnnouncementKind::FleetsJumped {
                commander: owner,
                fleets,
            },
        });
    }
}

/// `fleet` enters an emergency jump to `site`, arriving on `arrive`: uninterruptible, so its
/// orders go (D-166).
fn start_jump(state: &mut UniverseState, fleet: FleetId, site: SiteId, arrive: Tick) {
    if let Some(f) = state.entities.get_mut::<Fleet>(fleet.sim_id()) {
        f.place = Place::EmergencyJump { site, arrive };
        f.active = None;
        f.queue.clear();
        f.engaged_until = None;
    }
    state
        .sieges
        .deadlines
        .insert((arrive, Deadline::FleetArrives(fleet)));
}

/// Puts `fleet` at rest in space at `at`, in formation, station-keeping there (D-166).
fn place_in_space(
    state: &mut UniverseState,
    fleet: FleetId,
    at: (SystemId, Vec2),
    events: &mut Vec<FleetEvent>,
) {
    set_motion(state, fleet, at.1);
    if let Some(f) = state.entities.get_mut::<Fleet>(fleet.sim_id()) {
        f.place = Place::InSystem(at.0);
    }
    crate::fleet::orders::give(
        state,
        fleet,
        FleetOrder::StationKeep { relative_to: None },
        Queueing::Replace,
        events,
    );
}

/// Puts `fleet`'s anchor at rest at `point` and its ships in their formation slots.
fn set_motion(state: &mut UniverseState, fleet: FleetId, point: Vec2) {
    let anchor = Motion {
        position: point.into(),
        velocity: Vec2::ZERO,
        heading_rad: 0.0,
    };
    let members: Vec<(crate::fleet::ShipId, Vec2)> = state
        .entities
        .get::<Fleet>(fleet.sim_id())
        .map(|f| f.members.iter().map(|(&s, &o)| (s, o)).collect())
        .unwrap_or_default();
    for (ship, offset) in members {
        if let Some(m) = state.entities.get_mut::<Motion>(ship.sim_id()) {
            *m = Motion {
                position: slot_position(&anchor, offset).into(),
                ..anchor
            };
        }
    }
    if let Some(m) = state.entities.get_mut::<Motion>(fleet.sim_id()) {
        *m = anchor;
    }
}

/// `fleet`'s emergency jump arrives on `now`: it docks at its owner's holding there if there
/// is room, otherwise it station-keeps by the core (D-166).
pub(super) fn fleet_arrives(
    state: &mut UniverseState,
    now: Tick,
    fleet: FleetId,
    out: &mut TickOutputs,
) {
    let Some(f) = state.entities.get::<Fleet>(fleet.sim_id()) else {
        return;
    };
    let Place::EmergencyJump { site, arrive } = f.place else {
        return;
    };
    if arrive > now {
        return;
    }
    let Some(at) = launch_point_at(state, site) else {
        return;
    };
    let dock = state
        .holding_at(site)
        .filter(|(_, h)| f.owner == Owner::Commander(h.owner))
        .map(|(id, _)| id)
        .filter(|&id| dock_check(state, &f, id).is_ok());
    match dock {
        Some(holding) => {
            set_motion(state, fleet, at.1);
            if let Some(f) = state.entities.get_mut::<Fleet>(fleet.sim_id()) {
                f.place = Place::Docked(holding);
            }
            out.fleet_events.push(FleetEvent::Docked { fleet, holding });
        }
        None => place_in_space(state, fleet, at, &mut out.fleet_events),
    }
}

/// `holding`, a mother station, fell on `now` and `capturer` gains its site (D-034, D-057,
/// D-164 to D-167).
pub(super) fn mother_falls(
    state: &mut UniverseState,
    now: Tick,
    holding: HoldingId,
    capturer: CommanderId,
    out: &mut TickOutputs,
) {
    let Some(h) = state.holdings.get(&holding).cloned() else {
        return;
    };
    let (owner, site) = (h.owner, h.site);
    // The site's other modules are destroyed (D-034), and its builds cancelled.
    capture::cancel_builds(state, holding, &mut out.economy_events);
    capture::clear_standing_orders(state, holding);
    let others: Vec<_> = state
        .economy
        .holdings
        .get(&holding)
        .map(|e| {
            e.slots
                .iter()
                .filter(|&(&slot, _)| slot != Slot::Core)
                .map(|(_, &id)| id)
                .collect()
        })
        .unwrap_or_default();
    for module in others {
        economy::destroy_module(state, module, &mut out.economy_events);
    }
    if let Some(e) = state.economy.holdings.get_mut(&holding) {
        e.ore = 0;
        e.buffer = economy::Stock::ZERO;
        e.regen_demand_kw = [0; 4];
    }
    let docked = docked_fleets(state, holding, owner);
    // The station leaves its site: no holding until it lands (D-041).
    state.holdings.remove(&holding);
    state.defence.holdings.remove(&holding);
    capture::end_siege(state, holding);
    // A holding left: the standings are counted again in phase 9 (D-191).
    crate::season::mark_dirty(state);

    // The cleared site passes to the capturer as an outpost (D-057, D-224).
    let outpost = state.insert_holding(site, capturer, false).ok();
    if let Some(outpost) = outpost {
        economy::spawn_outpost_core(state, outpost);
        out.economy_events.push(EconomyEvent::OutpostDeployed {
            holding: outpost,
            site,
        });
    }

    // The comeback grant, paid into the pool whatever its cap (D-167).
    let grant = grant::comeback_grant(state, owner);
    let pool = state.economy.pools.entry(owner).or_default();
    *pool = pool.plus(grant);

    // The destination and the jump (D-164 to D-166).
    let from = site_point(state, site);
    let leg = from.map_or(StationLeg::Waiting { by: None }, |from| {
        plan_leg(state, owner, from, now)
    });
    begin_leg(state, holding, leg);
    state.sieges.stations.insert(
        holding,
        StationJump {
            owner,
            from_site: site,
            leg,
        },
    );
    // Its docked ships go with it (D-034).
    let mut jumped = Vec::new();
    for fleet in docked {
        let to = match leg {
            StationLeg::ToSite { site, arrive } => Some((site, arrive)),
            StationLeg::ToHolding { holding, arrive } => {
                state.holdings.get(&holding).map(|h| (h.site, arrive))
            }
            StationLeg::Waiting { .. } => None,
        };
        match (to, launch_point_at(state, site)) {
            (Some((to, arrive)), _) => {
                start_jump(state, fleet, to, arrive);
                jumped.push((fleet, to, arrive));
            }
            (None, Some(at)) => place_in_space(state, fleet, at, &mut out.fleet_events),
            (None, None) => {}
        }
    }

    if let Some(outpost) = outpost {
        out.announcements.push(Announcement {
            tick: now,
            to: Recipients::Public,
            kind: AnnouncementKind::MotherStationFell {
                site,
                from: owner,
                to: capturer,
                outpost,
            },
        });
    }
    out.announcements.push(Announcement {
        tick: now,
        to: Recipients::Commanders(vec![owner]),
        kind: jumped_kind(state, holding, site, leg, grant),
    });
    announce_jumps(owner, jumped, now, out);
}

/// The owner's announcement of a station's jump.
fn jumped_kind(
    state: &UniverseState,
    holding: HoldingId,
    from_site: SiteId,
    leg: StationLeg,
    grant: economy::Stock,
) -> AnnouncementKind {
    let protection = Ticks::from_hours(u64::from(state.rules.sieges.jump_protection_hours));
    let (to_site, arrival_tick, protected_until) = match leg {
        StationLeg::ToSite { site, arrive } => {
            (Some(site), Some(arrive), Some(arrive.after(protection)))
        }
        StationLeg::ToHolding { arrive, .. } => (None, Some(arrive), None),
        StationLeg::Waiting { .. } => (None, None, None),
    };
    AnnouncementKind::MotherStationJumped {
        holding,
        from_site,
        to_site,
        arrival_tick,
        protected_until,
        grant,
    }
}

/// Schedules the arrival of a station's `leg`.
fn begin_leg(state: &mut UniverseState, holding: HoldingId, leg: StationLeg) {
    match leg {
        StationLeg::ToSite { arrive, .. } | StationLeg::ToHolding { arrive, .. } => {
            state
                .sieges
                .deadlines
                .insert((arrive, Deadline::StationArrives(holding)));
        }
        StationLeg::Waiting { .. } => {}
    }
}

/// Whether `site` is free for a fallen mother station: in a claimable system, with no
/// holding or outpost, and not reserved by another station's jump (D-164).
fn site_free(state: &UniverseState, site: SiteId) -> bool {
    state
        .galaxy
        .site(site)
        .and_then(|s| state.galaxy.system(s.system))
        .is_some_and(|sys| is_claimable(sys.kind))
        && state.holding_at(site).is_none()
        && !super::site_reserved(state, site)
}

/// Where `owner`'s fallen mother station at `from` goes, leaving on `now` (D-164, D-165).
fn plan_leg(
    state: &UniverseState,
    owner: CommanderId,
    from: (SystemId, Vec2),
    now: Tick,
) -> StationLeg {
    let speed = f64::from(state.rules.sieges.station_jump_speed_m_per_s);
    let to_site = |site: SiteId| {
        let to = site_point(state, site)?;
        route_ticks(state, from, to, speed)
    };
    // The commander's nearest holding by route time, ties to the lower identity.
    let nearest = state
        .holdings
        .iter()
        .filter(|(_, h)| h.owner == owner && h.core_complete)
        .filter_map(|(&id, h)| Some((to_site(h.site)?, id, h.site)))
        .min_by_key(|&(ticks, id, _)| (ticks, id));
    // 1. A free site in its system: the shortest route, then the lowest SiteId.
    if let Some((_, _, near_site)) = nearest
        && let Some(system) = state.galaxy.site(near_site).map(|s| s.system)
        && let Some((ticks, site)) = state
            .galaxy
            .system(system)
            .into_iter()
            .flat_map(|sys| sys.sites.iter().copied())
            .filter(|&s| site_free(state, s))
            .filter_map(|s| Some((to_site(s)?, s)))
            .min()
    {
        return StationLeg::ToSite {
            site,
            arrive: now.after(ticks),
        };
    }
    // 2. The free site in the claimable system with the fewest holdings, then the shortest
    // route, then the lowest SiteId (D-165).
    let taken_in = |system: SystemId| {
        state.galaxy.system(system).map_or(0, |sys| {
            sys.sites
                .iter()
                .filter(|&&s| state.holding_at(s).is_some() || super::site_reserved(state, s))
                .count()
        })
    };
    if let Some((_, ticks, site)) = state
        .galaxy
        .systems()
        .filter(|(_, sys)| is_claimable(sys.kind))
        .flat_map(|(id, sys)| sys.sites.iter().map(move |&s| (id, s)))
        .filter(|&(_, s)| site_free(state, s))
        .filter_map(|(system, s)| Some((taken_in(system), to_site(s)?, s)))
        .min()
    {
        return StationLeg::ToSite {
            site,
            arrive: now.after(ticks),
        };
    }
    // 3. No free site: wait by the nearest holding until one frees up (D-164).
    match nearest {
        Some((ticks, holding, _)) => StationLeg::ToHolding {
            holding,
            arrive: now.after(ticks),
        },
        None => StationLeg::Waiting { by: None },
    }
}

/// A fallen mother station's leg arrives on `now`.
pub(super) fn station_arrives(
    state: &mut UniverseState,
    now: Tick,
    holding: HoldingId,
    out: &mut TickOutputs,
) {
    let Some(jump) = state.sieges.stations.get(&holding).cloned() else {
        return;
    };
    match jump.leg {
        StationLeg::ToSite { site, arrive } if arrive <= now => {
            land(state, now, holding, site, jump.owner, out);
        }
        StationLeg::ToHolding {
            holding: by,
            arrive,
        } if arrive <= now => {
            if let Some(j) = state.sieges.stations.get_mut(&holding) {
                j.leg = StationLeg::Waiting { by: Some(by) };
            }
            // A site may have freed up on the way.
            retry_waiting(state, now, out);
        }
        StationLeg::ToSite { .. } | StationLeg::ToHolding { .. } | StationLeg::Waiting { .. } => {}
    }
}

/// The station `holding` lands on `site` on `now`: a holding again, protected for 72 h
/// (D-034, D-166).
fn land(
    state: &mut UniverseState,
    now: Tick,
    holding: HoldingId,
    site: SiteId,
    owner: CommanderId,
    out: &mut TickOutputs,
) {
    state.sieges.stations.remove(&holding);
    state.holdings.insert(
        holding,
        Holding {
            site,
            owner,
            core_complete: true,
            mother_station: true,
        },
    );
    state.economy.holdings.entry(holding).or_default();
    let rules = &state.rules.sieges;
    let (percent, protection) = (
        rules.landing_core_percent,
        Ticks::from_hours(u64::from(rules.jump_protection_hours)),
    );
    let core = state
        .economy
        .holdings
        .get(&holding)
        .and_then(|e| e.slots.get(&Slot::Core).copied());
    match core {
        Some(core) => {
            if let Some(hull) = state.entities.get_mut::<Hull>(core) {
                let landed = u32::try_from(u64::from(hull.max) * u64::from(percent) / 100)
                    .unwrap_or(hull.max);
                hull.hp = hull.hp.max(landed);
            }
        }
        // Defensive: a station always keeps its core.
        None => {
            economy::spawn_module(state, holding, Slot::Core, ModuleKind::Core, None);
        }
    }
    let breachable_from = now.after(protection);
    state.sieges.holdings.insert(
        holding,
        HoldingSiege {
            state: DefenceState::Nominal,
            breachable_from,
        },
    );
    // A holding again: the standings are counted again in phase 9 (D-191).
    crate::season::mark_dirty(state);
    out.announcements.push(Announcement {
        tick: now,
        to: Recipients::Public,
        kind: AnnouncementKind::StationLanded {
            commander: owner,
            holding,
            site,
            breachable_from,
        },
    });
}

/// Stations waiting for a free site try again on `now`, in identity order (D-164): a site
/// freed up, or one arrived where it waits.
pub(super) fn retry_waiting(state: &mut UniverseState, now: Tick, out: &mut TickOutputs) {
    let waiting: Vec<(HoldingId, CommanderId, SiteId, Option<HoldingId>)> = state
        .sieges
        .stations
        .iter()
        .filter_map(|(&id, j)| match j.leg {
            StationLeg::Waiting { by } => Some((id, j.owner, j.from_site, by)),
            StationLeg::ToSite { .. } | StationLeg::ToHolding { .. } => None,
        })
        .collect();
    for (holding, owner, from_site, by) in waiting {
        let at = by
            .and_then(|h| state.holdings.get(&h).map(|h| h.site))
            .unwrap_or(from_site);
        let Some(from) = site_point(state, at) else {
            continue;
        };
        let leg = plan_leg(state, owner, from, now);
        if let StationLeg::ToSite { .. } = leg {
            begin_leg(state, holding, leg);
            if let Some(j) = state.sieges.stations.get_mut(&holding) {
                j.leg = leg;
            }
            out.announcements.push(Announcement {
                tick: now,
                to: Recipients::Commanders(vec![owner]),
                kind: jumped_kind(state, holding, from_site, leg, economy::Stock::ZERO),
            });
        }
    }
}
