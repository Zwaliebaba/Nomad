//! Fleet orders: the queue, starting orders, and what each order does every tick
//! (doc 15 §3.4; S3 plan §4.4).
//!
//! | Order | S3 behaviour |
//! |---|---|
//! | Move | To a point in the fleet's system |
//! | Attack-move | As Move, but pauses while a hostile is within weapon range; fires as under weapons free (S5) |
//! | Patrol | Loop through the waypoints |
//! | Guard | Stay within the guard radius of the object, following it |
//! | Station-keep | Hold the position, or the present offset from a target, following it |
//! | Attack | Close to weapon range of the target, hold and fire at it; completes when it is destroyed in sight (S5). A holding's dome is a circle: the fleet closes to weapon range of its edge (S5b). A ship or fleet that goes out of detection is followed to where it was last seen, where the fleet holds and the order completes with `TargetLost` unless it is detected again on the way (D-180) |
//! | Dock | Approach the holding's launch point and dock, if there is room (D-102) |
//! | Launch | Undock at once, at the holding's launch point |
//! | Retreat | Route to the nearest (or the given) owned holding by time, and dock |
//! | Jump | Route over lanes: gate, 5 s spool-up, transit, arrival with a 10 s shroud (D-055) |
//! | Rules of engagement | Stored on the fleet at once, whatever the queue (S5: who fires) |
//!
//! An order that cannot start when its turn comes, because things changed since it was
//! validated, is dropped with a [`FleetEvent::OrderDropped`]. So is an order whose target
//! disappears.
//!
//! **Fog of war (S7):** orders follow only what their commander sees. An order on another
//! owner's ship, fleet or module starts only while it is in the picture of the last sensors
//! pass; Guard and station-keeping on such a target are dropped when it goes out of view, as
//! when it leaves the system (*Derived* from S3's rule and doc 16: following its true
//! position would use information the commander does not have); Attack follows D-180.

use std::collections::VecDeque;

use nomad_types::{FleetOrder, PositionMetres, Queueing, RulesOfEngagement};
use tracing::debug;

use crate::combat::hostile_ship_within;
use crate::command::{Context, dock_check, fleet_cruise_speed, locate, nearest_holding};
use crate::fleet::route::{self, RouteEnd};
use crate::fleet::{
    ActiveOrder, Fleet, FleetId, Hop, Motion, Place, Ship, Task, TravelStage, slot_position,
    station,
};
use crate::galaxy::{GateId, LaneId, SystemId};
use crate::geometry::{Vec2, distance_m};
use crate::holding::HoldingId;
use crate::state::UniverseState;
use crate::time::{Tick, Ticks};

/// Something that happened to a fleet during a tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FleetEvent {
    /// It undocked at a holding.
    Launched {
        /// The fleet.
        fleet: FleetId,
        /// The holding.
        holding: HoldingId,
    },
    /// It docked at a holding.
    Docked {
        /// The fleet.
        fleet: FleetId,
        /// The holding.
        holding: HoldingId,
    },
    /// Its active order finished.
    OrderCompleted {
        /// The fleet.
        fleet: FleetId,
    },
    /// An Attack ended where its target was last seen, without seeing it again (D-180).
    TargetLost {
        /// The fleet.
        fleet: FleetId,
    },
    /// An order could not start, or lost its target or its docking room.
    OrderDropped {
        /// The fleet.
        fleet: FleetId,
    },
    /// Every ship is within jump range of the gate; the spool-up runs until `until`.
    SpoolUpStarted {
        /// The fleet.
        fleet: FleetId,
        /// The departure gate.
        gate: GateId,
        /// When it enters the lane.
        until: Tick,
    },
    /// A ship moved out of jump range during the spool-up, or the jump was replaced.
    SpoolUpCancelled {
        /// The fleet.
        fleet: FleetId,
    },
    /// It entered a lane; it arrives at `arrive` (D-042).
    Departed {
        /// The fleet.
        fleet: FleetId,
        /// The lane.
        lane: LaneId,
        /// When it arrives.
        arrive: Tick,
    },
    /// It arrived at a gate and is shrouded (D-055).
    Arrived {
        /// The fleet.
        fleet: FleetId,
        /// The system it arrived in.
        system: SystemId,
        /// The arrival gate.
        gate: GateId,
    },
}

