//! The command validator and command execution (D-077; S3 plan §4.5, doc 20 §5).
//!
//! **One validator serves every source of commands:** human players, the Deputy, AI
//! commanders and the raider director all issue [`Command`]s through
//! [`validate_command`] (D-077). It checks:
//!
//! - **ownership:** the issuer owns the fleet, ships or holding it commands;
//! - **targets exist:** fleets, ships, holdings, systems, and positions that are finite and
//!   inside the system;
//! - **the order applies:** a docked fleet must launch first; a target must be in the system
//!   the fleet will be in when the order starts (S3 plan §6 task 0); fleets in lane transit
//!   cannot be reorganised;
//! - **reachability:** a jump destination has a route; a point is not inside an obstacle;
//! - **the fleet's composition:** attacks need an armed ship; frigates dock only where there
//!   is a dry dock (D-102); docking needs room for every ship;
//! - **station orders** (S5b, [`crate::defence`]): abilities need a completed holding, an
//!   existing sector that is not open, an elapsed cooldown and their power (D-128).
//! - **the commander's own settings** (S6a): an exposure window is chosen by a commander of
//!   the Universe, starts on a quarter hour, and only in the first 7 days (D-157).
//!
//! A rejection carries its reason ([`CommandRejection`]). The visibility check ("you can
//! only target what you detect", D-039) is added with sensors in S7; until then every entity
//! counts as visible.
//!
//! **Where the fleet will be:** an order that replaces the queue starts from where the fleet
//! is (or, in lane transit, where it will arrive: transit cannot be interrupted, D-042). An
//! appended order starts where the queue ends: after a jump, in the destination system;
//! after a dock or retreat, docked.

