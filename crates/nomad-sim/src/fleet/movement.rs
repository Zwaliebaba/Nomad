//! Phase 5 of the tick: movement and transit (Design doc 11 §4.1; doc 15 §3.3; S3 plan
//! §4.3).
//!
//! Every tick, Δt = 0.1 s:
//!
//! 1. **Transit arrivals** from the jump-transit queue: each ship appears within 2 km of
//!    the destination gate, scattered by the system's seeded stream, with a 10 s jump
//!    shroud (D-055). Expired shrouds end.
//! 2. **Orders** decide each fleet's goal from the state at the start of the phase, then
//!    their task changes apply (dock, depart, next waypoint, next order).
//! 3. **Kinematics**, fleet by fleet in identity order:
//!    - the **anchor** seeks and arrives at the goal at the fleet's limits: its slowest
//!      member's top speed in the current mode, its lowest acceleration and turn rate
//!      (doc 15 §3.4). It brakes on the exact discrete stopping curve, so it comes to rest on
//!      the goal without overshooting;
//!    - each **ship** steers to its formation slot, the anchor plus its offset rotated with
//!      the fleet's heading, following the anchor's velocity and closing the gap at its own
//!      limits; its velocity change is capped by its acceleration and its heading turns
//!      towards its velocity at its turn rate (D-101);
//!    - **obstacles** (D-105): look-ahead steering turns a ship onto the tangent of the
//!      nearest obstacle in its way. Obstacles whose footprints overlap are steered around
//!      as one enclosing circle, so there are no pockets to get stuck in. A ship is never
//!      inside a footprint: if pushed in, it is put back on the edge;
//!    - **separation** (D-105): a ship closer to another than the separation distance aims
//!      for a point moved away from it by the overlap. It is soft: two ships with the same
//!      slot settle two thirds of the separation distance apart. Neighbours come from the
//!      positions at the start of the phase, through a per-system grid of 2 km cells.
//!
//! **Cruise mode** (doc 15 §3.3, D-103): a fleet whose anchor is outside every ops area and
//! that is not engaged moves at cruise speeds; otherwise at tactical speeds.
//!
//! Every quantity is computed from the state at the start of the phase or from the
//! fleet's own new anchor, never from another fleet's update this tick, so the result does
//! not depend on processing order beyond the fixed identity order (D-049).

use std::collections::BTreeMap;

use crate::fleet::orders::{self, FleetEvent, Goal};
use crate::fleet::station::{Obstacle, obstacles_in};
use crate::fleet::{
    Fleet, FleetId, Motion, MotionMode, Place, Ship, ShipId, Shroud, Task, TravelStage,
    slot_position,
};
use crate::galaxy::{PositionMetres, SystemId};
use crate::geometry::{Vec2, angle_difference};
use crate::rng::{StreamKey, purpose};
use crate::rules::Rules;
use crate::state::UniverseState;
use crate::time::{TICK_MILLIS, Tick, Ticks};

/// Seconds per tick.
pub(crate) const DT: f64 = TICK_MILLIS as f64 / 1_000.0;

/// Below this speed a heading does not follow the velocity, so a fleet at rest keeps its
/// formation's orientation.
const TURN_MIN_SPEED_M_PER_S: f64 = 1.0;

/// Runs phase 5 for tick `now`.
pub(crate) fn run(state: &mut UniverseState, now: Tick, events: &mut Vec<FleetEvent>) {
    arrivals(state, now, events);
    expire_shrouds(state, now);

    let in_space: Vec<(FleetId, Fleet)> = state
        .entities
        .ids_with::<Fleet>()
        .into_iter()
        .filter_map(|id| {
            let fleet = state.entities.get::<Fleet>(id)?;
            matches!(fleet.place, Place::InSystem(_)).then_some((FleetId(id), fleet))
        })
        .collect();
    let decisions: Vec<_> = in_space
        .iter()
        .map(|(id, fleet)| (*id, orders::decide(state, *id, fleet, now)))
        .collect();
    let mut goals = BTreeMap::new();
    for (id, decision) in decisions {
        goals.insert(id, decision.goal);
        orders::apply(state, id, decision.change, now, events);
    }
    kinematics(state, &goals);
}