/// What a fleet steers its anchor towards this tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Goal {
    /// Come to rest at this point.
    Point(Vec2),
    /// Come to rest where it is.
    Stop,
}

/// Gives a validated `order` to `fleet`.
pub(crate) fn give(
    state: &mut UniverseState,
    id: FleetId,
    order: FleetOrder,
    queue: Queueing,
    events: &mut Vec<FleetEvent>,
) {
    let Some(fleet) = state.entities.get_mut::<Fleet>(id.sim_id()) else {
        return;
    };
    if let FleetOrder::SetRulesOfEngagement { rules } = order {
        fleet.rules_of_engagement = rules;
        return;
    }
    if queue == Queueing::Replace {
        fleet.queue.clear();
        let in_transit = matches!(fleet.place, Place::InTransit { .. });
        match &mut fleet.active {
            // Transit cannot be interrupted (D-042): the fleet finishes the hop it is on.
            Some(ActiveOrder {
                task: Task::Travel {
                    hops, then_dock, ..
                },
                ..
            }) if in_transit => {
                hops.truncate(1);
                *then_dock = None;
            }
            Some(ActiveOrder {
                task:
                    Task::Travel {
                        stage: TravelStage::SpoolUp { .. },
                        ..
                    },
                ..
            }) => {
                fleet.active = None;
                events.push(FleetEvent::SpoolUpCancelled { fleet: id });
            }
            _ => fleet.active = None,
        }
    }
    fleet.queue.push_back(order);
    start_next(state, id, events);
}

/// Starts queued orders while `fleet` has no active order. Orders that take no time, such as
/// Launch, run at once and the next one starts.
pub(crate) fn start_next(state: &mut UniverseState, id: FleetId, events: &mut Vec<FleetEvent>) {
    loop {
        let Some(fleet) = state.entities.get::<Fleet>(id.sim_id()) else {
            return;
        };
        if fleet.active.is_some() {
            return;
        }
        let Some(order) = fleet.queue.front().cloned() else {
            return;
        };
        if let Some(f) = state.entities.get_mut::<Fleet>(id.sim_id()) {
            f.queue.pop_front();
        }
        match activate(state, id, &fleet, &order) {
            Start::Task(task) => {
                if let Some(f) = state.entities.get_mut::<Fleet>(id.sim_id()) {
                    f.active = Some(ActiveOrder { order, task });
                }
                return;
            }
            Start::Launch(holding) => launch_at(state, id, holding, events),
            Start::Dropped => {
                debug!(fleet = ?id, ?order, "order could not start");
                events.push(FleetEvent::OrderDropped { fleet: id });
            }
        }
    }
}

enum Start {
    Task(Task),
    Launch(HoldingId),
    Dropped,
}