use nomad_types::{
    Command, FleetOrder, HoldingOrder, Location, Owner, PositionMetres, Queueing, ShipId, SimId,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::fleet::route::{self, RouteEnd};
use crate::fleet::{ActiveOrder, Fleet, FleetEvent, FleetId, Motion, Place, Ship, Task, station};
use crate::galaxy::{GateId, SystemId};
use crate::geometry::{Vec2, distance_m};
use crate::holding::HoldingId;
use crate::state::UniverseState;
use crate::time::Ticks;

/// Why a command was rejected (D-077; doc 20 §5).
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Error, Serialize, Deserialize,
)]
pub enum CommandRejection {
    /// The fleet does not exist.
    #[error("no such fleet")]
    UnknownFleet,
    /// A ship does not exist.
    #[error("no such ship")]
    UnknownShip,
    /// The holding does not exist.
    #[error("no such holding")]
    UnknownHolding,
    /// The target or object does not exist, or is not in space (docked, in lane transit).
    #[error("no such target in space")]
    UnknownTarget,
    /// The system does not exist.
    #[error("no such system")]
    UnknownSystem,
    /// The issuer does not own the fleet, ship or holding it commands.
    #[error("not owned by the issuer")]
    NotOwner,
    /// The holding to dock or retreat at belongs to someone else.
    #[error("the holding belongs to someone else")]
    HoldingNotOwned,
    /// A position is not finite, or lies outside the system.
    #[error("invalid position")]
    InvalidPosition,
    /// The target is not in the system the fleet will be in; another system is reached
    /// with Jump.
    #[error("the target is in another system")]
    WrongSystem,
    /// The point lies inside an obstacle footprint, or no lane route leads there.
    #[error("unreachable")]
    Unreachable,
    /// A jump to the system the fleet will already be in.
    #[error("already in that system")]
    AlreadyThere,
    /// The fleet will be docked; it must launch first.
    #[error("the fleet is docked; launch first")]
    Docked,
    /// Launch for a fleet that will not be docked.
    #[error("the fleet is not docked")]
    NotDocked,
    /// A fleet or ship in lane transit cannot be reorganised (D-042).
    #[error("in lane transit")]
    InTransit,
    /// The holding cannot take every ship of the fleet (D-102).
    #[error("not enough docking room")]
    NoCapacity,
    /// The fleet has a frigate and the holding has no dry dock (D-102).
    #[error("frigates need a dry dock")]
    NoDryDock,
    /// Retreat: no owned holding can take the fleet.
    #[error("no holding to retreat to")]
    NoHolding,
    /// The fleet has no armed ship.
    #[error("the fleet has no weapons")]
    NoWeapons,
    /// The target belongs to the issuer.
    #[error("the target is friendly")]
    FriendlyTarget,
    /// The fleet would guard, follow or merge with itself.
    #[error("the fleet itself")]
    SelfTarget,
    /// A patrol without waypoints.
    #[error("empty patrol route")]
    EmptyRoute,
    /// A new fleet without ships.
    #[error("no ships")]
    EmptyFleet,
    /// The same ship is listed twice.
    #[error("a ship is listed twice")]
    DuplicateShip,
    /// The ships or fleets are not in one place: one system in space, or one holding.
    #[error("not together")]
    NotTogether,
    /// Launch-ready at a holding where none of the issuer's fleets is docked.
    #[error("nothing to launch")]
    NothingToLaunch,
    /// The holding is an outpost: its core is not complete yet (D-041).
    #[error("the holding is an outpost")]
    Outpost,
    /// Cores and NPC classes cannot be built.
    #[error("cannot be built")]
    NotBuildable,
    /// The holding has no such slot (D-130, D-131).
    #[error("no such slot")]
    NoSuchSlot,
    /// The module kind does not go into that slot, or the deposit there does not suit it.
    #[error("wrong slot for this module")]
    WrongSlot,
    /// The slot has a module, or one is queued for it.
    #[error("the slot is taken")]
    SlotTaken,
    /// The holding's build queue is full.
    #[error("the build queue is full")]
    QueueFull,
    /// No such item in the holding's queue.
    #[error("no such queue item")]
    UnknownItem,
    /// The fleet has no MULE.
    #[error("the fleet has no MULE")]
    NoMule,
    /// Every MULE of the fleet carries something.
    #[error("no empty MULE")]
    CargoFull,
    /// No core kit to load or deploy.
    #[error("no core kit")]
    NoKit,
    /// The site does not exist.
    #[error("no such site")]
    UnknownSite,
    /// The site is in a system that cannot be claimed.
    #[error("the site cannot be claimed")]
    NotClaimable,
    /// The site has a holding or an outpost.
    #[error("the site is taken")]
    SiteTaken,
    /// The fleet is not close enough to the site.
    #[error("too far away")]
    TooFar,
    /// A convoy route with more stops than the rules allow.
    #[error("the route has too many stops")]
    RouteTooLong,
    /// A convoy route lists a holding twice.
    #[error("a stop is listed twice")]
    DuplicateStop,
    /// A convoy route does not end at a depot (D-134).
    #[error("the route must end at a depot")]
    NotDepot,

    // Station orders (S5b; doc 15 §5.3).
    /// The holding's dome has no such sector.
    #[error("no such shield sector")]
    NoSuchSector,
    /// The ability is cooling down.
    #[error("the ability is cooling down")]
    AbilityCooldown,
    /// SHLD SURGE on an open sector (doc 15 §5.3).
    #[error("the sector is open")]
    SectorOpen,
    /// The holding cannot supply the ability's power draw (D-128).
    #[error("not enough power")]
    NoPower,

    // S6: offline and siege rules.
    /// The issuer is not a commander of this Universe.
    #[error("no such commander")]
    UnknownCommander,
    /// The window start is not a quarter hour of the day (D-155).
    #[error("not a quarter hour of the day")]
    InvalidWindow,
    /// The free choice of the exposure window is over: the first 7 days have passed (D-157).
    #[error("the window can no longer be chosen")]
    WindowLocked,
}

/// Checks `command` from `issuer` against the current state (D-077).
pub fn validate_command(
    state: &UniverseState,
    issuer: Owner,
    command: &Command,
) -> Result<(), CommandRejection> {
    match command {
        Command::Fleet {
            fleet,
            order,
            queue,
        } => validate_order(state, issuer, *fleet, order, *queue),
        Command::Holding { holding, order } => {
            validate_holding_order(state, issuer, *holding, *order)
        }
        Command::CreateFleet { ships } => validate_create(state, issuer, ships),
        Command::MergeFleets { fleet, into } => validate_merge(state, issuer, *fleet, *into),
        Command::Economy(command) => crate::economy::validate(state, issuer, command),
        Command::SetExposureWindow { start } => {
            crate::commander::validate_window_choice(state, issuer, *start)
        }
    }
}