/// Brings fleets whose transit ends now into their destination systems (D-042, D-055).
fn arrivals(state: &mut UniverseState, now: Tick, events: &mut Vec<FleetEvent>) {
    while let Some((_, _, id)) = state.transits.pop_due(now) {
        let Some(fleet) = state.entities.get::<Fleet>(id.sim_id()) else {
            continue;
        };
        let Place::InTransit { to_gate, .. } = fleet.place else {
            continue;
        };
        let Some(gate) = state.galaxy.gate(to_gate).cloned() else {
            continue;
        };
        let system = gate.system;
        let key = StreamKey {
            scope: system.sim_id().get(),
            purpose: purpose::ARRIVAL_SCATTER,
        };
        let scatter = state.rules.gates.arrival_scatter_m;
        let shroud = Shroud {
            until: now.after(Ticks::from_secs(state.rules.gates.shroud_s)),
        };
        let centre = Vec2::from(gate.position);
        let mut sum = Vec2::ZERO;
        for &ship in fleet.members.keys() {
            // Uniform over the disc: radius √u × scatter, so the density is even.
            let radius = libm::sqrt(state.rng.unit_f64(key)) * scatter;
            let angle = state.rng.unit_f64(key) * std::f64::consts::TAU;
            let position = centre + Vec2::from_angle(angle) * radius;
            sum += position;
            if let Some(m) = state.entities.get_mut::<Motion>(ship.sim_id()) {
                m.position = position.into();
                m.velocity = Vec2::ZERO;
            }
            state.entities.insert(ship.sim_id(), shroud);
        }
        let n = fleet.members.len().max(1) as f64;
        if let Some(m) = state.entities.get_mut::<Motion>(id.sim_id()) {
            m.position = (sum * (1.0 / n)).into();
            m.velocity = Vec2::ZERO;
        }
        let mut finished = false;
        if let Some(f) = state.entities.get_mut::<Fleet>(id.sim_id()) {
            f.place = Place::InSystem(system);
            if let Some(active) = &mut f.active {
                // The hop is done; the route continues, or ends here, or ends with docking.
                let route_end = match &mut active.task {
                    Task::Travel {
                        hops,
                        stage,
                        then_dock,
                    } => {
                        hops.pop_front();
                        *stage = TravelStage::Approach;
                        hops.is_empty().then_some(*then_dock)
                    }
                    _ => None,
                };
                match route_end {
                    Some(Some(holding)) => active.task = Task::Dock { holding },
                    Some(None) => finished = true,
                    None => {}
                }
            }
        }
        events.push(FleetEvent::Arrived {
            fleet: id,
            system,
            gate: to_gate,
        });
        if finished {
            orders::apply(state, id, orders::Change::Complete, now, events);
        }
    }
}

/// Ends jump shrouds whose deadline has come (D-055).
fn expire_shrouds(state: &mut UniverseState, now: Tick) {
    for id in state.entities.ids_with::<Shroud>() {
        if state
            .entities
            .get::<Shroud>(id)
            .is_some_and(|shroud| shroud.until <= now)
        {
            state.entities.remove::<Shroud>(id);
        }
    }
}

/// A fleet's or a ship's movement limits.
#[derive(Clone, Copy, Debug)]
struct Limits {
    speed_m_per_s: f64,
    acceleration_m_per_s2: f64,
    turn_rad_per_s: f64,
}

fn class_limits(rules: &Rules, class: crate::fleet::ShipClass, mode: MotionMode) -> Limits {
    let c = rules.class(class);
    Limits {
        speed_m_per_s: match mode {
            MotionMode::Cruise => c.cruise_speed_m_per_s,
            MotionMode::Tactical => c.tactical_speed_m_per_s,
        },
        acceleration_m_per_s2: c.acceleration_m_per_s2,
        turn_rad_per_s: c.turn_rate_rad_per_s(),
    }
}