/// Turns `order` into a task for `fleet` as it is now.
fn activate(state: &UniverseState, id: FleetId, fleet: &Fleet, order: &FleetOrder) -> Start {
    let anchor = state
        .entities
        .get::<Motion>(id.sim_id())
        .map_or(Vec2::ZERO, |m| m.position.into());
    let system = match fleet.place {
        Place::InSystem(system) => system,
        Place::Docked(holding) => {
            return match order {
                FleetOrder::Launch => Start::Launch(holding),
                _ => Start::Dropped,
            };
        }
        Place::InTransit { .. } | Place::EmergencyJump { .. } => return Start::Dropped,
    };
    let here = |loc: &nomad_types::Location| loc.system == system;
    // Only what the commander sees can be followed (S7).
    let target_here = |target| {
        locate(state, target).filter(|t| {
            t.system == system
                && t.fleet != Some(id)
                && crate::sensors::sees(state, fleet.owner, system, target)
        })
    };
    let task = match order {
        FleetOrder::Move { to } | FleetOrder::AttackMove { to } if here(to) => {
            Task::MoveTo { to: to.position }
        }
        FleetOrder::Patrol { route } if !route.is_empty() && route.iter().all(here) => {
            Task::Patrol {
                route: route.iter().map(|l| l.position).collect(),
                next: 0,
            }
        }
        FleetOrder::Guard { object } if target_here(*object).is_some() => {
            Task::Guard { object: *object }
        }
        FleetOrder::StationKeep { relative_to: None } => Task::StationKeep { at: anchor.into() },
        FleetOrder::StationKeep {
            relative_to: Some(target),
        } => match target_here(*target) {
            Some(t) => Task::KeepStationOn {
                target: *target,
                offset: anchor - t.position,
            },
            None => return Start::Dropped,
        },
        FleetOrder::Attack { target } => match target_here(*target) {
            Some(t) => Task::Attack {
                target: *target,
                seen: t.fleet.map(|_| t.position.into()),
            },
            None => return Start::Dropped,
        },
        FleetOrder::Dock { at } if holding_system(state, *at) == Some(system) => {
            Task::Dock { holding: *at }
        }
        FleetOrder::Retreat { to } => match retreat(state, fleet, system, anchor, *to) {
            Some(task) => task,
            None => return Start::Dropped,
        },
        FleetOrder::Jump { system: dest } if *dest != system => {
            match plan(state, fleet, system, anchor, RouteEnd::System(*dest)) {
                Some(hops) if !hops.is_empty() => Task::Travel {
                    hops,
                    stage: TravelStage::Approach,
                    then_dock: None,
                },
                _ => return Start::Dropped,
            }
        }
        _ => return Start::Dropped,
    };
    Start::Task(task)
}

/// A Retreat's task: dock at `to`, or the nearest own holding that can take the fleet, by
/// route time (S3 plan §4.4); `None` if there is none.
fn retreat(
    state: &UniverseState,
    fleet: &Fleet,
    system: SystemId,
    anchor: Vec2,
    to: Option<HoldingId>,
) -> Option<Task> {
    let ctx = Context {
        system,
        docked: None,
        position: Some(anchor),
    };
    let holding = to.or_else(|| nearest_holding(state, fleet, ctx).map(|(h, _)| h))?;
    dock_check(state, fleet, holding).ok()?;
    let dest = holding_system(state, holding)?;
    let point = station::launch_point(state, holding)?;
    let hops = plan(
        state,
        fleet,
        system,
        anchor,
        RouteEnd::Point(dest, point.into()),
    )?;
    Some(if hops.is_empty() {
        Task::Dock { holding }
    } else {
        Task::Travel {
            hops,
            stage: TravelStage::Approach,
            then_dock: Some(holding),
        }
    })
}

fn plan(
    state: &UniverseState,
    fleet: &Fleet,
    from: SystemId,
    start: Vec2,
    end: RouteEnd,
) -> Option<VecDeque<Hop>> {
    route::plan(
        &state.galaxy,
        from,
        start,
        end,
        fleet_cruise_speed(state, fleet),
        Ticks::from_secs(state.rules.gates.spool_up_s),
    )
    .map(|r| r.hops)
}

fn holding_system(state: &UniverseState, holding: HoldingId) -> Option<SystemId> {
    let site = state.holdings.get(&holding)?.site;
    Some(state.galaxy.site(site)?.system)
}

/// Launches a docked `fleet` now, for the LAUNCH button (doc 15 §5.3).
pub(crate) fn launch(state: &mut UniverseState, id: FleetId, events: &mut Vec<FleetEvent>) {
    let Some(Place::Docked(holding)) = state.entities.with::<Fleet, _>(id.sim_id(), |f| f.place)
    else {
        return;
    };
    launch_at(state, id, holding, events);
}