/// Validates `command` and, if it is valid, carries it out.
pub(crate) fn execute(
    state: &mut UniverseState,
    issuer: Owner,
    command: &Command,
    events: &mut Vec<FleetEvent>,
    economy_events: &mut Vec<crate::economy::EconomyEvent>,
) -> Result<(), CommandRejection> {
    validate_command(state, issuer, command)?;
    match command {
        Command::Fleet {
            fleet,
            order,
            queue,
        } => {
            // An order to a convoy fleet ends its route (D-134).
            crate::economy::end_convoy(state, *fleet, economy_events);
            crate::fleet::orders::give(state, *fleet, order.clone(), *queue, events);
        }
        Command::Holding {
            holding,
            order: HoldingOrder::LaunchReady,
        } => {
            for fleet in docked_fleets(state, issuer, *holding) {
                crate::fleet::orders::launch(state, fleet, events);
            }
        }
        // Station orders (S5b).
        Command::Holding { holding, order } => crate::defence::execute(state, *holding, *order),
        Command::CreateFleet { ships } => {
            crate::fleet::create_fleet(state, issuer, ships);
        }
        Command::MergeFleets { fleet, into } => crate::fleet::merge_fleets(state, *fleet, *into),
        Command::Economy(command) => crate::economy::execute(state, command, economy_events),
        Command::SetExposureWindow { start } => {
            crate::commander::choose_window(state, issuer, *start);
        }
    }
    Ok(())
}

/// Where a fleet will be when an order starts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Context {
    pub(crate) system: SystemId,
    pub(crate) docked: Option<HoldingId>,
    /// Its position then, if known.
    pub(crate) position: Option<Vec2>,
}

/// Something an order can aim at, in space (S3: every entity counts as visible).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Target {
    pub(crate) system: SystemId,
    pub(crate) position: Vec2,
    pub(crate) owner: Option<Owner>,
    /// The fleet it is, or whose ship it is.
    pub(crate) fleet: Option<FleetId>,
}

/// Finds an entity in space: a fleet (its anchor), a ship, a holding, a site or a gate.
/// Docked fleets and ships, and those in lane transit, cannot be targeted.
pub(crate) fn locate(state: &UniverseState, id: SimId) -> Option<Target> {
    let in_space = |fleet: &Fleet| match fleet.place {
        Place::InSystem(system) => Some(system),
        Place::Docked(_) | Place::InTransit { .. } => None,
    };
    if let Some((system, owner)) = state
        .entities
        .with::<Fleet, _>(id, |f| (in_space(f), f.owner))
    {
        let system = system?;
        let anchor = state.entities.get::<Motion>(id)?;
        return Some(Target {
            system,
            position: anchor.position.into(),
            owner: Some(owner),
            fleet: Some(FleetId(id)),
        });
    }
    if let Some(ship) = state.entities.get::<Ship>(id) {
        let system = state
            .entities
            .with::<Fleet, _>(ship.fleet.sim_id(), in_space)??;
        let motion = state.entities.get::<Motion>(id)?;
        return Some(Target {
            system,
            position: motion.position.into(),
            owner: Some(ship.owner),
            fleet: Some(ship.fleet),
        });
    }
    if let Some(holding) = state.holdings.get(&HoldingId(id)) {
        let site = state.galaxy.site(holding.site)?;
        return Some(Target {
            system: site.system,
            position: site.position.into(),
            owner: Some(Owner::Commander(holding.owner)),
            fleet: None,
        });
    }
    if let Some(site) = state.galaxy.site(crate::galaxy::SiteId(id)) {
        return Some(Target {
            system: site.system,
            position: site.position.into(),
            owner: None,
            fleet: None,
        });
    }
    // A module (S5b): any but a pylon, which cannot be targeted (D-053).
    if let Some((system, position, owner)) = crate::defence::locate_module(state, id) {
        return Some(Target {
            system,
            position,
            owner: Some(owner),
            fleet: None,
        });
    }
    let gate = state.galaxy.gate(GateId(id))?;
    Some(Target {
        system: gate.system,
        position: gate.position.into(),
        owner: None,
        fleet: None,
    })
}