/// The fleet's limits: its slowest member in each respect (doc 15 §3.4).
fn fleet_limits(members: &[Limits]) -> Limits {
    members.iter().fold(
        Limits {
            speed_m_per_s: f64::INFINITY,
            acceleration_m_per_s2: f64::INFINITY,
            turn_rad_per_s: f64::INFINITY,
        },
        |a, b| Limits {
            speed_m_per_s: a.speed_m_per_s.min(b.speed_m_per_s),
            acceleration_m_per_s2: a.acceleration_m_per_s2.min(b.acceleration_m_per_s2),
            turn_rad_per_s: a.turn_rad_per_s.min(b.turn_rad_per_s),
        },
    )
}

/// The highest speed from which braking at `acceleration` every tick stops within
/// `distance_m`, for the semi-implicit integration used here. A body at this speed brakes by
/// exactly one tick's acceleration per tick and comes to rest on the point.
fn stopping_speed(distance_m: f64, acceleration: f64) -> f64 {
    let half = acceleration * DT / 2.0;
    libm::sqrt(half * half + 2.0 * acceleration * distance_m) - half
}

/// The velocity that seeks `offset` away and arrives there at rest (S3 plan §4.3).
fn arrive(offset: Vec2, speed_m_per_s: f64, acceleration: f64) -> Vec2 {
    let distance = offset.length();
    if distance <= 1e-9 {
        return Vec2::ZERO;
    }
    let speed = speed_m_per_s
        .min(stopping_speed(distance, acceleration))
        .min(distance / DT);
    offset * (speed / distance)
}

/// Caps the part of `desired` that heads for the ship's resting slot, `to_rest` away, at the
/// speed from which the ship can still stop there. A ship that lags its slot closes the gap
/// while the anchor brakes, so without this cap it would overshoot (S3 plan §7: no more than
/// 1% of the leg).
fn within_stopping(desired: Vec2, to_rest: Vec2, limits: Limits) -> Vec2 {
    let distance = to_rest.length();
    let Some(towards) = to_rest.normalized() else {
        return desired;
    };
    let cap = stopping_speed(distance, limits.acceleration_m_per_s2).min(distance / DT);
    let along = desired.dot(towards);
    if along > cap {
        desired - towards * (along - cap)
    } else {
        desired
    }
}

/// Changes `motion`'s velocity towards `desired` by at most one tick's acceleration, moves it
/// and turns its heading towards its velocity by at most one tick's turn (D-101).
fn integrate(motion: &mut Motion, desired: Vec2, limits: Limits) {
    let change = (desired - motion.velocity).clamp_length(limits.acceleration_m_per_s2 * DT);
    motion.velocity += change;
    motion.position = (Vec2::from(motion.position) + motion.velocity * DT).into();
    if motion.velocity.length() > TURN_MIN_SPEED_M_PER_S {
        let turn = angle_difference(motion.heading_rad, motion.velocity.angle());
        let max = limits.turn_rad_per_s * DT;
        motion.heading_rad = angle_difference(0.0, motion.heading_rad + turn.clamp(-max, max));
    }
}

/// Obstacles of one system: footprints, and how steering sees them.
struct SystemObstacles {
    footprints: Vec<Obstacle>,
    steering: Vec<SteeringCircle>,
}

/// A group of footprints that lie close together, as steering sees it: one enclosing circle
/// plus the margin, and the members, each plus the margin, for ships already inside it.
struct SteeringCircle {
    outer: Obstacle,
    members: Vec<Obstacle>,
}

/// Groups footprints that come within `2 × margin` of each other (D-105). Steering avoids a
/// group as one enclosing circle, so there are no pockets between footprints to get stuck in.
fn steering_circles(footprints: &[Obstacle], margin: f64) -> Vec<SteeringCircle> {
    let n = footprints.len();
    let mut group: Vec<usize> = (0..n).collect();
    let find = |group: &mut Vec<usize>, mut i: usize| {
        while group[i] != i {
            group[i] = group[group[i]];
            i = group[i];
        }
        i
    };
    for i in 0..n {
        for j in i + 1..n {
            let (a, b) = (footprints[i], footprints[j]);
            if (a.centre - b.centre).length() < a.radius_m + b.radius_m + 2.0 * margin {
                let (ri, rj) = (find(&mut group, i), find(&mut group, j));
                group[ri.max(rj)] = ri.min(rj);
            }
        }
    }
    let grow = |c: Obstacle| Obstacle {
        radius_m: c.radius_m + margin,
        ..c
    };
    let mut circles: BTreeMap<usize, SteeringCircle> = BTreeMap::new();
    for (i, &next) in footprints.iter().enumerate() {
        let root = find(&mut group, i);
        circles
            .entry(root)
            .and_modify(|c| {
                c.outer = enclose(c.outer, grow(next));
                c.members.push(grow(next));
            })
            .or_insert(SteeringCircle {
                outer: grow(next),
                members: vec![grow(next)],
            });
    }
    circles.into_values().collect()
}