/// Puts a docked fleet at `holding`'s launch point, in formation, at rest, facing away from
/// the station.
fn launch_at(
    state: &mut UniverseState,
    id: FleetId,
    holding: HoldingId,
    events: &mut Vec<FleetEvent>,
) {
    let (Some(system), Some(point)) = (
        holding_system(state, holding),
        station::launch_point(state, holding),
    ) else {
        return;
    };
    let site = state
        .holdings
        .get(&holding)
        .and_then(|h| state.galaxy.site(h.site))
        .map_or(Vec2::ZERO, |s| s.position.into());
    let heading_rad = (Vec2::from(point) - site).angle();
    let anchor = Motion {
        position: point,
        velocity: Vec2::ZERO,
        heading_rad,
    };
    let Some(fleet) = state.entities.get_mut::<Fleet>(id.sim_id()) else {
        return;
    };
    fleet.place = Place::InSystem(system);
    let members: Vec<(crate::fleet::ShipId, Vec2)> =
        fleet.members.iter().map(|(&s, &o)| (s, o)).collect();
    for (ship, offset) in members {
        if let Some(m) = state.entities.get_mut::<Motion>(ship.sim_id()) {
            *m = Motion {
                position: slot_position(&anchor, offset).into(),
                ..anchor
            };
        }
    }
    if let Some(m) = state.entities.get_mut::<Motion>(id.sim_id()) {
        *m = anchor;
    }
    events.push(FleetEvent::Launched { fleet: id, holding });
}

/// What a fleet does with its task this tick, decided from the state at the start of the
/// movement phase.
pub(crate) struct Decision {
    pub(crate) goal: Goal,
    pub(crate) change: Change,
}

/// How a fleet's task changes this tick.
pub(crate) enum Change {
    None,
    /// The task moves on (a patrol's next waypoint, a spool-up starting or cancelled).
    Update(Task),
    /// The order is done.
    Complete,
    /// The order lost its target.
    Drop,
    /// An Attack reached where its target was last seen (D-180).
    TargetLost,
    /// Dock at this holding now.
    DockNow(HoldingId),
    /// Enter the lane of the first hop now.
    Depart,
}

/// Whether the anchor has come to rest at `point`.
fn arrived(anchor: &Motion, point: Vec2) -> bool {
    (Vec2::from(anchor.position) - point).length() < 0.5 && anchor.velocity.length() < 0.5
}

/// The shortest weapon range among `fleet`'s armed ships, so every armed ship can fire from
/// where the fleet holds (S3: "close to the class weapon range").
fn engagement_range_m(state: &UniverseState, fleet: &Fleet) -> Option<f64> {
    fleet
        .members
        .keys()
        .filter_map(|s| state.entities.get::<Ship>(s.sim_id()))
        .filter_map(|s| state.rules.class(s.class).weapon_range_m)
        .reduce(f64::min)
}

/// Decides `fleet`'s goal and task change for this tick.
pub(crate) fn decide(state: &UniverseState, id: FleetId, fleet: &Fleet, now: Tick) -> Decision {
    let anchor = state.entities.get::<Motion>(id.sim_id()).unwrap_or(Motion {
        position: PositionMetres::ORIGIN,
        velocity: Vec2::ZERO,
        heading_rad: 0.0,
    });
    let here = Vec2::from(anchor.position);
    let Place::InSystem(system) = fleet.place else {
        return Decision {
            goal: Goal::Stop,
            change: Change::None,
        };
    };
    let Some(active) = &fleet.active else {
        return Decision {
            goal: Goal::Stop,
            change: Change::None,
        };
    };
    // Only what the commander sees is followed (S7).
    let track = |target| {
        locate(state, target).filter(|t| {
            t.system == system && crate::sensors::sees(state, fleet.owner, system, target)
        })
    };
    let keep = |goal| Decision {
        goal,
        change: Change::None,
    };
    match &active.task {
        Task::MoveTo { .. } if attack_move_pauses(state, fleet, &active.order, system, here) => {
            keep(Goal::Stop)
        }
        Task::MoveTo { to } => {
            let to = Vec2::from(*to);
            Decision {
                goal: Goal::Point(to),
                change: if arrived(&anchor, to) {
                    Change::Complete
                } else {
                    Change::None
                },
            }
        }
        Task::Patrol { route, next } => {
            let point = route.get(*next).copied().map_or(here, Vec2::from);
            if arrived(&anchor, point) && route.len() > 1 {
                let next = (next + 1) % route.len();
                Decision {
                    goal: Goal::Point(route.get(next).copied().map_or(here, Vec2::from)),
                    change: Change::Update(Task::Patrol {
                        route: route.clone(),
                        next,
                    }),
                }
            } else {
                keep(Goal::Point(point))
            }
        }
        Task::StationKeep { at } => keep(Goal::Point((*at).into())),
        Task::KeepStationOn { target, offset } => match track(*target) {
            Some(t) => keep(Goal::Point(t.position + *offset)),
            None => drop_order(),
        },
        Task::Guard { object } => match track(*object) {
            Some(t) => {
                let radius = state.rules.movement.guard_radius_m;
                keep(hold_within(here, t.position, radius, 0.5))
            }
            None => drop_order(),
        },
        Task::Attack { target, seen } => decide_attack(state, fleet, *target, *seen, system, here),
        Task::Pursue { target, to } => decide_pursuit(state, fleet, *target, *to, system, &anchor),
        Task::Dock { holding } => match station::launch_point(state, *holding) {
            Some(point) if holding_system(state, *holding) == Some(system) => {
                let point = Vec2::from(point);
                Decision {
                    goal: Goal::Point(point),
                    change: if arrived(&anchor, point) {
                        Change::DockNow(*holding)
                    } else {
                        Change::None
                    },
                }
            }
            _ => drop_order(),
        },
        Task::Travel {
            hops,
            stage,
            then_dock,
        } => decide_travel(state, fleet, hops, *stage, *then_dock, now),
    }
}