/// How far from the star a point of `system` may be: its farthest site or gate plus the
/// ops-area radius. Gates sit at the edge of a system (doc 10 §4.1), so this is the edge plus
/// the gate approach zone.
pub(crate) fn system_radius_m(state: &UniverseState, system: SystemId) -> Option<f64> {
    let sys = state.galaxy.system(system)?;
    let sites = sys
        .sites
        .iter()
        .filter_map(|&s| state.galaxy.site(s).map(|s| s.position));
    let gates = sys
        .gates
        .iter()
        .filter_map(|&g| state.galaxy.gate(g).map(|g| g.position));
    let farthest = sites
        .chain(gates)
        .map(|p| distance_m(p, PositionMetres::ORIGIN))
        .fold(0.0, f64::max);
    Some(farthest + state.rules.movement.ops_area_radius_m)
}

/// The slowest cruise speed among `fleet`'s ships: the speed routes are timed at (doc 15
/// §3.4).
pub(crate) fn fleet_cruise_speed(state: &UniverseState, fleet: &Fleet) -> f64 {
    fleet
        .members
        .keys()
        .filter_map(|ship| state.entities.get::<Ship>(ship.sim_id()))
        .map(|ship| state.rules.class(ship.class).cruise_speed_m_per_s)
        .fold(f64::INFINITY, f64::min)
}

/// Whether some ship of `fleet` is armed.
fn is_armed(state: &UniverseState, fleet: &Fleet) -> bool {
    fleet.members.keys().any(|ship| {
        state
            .entities
            .get::<Ship>(ship.sim_id())
            .is_some_and(|s| state.rules.class(s.class).weapon_range_m.is_some())
    })
}

/// Whether some ship of `fleet` is a frigate.
fn has_frigate(state: &UniverseState, fleet: &Fleet) -> bool {
    fleet.members.keys().any(|ship| {
        state
            .entities
            .get::<Ship>(ship.sim_id())
            .is_some_and(|s| state.rules.class(s.class).frigate)
    })
}

/// Whether `holding` can take every ship of `fleet` now (D-102).
pub(crate) fn dock_check(
    state: &UniverseState,
    fleet: &Fleet,
    holding: HoldingId,
) -> Result<(), CommandRejection> {
    let h = state
        .holdings
        .get(&holding)
        .ok_or(CommandRejection::UnknownHolding)?;
    if fleet.owner != Owner::Commander(h.owner) {
        return Err(CommandRejection::HoldingNotOwned);
    }
    if has_frigate(state, fleet) && state.dry_docks(holding) == 0 {
        return Err(CommandRejection::NoDryDock);
    }
    let size = u32::try_from(fleet.members.len()).unwrap_or(u32::MAX);
    let free = state
        .docking_capacity(holding)
        .saturating_sub(state.docked_ships(holding));
    if size > free {
        return Err(CommandRejection::NoCapacity);
    }
    Ok(())
}

/// The issuer's fleets docked at `holding`, in identity order.
fn docked_fleets(state: &UniverseState, issuer: Owner, holding: HoldingId) -> Vec<FleetId> {
    state
        .fleet_ids()
        .into_iter()
        .filter(|&id| {
            state.entities.with::<Fleet, _>(id.sim_id(), |f| {
                f.owner == issuer && f.place == Place::Docked(holding)
            }) == Some(true)
        })
        .collect()
}

/// Where `fleet` is now, as an order context.
fn current_context(state: &UniverseState, id: FleetId, fleet: &Fleet) -> Option<Context> {
    match fleet.place {
        Place::InSystem(system) => Some(Context {
            system,
            docked: None,
            position: state
                .entities
                .get::<Motion>(id.sim_id())
                .map(|m| m.position.into()),
        }),
        Place::Docked(holding) => Some(Context {
            system: holding_system(state, holding)?,
            docked: Some(holding),
            position: station::launch_point(state, holding).map(Vec2::from),
        }),
        Place::InTransit { to_gate, .. } => {
            let gate = state.galaxy.gate(to_gate)?;
            Some(Context {
                system: gate.system,
                docked: None,
                position: Some(gate.position.into()),
            })
        }
    }
}