/// The smallest circle around two circles.
fn enclose(a: Obstacle, b: Obstacle) -> Obstacle {
    let between = b.centre - a.centre;
    let d = between.length();
    if d + b.radius_m <= a.radius_m {
        return a;
    }
    if d + a.radius_m <= b.radius_m {
        return b;
    }
    let radius_m = (d + a.radius_m + b.radius_m) / 2.0;
    let centre = a.centre + between * ((radius_m - a.radius_m) / d);
    Obstacle { centre, radius_m }
}

/// Steers `desired` round obstacles (D-105).
///
/// A group of footprints is avoided as its enclosing circle, unless the ship's resting slot
/// lies inside it; then its members are avoided one by one, so the ship can reach the slot.
/// Steering looks for the nearest such circle across the segment ahead, up to `reach`, and
/// turns onto its tangent. A ship that has strayed into an enclosing circle it should avoid
/// leaves along its edge. Footprints themselves are enforced separately ([`push_out`]).
fn avoid(desired: Vec2, position: Vec2, rest: Vec2, groups: &[SteeringCircle], reach: f64) -> Vec2 {
    let speed = desired.length();
    let Some(direction) = desired.normalized() else {
        return desired;
    };
    let contains = |c: &Obstacle, p: Vec2| (p - c.centre).length() < c.radius_m;
    for group in groups {
        if contains(&group.outer, position) && !contains(&group.outer, rest) {
            let Some(outwards) = (position - group.outer.centre).normalized() else {
                return desired;
            };
            if desired.dot(outwards) >= 0.0 {
                return desired;
            }
            let side = if outwards.cross(direction) >= 0.0 {
                std::f64::consts::FRAC_PI_2
            } else {
                -std::f64::consts::FRAC_PI_2
            };
            let along_edge = outwards.rotated(side) + outwards * 0.5;
            return along_edge.normalized().map_or(desired, |d| d * speed);
        }
    }
    let candidates = groups.iter().flat_map(|group| {
        if contains(&group.outer, rest) {
            group.members.as_slice()
        } else {
            std::slice::from_ref(&group.outer)
        }
    });
    let mut nearest: Option<(f64, Obstacle)> = None;
    for &circle in candidates {
        if contains(&circle, position) || contains(&circle, rest) {
            continue;
        }
        let to_centre = circle.centre - position;
        let distance = to_centre.length();
        let along = to_centre.dot(direction);
        let off_line = to_centre.cross(direction).abs();
        if along <= 0.0 || along - circle.radius_m > reach || off_line >= circle.radius_m {
            continue;
        }
        if nearest.is_none_or(|(d, _)| distance < d) {
            nearest = Some((distance, circle));
        }
    }
    let Some((distance, circle)) = nearest else {
        return desired;
    };
    let towards = (circle.centre - position) * (1.0 / distance);
    let half_angle = libm::asin((circle.radius_m / distance).min(1.0));
    let left = towards.rotated(half_angle);
    let right = towards.rotated(-half_angle);
    let tangent = if left.dot(direction) >= right.dot(direction) {
        left
    } else {
        right
    };
    tangent * speed
}