/// Whether an Attack-move pauses this tick: it fights what it meets, so it holds while a
/// hostile is within the fleet's engagement range (S5 plan §4.2), unless it holds fire.
fn attack_move_pauses(
    state: &UniverseState,
    fleet: &Fleet,
    order: &FleetOrder,
    system: SystemId,
    here: Vec2,
) -> bool {
    matches!(order, FleetOrder::AttackMove { .. })
        && fleet.rules_of_engagement != RulesOfEngagement::HoldFire
        && engagement_range_m(state, fleet)
            .is_some_and(|range| hostile_ship_within(state, system, fleet.owner, here, range))
}

/// An Attack: close to weapon range of the target and hold; combat fires at it (S5). For a
/// holding with a dome, weapon range of the dome's edge (S5b): the dome is what is hit, and
/// strike craft breaching from their full range stay out of reach of the turrets inside
/// (S5 plan §5).
///
/// A ship or fleet (`seen` is set) is followed while the commander detects it, and `seen`
/// keeps up with it. One that is gone while still in the picture was destroyed in sight: the
/// attack succeeded. One that is out of the picture, or no longer in space here (it docked
/// or jumped), is lost: the fleet goes to where it was last seen (D-180). A structure does not
/// move: it is followed where it stands, and the attack succeeds when it is gone (S5b).
fn decide_attack(
    state: &UniverseState,
    fleet: &Fleet,
    target: crate::id::SimId,
    seen: Option<PositionMetres>,
    system: SystemId,
    here: Vec2,
) -> Decision {
    let detected = seen.is_none() || crate::sensors::sees(state, fleet.owner, system, target);
    let tracked = locate(state, target).filter(|t| detected && t.system == system);
    let range = engagement_range_m(state, fleet);
    if let (Some(t), Some(range)) = (tracked, range) {
        let edge = crate::defence::attack_edge_m(state, target);
        let now_seen = t.fleet.map(|_| PositionMetres::from(t.position));
        return Decision {
            goal: hold_within_edge(here, t.position, edge, range, 0.9),
            change: if now_seen == seen {
                Change::None
            } else {
                Change::Update(Task::Attack {
                    target,
                    seen: now_seen,
                })
            },
        };
    }
    if detected && gone(state, target) {
        return Decision {
            goal: Goal::Stop,
            change: Change::Complete,
        };
    }
    match (seen, range) {
        (Some(to), Some(_)) => Decision {
            goal: Goal::Point(to.into()),
            change: Change::Update(Task::Pursue { target, to }),
        },
        _ => drop_order(),
    }
}

/// Whether `target` no longer exists: a destroyed ship, fleet or module.
fn gone(state: &UniverseState, target: crate::id::SimId) -> bool {
    !state.entities.contains(target) && !state.holdings.contains_key(&HoldingId(target))
}