fn holding_system(state: &UniverseState, holding: HoldingId) -> Option<SystemId> {
    let site = state.holdings.get(&holding)?.site;
    Some(state.galaxy.site(site)?.system)
}

/// Where the fleet will be after `order`, which starts in `ctx`.
fn after(state: &UniverseState, fleet: &Fleet, ctx: Context, order: &FleetOrder) -> Context {
    match order {
        FleetOrder::Move { to } | FleetOrder::AttackMove { to } => Context {
            position: Some(to.position.into()),
            ..ctx
        },
        FleetOrder::Jump { system } => Context {
            system: *system,
            docked: None,
            position: None,
        },
        FleetOrder::Dock { at } => Context {
            docked: Some(*at),
            ..ctx
        },
        FleetOrder::Retreat { to } => {
            match to.or_else(|| nearest_holding(state, fleet, ctx).map(|(h, _)| h)) {
                Some(holding) => Context {
                    system: holding_system(state, holding).unwrap_or(ctx.system),
                    docked: Some(holding),
                    position: station::launch_point(state, holding).map(Vec2::from),
                },
                None => ctx,
            }
        }
        FleetOrder::Launch => Context {
            docked: None,
            ..ctx
        },
        FleetOrder::Attack { .. }
        | FleetOrder::Patrol { .. }
        | FleetOrder::Guard { .. }
        | FleetOrder::StationKeep { .. }
        | FleetOrder::SetRulesOfEngagement { .. } => ctx,
    }
}

/// Where the fleet will be after its active order: the end of a route, or docked.
fn after_task(state: &UniverseState, ctx: Context, active: &ActiveOrder) -> Context {
    match &active.task {
        Task::Travel {
            hops, then_dock, ..
        } => {
            let system = hops
                .back()
                .and_then(|hop| state.galaxy.gate(hop.to_gate))
                .map_or(ctx.system, |g| g.system);
            Context {
                system,
                docked: *then_dock,
                position: None,
            }
        }
        Task::Dock { holding } => Context {
            docked: Some(*holding),
            ..ctx
        },
        Task::MoveTo { to } => Context {
            position: Some((*to).into()),
            ..ctx
        },
        Task::Patrol { .. }
        | Task::StationKeep { .. }
        | Task::KeepStationOn { .. }
        | Task::Guard { .. }
        | Task::Attack { .. } => ctx,
    }
}

/// The context a new order of `fleet` starts in.
fn order_context(
    state: &UniverseState,
    id: FleetId,
    fleet: &Fleet,
    queue: Queueing,
) -> Option<Context> {
    let mut ctx = current_context(state, id, fleet)?;
    if queue == Queueing::Append {
        if let Some(active) = &fleet.active {
            ctx = after_task(state, ctx, active);
        }
        for order in &fleet.queue {
            ctx = after(state, fleet, ctx, order);
        }
    }
    Some(ctx)
}

/// The nearest owned holding that can take `fleet`, by route time (S3 plan §4.4), and that
/// route's duration. Ties go to the lower holding identity.
pub(crate) fn nearest_holding(
    state: &UniverseState,
    fleet: &Fleet,
    ctx: Context,
) -> Option<(HoldingId, f64)> {
    let speed = fleet_cruise_speed(state, fleet);
    let spool = Ticks::from_secs(state.rules.gates.spool_up_s);
    let start = ctx.position.unwrap_or(Vec2::ZERO);
    let mut best: Option<(HoldingId, f64)> = None;
    for &holding in state.holdings.keys() {
        if dock_check(state, fleet, holding).is_err() {
            continue;
        }
        let (Some(system), Some(point)) = (
            holding_system(state, holding),
            station::launch_point(state, holding),
        ) else {
            continue;
        };
        let Some(r) = route::plan(
            &state.galaxy,
            ctx.system,
            start,
            RouteEnd::Point(system, point.into()),
            speed,
            spool,
        ) else {
            continue;
        };
        if best.is_none_or(|(_, d)| r.duration_s < d) {
            best = Some((holding, r.duration_s));
        }
    }
    best
}