/// Puts a ship that is inside a footprint back on its edge and removes the velocity that
/// points inwards, so no ship is ever inside an obstacle (D-105). Where two footprints
/// overlap the edge of one can lie inside the other; if a few rounds do not settle it, the
/// ship stays where it was this tick, which was outside.
fn push_out(motion: &mut Motion, before: PositionMetres, footprints: &[Obstacle]) {
    let inside = |p: PositionMetres| {
        footprints
            .iter()
            .any(|o| (Vec2::from(p) - o.centre).length() < o.radius_m)
    };
    for _ in 0..8 {
        if !inside(motion.position) {
            return;
        }
        for obstacle in footprints {
            let from_centre = Vec2::from(motion.position) - obstacle.centre;
            if from_centre.length() >= obstacle.radius_m {
                continue;
            }
            let normal = from_centre.normalized().unwrap_or(Vec2::new(1.0, 0.0));
            // A hair beyond the edge, so rounding cannot leave the ship inside.
            motion.position = (obstacle.centre + normal * (obstacle.radius_m + 1e-6)).into();
            let inwards = motion.velocity.dot(normal);
            if inwards < 0.0 {
                motion.velocity -= normal * inwards;
            }
        }
    }
    if inside(motion.position) {
        motion.position = before;
        motion.velocity = Vec2::ZERO;
    }
}

/// A cell of the neighbour grid.
#[expect(
    clippy::cast_possible_truncation,
    reason = "positions lie within a system's radius, far inside i64 after division by the cell"
)]
fn cell(position: Vec2, cell_m: f64) -> (i64, i64) {
    (
        libm::floor(position.x / cell_m) as i64,
        libm::floor(position.y / cell_m) as i64,
    )
}

/// Ships in space by system and grid cell, at the start of the phase.
type NeighbourGrid = BTreeMap<(SystemId, (i64, i64)), Vec<(ShipId, Vec2)>>;

/// How far `ship` gives way to its neighbours (D-105): for each neighbour closer than the
/// separation distance, the overlap, away from it.
fn separation(
    grid: &NeighbourGrid,
    system: SystemId,
    ship: ShipId,
    position: Vec2,
    rules: &Rules,
) -> Vec2 {
    let separation_m = rules.movement.separation_m;
    let (cx, cy) = cell(position, rules.movement.neighbour_cell_m);
    let mut push = Vec2::ZERO;
    for x in cx.saturating_sub(1)..=cx.saturating_add(1) {
        for y in cy.saturating_sub(1)..=cy.saturating_add(1) {
            for &(other, at) in grid.get(&(system, (x, y))).into_iter().flatten() {
                if other == ship {
                    continue;
                }
                let away = position - at;
                let distance = away.length();
                if distance >= separation_m {
                    continue;
                }
                // Two ships on one point part along x, the lower identity eastwards.
                let direction = away.normalized().unwrap_or(if ship < other {
                    Vec2::new(1.0, 0.0)
                } else {
                    Vec2::new(-1.0, 0.0)
                });
                push += direction * (separation_m - distance);
            }
        }
    }
    push
}

/// Obstacles and the neighbour grid of every system with a fleet in space, from the state
/// at the start of the kinematics.
fn surroundings(
    state: &UniverseState,
    fleets: &[(FleetId, SystemId, Fleet)],
) -> (BTreeMap<SystemId, SystemObstacles>, NeighbourGrid) {
    let margin = state.rules.movement.obstacle_margin_m;
    let cell_m = state.rules.movement.neighbour_cell_m;
    let mut obstacles: BTreeMap<SystemId, SystemObstacles> = BTreeMap::new();
    let mut grid = NeighbourGrid::new();
    for (_, system, fleet) in fleets {
        obstacles.entry(*system).or_insert_with(|| {
            let footprints = obstacles_in(state, *system);
            let steering = steering_circles(&footprints, margin);
            SystemObstacles {
                footprints,
                steering,
            }
        });
        for &ship in fleet.members.keys() {
            if let Some(m) = state.entities.get::<Motion>(ship.sim_id()) {
                let p = Vec2::from(m.position);
                grid.entry((*system, cell(p, cell_m)))
                    .or_default()
                    .push((ship, p));
            }
        }
    }
    (obstacles, grid)
}

/// What a ship of a fleet steers by this tick.
struct Steering<'a> {
    /// The fleet's anchor after its move this tick.
    anchor: Motion,
    /// Where the anchor will come to rest.
    rest: Vec2,
    obstacles: &'a SystemObstacles,
    look_ahead_s: f64,
}