/// An Attack whose target was lost (D-180): resume it if the target is detected again on the
/// way, succeed if it was destroyed in sight, else go to where it was last seen and end there
/// with `TargetLost`. The fleet then holds, unless another order is queued.
fn decide_pursuit(
    state: &UniverseState,
    fleet: &Fleet,
    target: crate::id::SimId,
    to: PositionMetres,
    system: SystemId,
    anchor: &Motion,
) -> Decision {
    if crate::sensors::sees(state, fleet.owner, system, target) {
        if let Some(t) = locate(state, target).filter(|t| t.system == system) {
            return Decision {
                goal: Goal::Point(t.position),
                change: Change::Update(Task::Attack {
                    target,
                    seen: Some(t.position.into()),
                }),
            };
        }
        if gone(state, target) {
            return Decision {
                goal: Goal::Stop,
                change: Change::Complete,
            };
        }
    }
    let point = Vec2::from(to);
    Decision {
        goal: Goal::Point(point),
        change: if arrived(anchor, point) {
            Change::TargetLost
        } else {
            Change::None
        },
    }
}

/// A route's current hop (S3 plan §4.4, D-055): approach the gate until every ship is within
/// jump range; spool up for 5 s, cancelled if a ship leaves that range; then enter the lane.
fn decide_travel(
    state: &UniverseState,
    fleet: &Fleet,
    hops: &VecDeque<Hop>,
    stage: TravelStage,
    then_dock: Option<HoldingId>,
    now: Tick,
) -> Decision {
    let Some(hop) = hops.front() else {
        return Decision {
            goal: Goal::Stop,
            change: match then_dock {
                Some(holding) => Change::Update(Task::Dock { holding }),
                None => Change::Complete,
            },
        };
    };
    let Some(gate) = state.galaxy.gate(hop.from_gate) else {
        return drop_order();
    };
    let range = state.rules.gates.jump_range_m;
    let all_in_range = fleet.members.keys().all(|s| {
        state
            .entities
            .get::<Motion>(s.sim_id())
            .is_some_and(|m| distance_m(m.position, gate.position) <= range)
    });
    let travel = |stage| Task::Travel {
        hops: hops.clone(),
        stage,
        then_dock,
    };
    let change = match stage {
        TravelStage::Approach if all_in_range => {
            let spool = Ticks::from_secs(state.rules.gates.spool_up_s);
            Change::Update(travel(TravelStage::SpoolUp {
                started: now,
                until: now.after(spool),
            }))
        }
        TravelStage::SpoolUp { .. } if !all_in_range => {
            Change::Update(travel(TravelStage::Approach))
        }
        TravelStage::SpoolUp { until, .. } if now >= until => Change::Depart,
        TravelStage::Approach | TravelStage::SpoolUp { .. } | TravelStage::Transit => Change::None,
    };
    Decision {
        goal: Goal::Point(gate.position.into()),
        change,
    }
}

fn drop_order() -> Decision {
    Decision {
        goal: Goal::Stop,
        change: Change::Drop,
    }
}

/// Stay within `radius` of `target`: if farther, close to `fraction × radius` along the line
/// from the target; otherwise come to rest.
/// As [`hold_within`], for a target that is a circle of radius `edge`: stop within `range`
/// of its edge, or head for `fraction` of the range outside it. With no edge it is
/// [`hold_within`].
fn hold_within_edge(here: Vec2, target: Vec2, edge: f64, range: f64, fraction: f64) -> Goal {
    let away = here - target;
    if away.length() <= edge + range {
        Goal::Stop
    } else {
        let direction = away.normalized().unwrap_or(Vec2::new(1.0, 0.0));
        Goal::Point(target + direction * (edge + range * fraction))
    }
}

fn hold_within(here: Vec2, target: Vec2, radius: f64, fraction: f64) -> Goal {
    let away = here - target;
    if away.length() <= radius {
        Goal::Stop
    } else {
        let direction = away.normalized().unwrap_or(Vec2::new(1.0, 0.0));
        Goal::Point(target + direction * (radius * fraction))
    }
}