fn validate_order(
    state: &UniverseState,
    issuer: Owner,
    id: FleetId,
    order: &FleetOrder,
    queue: Queueing,
) -> Result<(), CommandRejection> {
    use CommandRejection as R;
    let fleet = state
        .entities
        .get::<Fleet>(id.sim_id())
        .ok_or(R::UnknownFleet)?;
    if fleet.owner != issuer {
        return Err(R::NotOwner);
    }
    // Rules of engagement apply at once, wherever the fleet is.
    if let FleetOrder::SetRulesOfEngagement { .. } = order {
        return Ok(());
    }
    let ctx = order_context(state, id, &fleet, queue).ok_or(R::UnknownFleet)?;
    match order {
        FleetOrder::Launch => {
            return if ctx.docked.is_some() {
                Ok(())
            } else {
                Err(R::NotDocked)
            };
        }
        _ if ctx.docked.is_some() => return Err(R::Docked),
        _ => {}
    }
    match order {
        FleetOrder::Move { to } => check_point(state, ctx, to),
        FleetOrder::AttackMove { to } => {
            check_point(state, ctx, to)?;
            if is_armed(state, &fleet) {
                Ok(())
            } else {
                Err(R::NoWeapons)
            }
        }
        FleetOrder::Patrol { route } => {
            if route.is_empty() {
                return Err(R::EmptyRoute);
            }
            route.iter().try_for_each(|to| check_point(state, ctx, to))
        }
        FleetOrder::Attack { target } => {
            let t = check_target(state, id, ctx, *target)?;
            if t.owner == Some(issuer) {
                return Err(R::FriendlyTarget);
            }
            if is_armed(state, &fleet) {
                Ok(())
            } else {
                Err(R::NoWeapons)
            }
        }
        FleetOrder::Guard { object } => check_target(state, id, ctx, *object).map(drop),
        FleetOrder::StationKeep { relative_to } => match relative_to {
            Some(target) => check_target(state, id, ctx, *target).map(drop),
            None => Ok(()),
        },
        FleetOrder::Dock { at } => {
            if holding_system(state, *at).ok_or(R::UnknownHolding)? != ctx.system {
                return Err(R::WrongSystem);
            }
            dock_check(state, &fleet, *at)
        }
        FleetOrder::Retreat { to: Some(holding) } => {
            let system = holding_system(state, *holding).ok_or(R::UnknownHolding)?;
            dock_check(state, &fleet, *holding)?;
            let point = station::launch_point(state, *holding).ok_or(R::UnknownHolding)?;
            reachable(state, &fleet, ctx, RouteEnd::Point(system, point.into()))
        }
        FleetOrder::Retreat { to: None } => nearest_holding(state, &fleet, ctx)
            .map(drop)
            .ok_or(R::NoHolding),
        FleetOrder::Jump { system } => {
            state.galaxy.system(*system).ok_or(R::UnknownSystem)?;
            if *system == ctx.system {
                return Err(R::AlreadyThere);
            }
            reachable(state, &fleet, ctx, RouteEnd::System(*system))
        }
        FleetOrder::Launch | FleetOrder::SetRulesOfEngagement { .. } => Ok(()),
    }
}

/// A point the fleet is ordered to: in its system, finite, inside the system and outside
/// every obstacle footprint.
fn check_point(state: &UniverseState, ctx: Context, to: &Location) -> Result<(), CommandRejection> {
    use CommandRejection as R;
    let radius = system_radius_m(state, to.system).ok_or(R::UnknownSystem)?;
    if to.system != ctx.system {
        return Err(R::WrongSystem);
    }
    if !to.position.is_finite() || distance_m(to.position, PositionMetres::ORIGIN) > radius {
        return Err(R::InvalidPosition);
    }
    let p = Vec2::from(to.position);
    if station::obstacles_in(state, to.system)
        .iter()
        .any(|o| (p - o.centre).length() < o.radius_m)
    {
        return Err(R::Unreachable);
    }
    Ok(())
}