/// Moves one ship a tick: follow the anchor and close the gap to the slot at the ship's own
/// limits, steer round obstacles, never approach the resting slot faster than the ship can
/// stop there, and never end inside a footprint.
fn steer_ship(motion: &mut Motion, offset: Vec2, give_way: Vec2, limits: Limits, s: &Steering<'_>) {
    let before = motion.position;
    let position = Vec2::from(motion.position);
    let slot = slot_position(&s.anchor, offset) + give_way;
    let rest_slot = s.rest + offset.rotated(s.anchor.heading_rad) + give_way;
    let gap = slot - (position + s.anchor.velocity * DT);
    let mut desired =
        s.anchor.velocity + arrive(gap, limits.speed_m_per_s, limits.acceleration_m_per_s2);
    let reach = (rest_slot - position)
        .length()
        .min(desired.length().max(motion.velocity.length()) * s.look_ahead_s);
    desired = avoid(desired, position, rest_slot, &s.obstacles.steering, reach);
    desired = within_stopping(desired, rest_slot - position, limits);
    desired = desired.clamp_length(limits.speed_m_per_s);
    integrate(motion, desired, limits);
    push_out(motion, before, &s.obstacles.footprints);
}

/// Moves every fleet in space and its ships one tick.
fn kinematics(state: &mut UniverseState, goals: &BTreeMap<FleetId, Goal>) {
    let fleets: Vec<(FleetId, SystemId, Fleet)> = state
        .entities
        .ids_with::<Fleet>()
        .into_iter()
        .filter_map(|id| {
            let fleet = state.entities.get::<Fleet>(id)?;
            match fleet.place {
                Place::InSystem(system) => Some((FleetId(id), system, fleet)),
                Place::Docked(_) | Place::InTransit { .. } | Place::EmergencyJump { .. } => None,
            }
        })
        .collect();
    let (obstacles, grid) = surroundings(state, &fleets);

    for (id, system, fleet) in fleets {
        let Some(mode) = state.fleet_mode(id) else {
            continue;
        };
        let Some(mut anchor) = state.entities.get::<Motion>(id.sim_id()) else {
            continue;
        };
        let members: Vec<(ShipId, Vec2, Limits)> = fleet
            .members
            .iter()
            .filter_map(|(&ship, &offset)| {
                let class = state.entities.get::<Ship>(ship.sim_id())?.class;
                Some((ship, offset, class_limits(&state.rules, class, mode)))
            })
            .collect();
        let limits = fleet_limits(&members.iter().map(|m| m.2).collect::<Vec<_>>());

        let goal = goals.get(&id).copied().unwrap_or(Goal::Stop);
        let desired = match goal {
            Goal::Point(point) => arrive(
                point - Vec2::from(anchor.position),
                limits.speed_m_per_s,
                limits.acceleration_m_per_s2,
            ),
            Goal::Stop => Vec2::ZERO,
        };
        integrate(&mut anchor, desired, limits);
        if let Some(m) = state.entities.get_mut::<Motion>(id.sim_id()) {
            *m = anchor;
        }
        let Some(system_obstacles) = obstacles.get(&system) else {
            continue;
        };
        let steering = Steering {
            anchor,
            // Where the anchor will come to rest: the goal, or where braking now stops it.
            rest: match goal {
                Goal::Point(point) => point,
                Goal::Stop => {
                    let v = anchor.velocity;
                    Vec2::from(anchor.position)
                        + v * (v.length() / (2.0 * limits.acceleration_m_per_s2))
                }
            },
            obstacles: system_obstacles,
            look_ahead_s: state.rules.movement.look_ahead_s,
        };
        for (ship, offset, ship_limits) in members {
            let Some(mut motion) = state.entities.get::<Motion>(ship.sim_id()) else {
                continue;
            };
            let give_way = separation(&grid, system, ship, motion.position.into(), &state.rules);
            steer_ship(&mut motion, offset, give_way, ship_limits, &steering);
            if let Some(m) = state.entities.get_mut::<Motion>(ship.sim_id()) {
                *m = motion;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn braking_on_the_stopping_curve_lands_on_the_point() {
        // A body on the curve brakes by exactly one tick's acceleration per tick.
        let a = 15.0;
        let mut distance: f64 = 5_000.0;
        let mut speed = stopping_speed(distance, a);
        let mut ticks = 0;
        while distance > 1e-9 && ticks < 10_000 {
            let next = speed.min(distance / DT);
            assert!(speed - next <= a * DT + 1e-9);
            distance -= next * DT;
            speed = stopping_speed(distance.max(0.0), a);
            ticks += 1;
        }
        assert!(distance.abs() < 1e-6, "{distance}");
    }

    #[test]
    fn overlapping_footprints_are_steered_round_as_one_circle() {
        let a = Obstacle {
            centre: Vec2::ZERO,
            radius_m: 500.0,
        };
        let b = Obstacle {
            centre: Vec2::new(600.0, 0.0),
            radius_m: 200.0,
        };
        let far = Obstacle {
            centre: Vec2::new(5_000.0, 0.0),
            radius_m: 100.0,
        };
        let circles = steering_circles(&[a, b, far], 50.0);
        assert_eq!(circles.len(), 2);
        let both = circles[0].outer;
        for o in [a, b] {
            assert!((o.centre - both.centre).length() + o.radius_m <= both.radius_m + 1e-9);
        }
        assert_eq!(circles[0].members.len(), 2);
        assert!((circles[1].outer.radius_m - 150.0).abs() < 1e-9);
    }

    #[test]
    fn steering_takes_the_tangent_round_an_obstacle_in_the_way() {
        let circle = Obstacle {
            centre: Vec2::new(1_000.0, 0.0),
            radius_m: 300.0,
        };
        let groups = steering_circles(&[circle], 0.0);
        let circles = &groups[..];
        let desired = Vec2::new(100.0, 0.0);
        let slot = Vec2::new(3_000.0, 0.0);
        let steered = avoid(desired, Vec2::ZERO, slot, circles, 2_000.0);
        assert!((steered.length() - 100.0).abs() < 1e-9);
        // The tangent misses the circle.
        let off_line = circle.centre.cross(steered.normalized().unwrap()).abs();
        assert!(off_line >= 300.0 - 1e-6);
        // Out of reach, behind, or with the resting slot inside: no change.
        assert_eq!(avoid(desired, Vec2::ZERO, slot, circles, 500.0), desired);
        assert_eq!(
            avoid(-desired, Vec2::ZERO, slot, circles, 2_000.0),
            -desired
        );
        assert_eq!(
            avoid(desired, Vec2::ZERO, circle.centre, circles, 2_000.0),
            desired
        );
    }

    #[test]
    fn a_ship_pushed_into_a_footprint_is_put_back_on_its_edge() {
        let footprint = Obstacle {
            centre: Vec2::ZERO,
            radius_m: 100.0,
        };
        let mut motion = Motion {
            position: Vec2::new(50.0, 0.0).into(),
            velocity: Vec2::new(-10.0, 5.0),
            heading_rad: 0.0,
        };
        let before = Vec2::new(150.0, 0.0).into();
        push_out(&mut motion, before, &[footprint]);
        assert!((Vec2::from(motion.position).length() - 100.0).abs() < 1e-5);
        assert_eq!(motion.velocity, Vec2::new(0.0, 5.0));
        // In the lens of two overlapping footprints the ship ends outside both: on an edge,
        // or, if the rounds do not settle it, where it was.
        let other = Obstacle {
            centre: Vec2::new(100.0, 0.0),
            radius_m: 100.0,
        };
        let mut lens = Motion {
            position: Vec2::new(50.0, 0.0).into(),
            velocity: Vec2::new(1.0, 1.0),
            heading_rad: 0.0,
        };
        push_out(
            &mut lens,
            Vec2::new(50.0, 100.0).into(),
            &[footprint, other],
        );
        let p = Vec2::from(lens.position);
        assert!(p.length() >= 100.0 && (p - other.centre).length() >= 100.0);
    }
}