/// Applies `change` to `fleet` on tick `now`.
pub(crate) fn apply(
    state: &mut UniverseState,
    id: FleetId,
    change: Change,
    now: Tick,
    events: &mut Vec<FleetEvent>,
) {
    match change {
        Change::None => {}
        Change::Update(task) => {
            let Some(fleet) = state.entities.get_mut::<Fleet>(id.sim_id()) else {
                return;
            };
            let Some(active) = &mut fleet.active else {
                return;
            };
            match (&active.task, &task) {
                (
                    Task::Travel {
                        stage: TravelStage::Approach,
                        hops,
                        ..
                    },
                    Task::Travel {
                        stage: TravelStage::SpoolUp { until, .. },
                        ..
                    },
                ) => {
                    if let Some(hop) = hops.front() {
                        events.push(FleetEvent::SpoolUpStarted {
                            fleet: id,
                            gate: hop.from_gate,
                            until: *until,
                        });
                    }
                }
                (
                    Task::Travel {
                        stage: TravelStage::SpoolUp { .. },
                        ..
                    },
                    Task::Travel {
                        stage: TravelStage::Approach,
                        ..
                    },
                ) => events.push(FleetEvent::SpoolUpCancelled { fleet: id }),
                _ => {}
            }
            active.task = task;
        }
        Change::Complete => finish(state, id, FleetEvent::OrderCompleted { fleet: id }, events),
        Change::Drop => finish(state, id, FleetEvent::OrderDropped { fleet: id }, events),
        Change::TargetLost => finish(state, id, FleetEvent::TargetLost { fleet: id }, events),
        Change::DockNow(holding) => {
            let Some(fleet) = state.entities.get::<Fleet>(id.sim_id()) else {
                return;
            };
            if dock_check(state, &fleet, holding).is_ok() {
                if let Some(f) = state.entities.get_mut::<Fleet>(id.sim_id()) {
                    f.place = Place::Docked(holding);
                }
                if let Some(m) = state.entities.get_mut::<Motion>(id.sim_id()) {
                    m.velocity = Vec2::ZERO;
                }
                events.push(FleetEvent::Docked { fleet: id, holding });
                finish(state, id, FleetEvent::OrderCompleted { fleet: id }, events);
            } else {
                debug!(fleet = ?id, ?holding, "no docking room on arrival");
                finish(state, id, FleetEvent::OrderDropped { fleet: id }, events);
            }
        }
        Change::Depart => depart(state, id, now, events),
    }
}

/// Ends the active order with `event` and starts the next.
fn finish(state: &mut UniverseState, id: FleetId, event: FleetEvent, events: &mut Vec<FleetEvent>) {
    if let Some(f) = state.entities.get_mut::<Fleet>(id.sim_id()) {
        f.active = None;
    }
    events.push(event);
    start_next(state, id, events);
}

/// Enters the lane of the first hop: the fleet becomes a timer (D-042).
fn depart(state: &mut UniverseState, id: FleetId, now: Tick, events: &mut Vec<FleetEvent>) {
    let Some(hop) = state
        .entities
        .with::<Fleet, _>(id.sim_id(), |f| match &f.active {
            Some(ActiveOrder {
                task: Task::Travel { hops, .. },
                ..
            }) => hops.front().copied(),
            _ => None,
        })
    else {
        return;
    };
    let Some(hop) = hop else { return };
    let Some(transit) = state.galaxy.lane(hop.lane).map(|l| l.transit) else {
        return;
    };
    let arrive = now.after(transit);
    if let Some(f) = state.entities.get_mut::<Fleet>(id.sim_id()) {
        f.place = Place::InTransit {
            lane: hop.lane,
            to_gate: hop.to_gate,
            arrive,
        };
        if let Some(ActiveOrder {
            task: Task::Travel { stage, .. },
            ..
        }) = &mut f.active
        {
            *stage = TravelStage::Transit;
        }
    }
    state.transits.schedule(&mut state.ids, arrive, id);
    events.push(FleetEvent::Departed {
        fleet: id,
        lane: hop.lane,
        arrive,
    });
}