/// A target or object in the fleet's system that is not the fleet itself or one of its
/// ships.
fn check_target(
    state: &UniverseState,
    fleet: FleetId,
    ctx: Context,
    target: SimId,
) -> Result<Target, CommandRejection> {
    use CommandRejection as R;
    let t = locate(state, target).ok_or(R::UnknownTarget)?;
    if t.fleet == Some(fleet) {
        return Err(R::SelfTarget);
    }
    if t.system != ctx.system {
        return Err(R::WrongSystem);
    }
    Ok(t)
}

fn reachable(
    state: &UniverseState,
    fleet: &Fleet,
    ctx: Context,
    end: RouteEnd,
) -> Result<(), CommandRejection> {
    let spool = Ticks::from_secs(state.rules.gates.spool_up_s);
    route::plan(
        &state.galaxy,
        ctx.system,
        ctx.position.unwrap_or(Vec2::ZERO),
        end,
        fleet_cruise_speed(state, fleet),
        spool,
    )
    .map(drop)
    .ok_or(CommandRejection::Unreachable)
}

fn validate_holding_order(
    state: &UniverseState,
    issuer: Owner,
    holding: HoldingId,
    order: HoldingOrder,
) -> Result<(), CommandRejection> {
    let h = state
        .holdings
        .get(&holding)
        .ok_or(CommandRejection::UnknownHolding)?;
    if Owner::Commander(h.owner) != issuer {
        return Err(CommandRejection::NotOwner);
    }
    match order {
        HoldingOrder::LaunchReady => {
            if docked_fleets(state, issuer, holding).is_empty() {
                Err(CommandRejection::NothingToLaunch)
            } else {
                Ok(())
            }
        }
        // Station orders (S5b).
        HoldingOrder::Ability { .. } | HoldingOrder::TurretsHold { .. } => {
            crate::defence::validate(state, holding, order)
        }
    }
}

/// Where a ship's fleet is.
fn ship_place(
    state: &UniverseState,
    issuer: Owner,
    ship: ShipId,
) -> Result<Place, CommandRejection> {
    let s = state
        .entities
        .get::<Ship>(ship.sim_id())
        .ok_or(CommandRejection::UnknownShip)?;
    if s.owner != issuer {
        return Err(CommandRejection::NotOwner);
    }
    state
        .entities
        .with::<Fleet, _>(s.fleet.sim_id(), |f| f.place)
        .ok_or(CommandRejection::UnknownShip)
}

/// Two places count as together if they are one system in space or one holding.
fn together(a: Place, b: Place) -> Result<(), CommandRejection> {
    match (a, b) {
        (Place::InTransit { .. }, _) | (_, Place::InTransit { .. }) => {
            Err(CommandRejection::InTransit)
        }
        (Place::InSystem(x), Place::InSystem(y)) if x == y => Ok(()),
        (Place::Docked(x), Place::Docked(y)) if x == y => Ok(()),
        _ => Err(CommandRejection::NotTogether),
    }
}

fn validate_create(
    state: &UniverseState,
    issuer: Owner,
    ships: &[ShipId],
) -> Result<(), CommandRejection> {
    let (&first, rest) = ships.split_first().ok_or(CommandRejection::EmptyFleet)?;
    let mut seen = std::collections::BTreeSet::new();
    if !ships.iter().all(|s| seen.insert(*s)) {
        return Err(CommandRejection::DuplicateShip);
    }
    let place = ship_place(state, issuer, first)?;
    together(place, place)?;
    for &ship in rest {
        together(place, ship_place(state, issuer, ship)?)?;
    }
    Ok(())
}

fn validate_merge(
    state: &UniverseState,
    issuer: Owner,
    fleet: FleetId,
    into: FleetId,
) -> Result<(), CommandRejection> {
    if fleet == into {
        return Err(CommandRejection::SelfTarget);
    }
    let place = |id: FleetId| {
        let (owner, place) = state
            .entities
            .with::<Fleet, _>(id.sim_id(), |f| (f.owner, f.place))
            .ok_or(CommandRejection::UnknownFleet)?;
        if owner == issuer {
            Ok(place)
        } else {
            Err(CommandRejection::NotOwner)
        }
    };
    together(place(fleet)?, place(into)?)
}

#[cfg(test)]
pub(crate) mod tests;
