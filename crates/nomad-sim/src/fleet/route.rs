//! Routing over lanes (doc 15 §3.4; S3 plan §4.4).
//!
//! The route is the **shortest by time**: in-system legs at the fleet's cruise speed (its
//! slowest member's), plus, for every jump, the 5 s spool-up and the lane's transit time.
//! Dijkstra over gates: a node is "the fleet is at this gate". Edges are straight in-system
//! legs between two gates of one system and lane hops between the two ends of a lane.
//!
//! Ties are broken by gate identity, so the route is deterministic (D-049). Costs are
//! non-negative `f64` seconds, compared by their IEEE bits, which order like the values.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::fleet::Hop;
use crate::galaxy::{Galaxy, GateId, LaneId, SystemId};
use crate::geometry::Vec2;
use crate::time::{TICKS_PER_SECOND, Ticks};

/// A planned route.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Route {
    /// The jumps, in order. Empty if start and end are in one system.
    pub(crate) hops: VecDeque<Hop>,
    /// The estimated duration, in seconds.
    pub(crate) duration_s: f64,
}

/// Where a route ends.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum RouteEnd {
    /// Anywhere in this system: the arrival is the end (Jump).
    System(SystemId),
    /// At this point of this system (Retreat to a holding).
    Point(SystemId, Vec2),
}

/// The fastest route from `start` in `from` to `end`, travelling in-system legs at
/// `speed_m_per_s` and paying `spool_up` per jump. `None` if there is no route.
pub(crate) fn plan(
    galaxy: &Galaxy,
    from: SystemId,
    start: Vec2,
    end: RouteEnd,
    speed_m_per_s: f64,
    spool_up: Ticks,
) -> Option<Route> {
    plan_avoiding(
        galaxy,
        from,
        start,
        end,
        speed_m_per_s,
        spool_up,
        &BTreeSet::new(),
    )
}

/// Like [`plan`], but the route neither departs from nor arrives at a gate in `avoid`: the
/// gate-avoiding query the Deputy reroutes convoys with (D-055, D-210; A1 plan §5.8).
pub(crate) fn plan_avoiding(
    galaxy: &Galaxy,
    from: SystemId,
    start: Vec2,
    end: RouteEnd,
    speed_m_per_s: f64,
    spool_up: Ticks,
    avoid: &BTreeSet<GateId>,
) -> Option<Route> {
    if speed_m_per_s <= 0.0 || !speed_m_per_s.is_finite() {
        return None;
    }
    let leg = |a: Vec2, b: Vec2| (a - b).length() / speed_m_per_s;
    let spool_s = seconds(spool_up);
    let target_system = match end {
        RouteEnd::System(s) | RouteEnd::Point(s, _) => s,
    };
    galaxy.system(target_system)?;

    // A route inside one system has no hops.
    let direct = match end {
        RouteEnd::Point(system, point) if system == from => Some(leg(start, point)),
        RouteEnd::System(system) if system == from => Some(0.0),
        RouteEnd::Point(..) | RouteEnd::System(_) => None,
    };

    let gate_pos = |g: GateId| galaxy.gate(g).map(|gate| Vec2::from(gate.position));
    let mut best: BTreeMap<GateId, f64> = BTreeMap::new();
    // How each gate was reached: by a lane from another gate, or by an in-system leg.
    let mut came_by: BTreeMap<GateId, (GateId, Option<LaneId>)> = BTreeMap::new();
    let mut open: BTreeSet<(u64, GateId)> = BTreeSet::new();
    for &g in &galaxy.system(from)?.gates {
        if avoid.contains(&g) {
            continue;
        }
        let Some(p) = gate_pos(g) else { continue };
        let cost = leg(start, p);
        best.insert(g, cost);
        open.insert((cost.to_bits(), g));
    }

    let mut finish: Option<(f64, GateId)> = None;
    while let Some((bits, gate)) = open.pop_first() {
        let cost = f64::from_bits(bits);
        if best.get(&gate).is_some_and(|&b| b < cost) {
            continue;
        }
        if let Some((done, _)) = finish
            && cost >= done
        {
            break;
        }
        let Some(g) = galaxy.gate(gate) else { continue };
        let here = Vec2::from(g.position);
        if g.system == target_system && (g.system != from || came_by.contains_key(&gate)) {
            let total = match end {
                RouteEnd::System(_) => cost,
                RouteEnd::Point(_, point) => cost + leg(here, point),
            };
            if finish.is_none_or(|(done, _)| total < done) {
                finish = Some((total, gate));
            }
        }
        let mut relax = |next: GateId, next_cost: f64, via: (GateId, Option<LaneId>)| {
            if avoid.contains(&next) {
                return;
            }
            if best.get(&next).is_none_or(|&b| next_cost < b) {
                best.insert(next, next_cost);
                came_by.insert(next, via);
                open.insert((next_cost.to_bits(), next));
            }
        };
        // Lane hops from this gate.
        for &lane_id in &g.lanes {
            let Some(lane) = galaxy.lane(lane_id) else {
                continue;
            };
            let Some(far) = lane.other_end(gate) else {
                continue;
            };
            relax(
                far,
                cost + spool_s + seconds(lane.transit),
                (gate, Some(lane_id)),
            );
        }
        // In-system legs to the system's other gates.
        let Some(system) = galaxy.system(g.system) else {
            continue;
        };
        for &other in &system.gates {
            if other == gate {
                continue;
            }
            if let Some(p) = gate_pos(other) {
                relax(other, cost + leg(here, p), (gate, None));
            }
        }
    }

    let hopped = finish.map(|(total, last)| Route {
        hops: hops_to(&came_by, last),
        duration_s: total,
    });
    match (direct, hopped) {
        (Some(d), Some(h)) if h.duration_s < d => Some(h),
        (Some(d), _) => Some(Route {
            hops: VecDeque::new(),
            duration_s: d,
        }),
        (None, h) => h,
    }
}

/// The lane hops on the way to `last`, in order, from how each gate was reached.
fn hops_to(came_by: &BTreeMap<GateId, (GateId, Option<LaneId>)>, last: GateId) -> VecDeque<Hop> {
    let mut hops = VecDeque::new();
    let mut at = last;
    while let Some(&(prev, lane)) = came_by.get(&at) {
        if let Some(lane) = lane {
            hops.push_front(Hop {
                from_gate: prev,
                lane,
                to_gate: at,
            });
        }
        at = prev;
    }
    hops
}

fn seconds(duration: Ticks) -> f64 {
    duration.get() as f64 / TICKS_PER_SECOND as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::galaxy::{LaneKind, PositionMetres, SystemKind};
    use crate::id::SimIdAllocator;

    /// Three systems A, B, C. A–B is a slow lane, A–C–B two fast ones.
    fn triangle(slow_minutes: u64) -> (Galaxy, [SystemId; 3], [GateId; 6]) {
        let mut ids = SimIdAllocator::default();
        let mut galaxy = Galaxy::default();
        let sector = galaxy.add_sector(&mut ids);
        let [sys_a, sys_b, sys_c] =
            [(); 3].map(|()| galaxy.add_system(&mut ids, sector, SystemKind::Standard));
        let mut gate = |system, x, y| galaxy.add_gate(&mut ids, system, PositionMetres::new(x, y));
        let ab = gate(sys_a, 100_000.0, 0.0);
        let ac = gate(sys_a, 0.0, 100_000.0);
        let ba = gate(sys_b, -100_000.0, 0.0);
        let bc = gate(sys_b, 0.0, 100_000.0);
        let ca = gate(sys_c, 0.0, -100_000.0);
        let cb = gate(sys_c, 0.0, 100_000.0);
        for (from, to, minutes) in [(ab, ba, slow_minutes), (ac, ca, 40), (cb, bc, 40)] {
            galaxy.add_lane(
                &mut ids,
                from,
                to,
                Ticks::from_minutes(minutes),
                LaneKind::Jump,
            );
        }
        (galaxy, [sys_a, sys_b, sys_c], [ab, ac, ba, bc, ca, cb])
    }

    #[test]
    fn the_route_is_the_shortest_by_time_not_by_jumps() {
        let spool = Ticks::from_secs(5);
        // Slow direct lane: two 40 min jumps plus 200 km of legs win over 120 min.
        let (g, [a, b, _], [_, ac, _, _, ca, cb]) = triangle(120);
        let route = plan(&g, a, Vec2::ZERO, RouteEnd::System(b), 500.0, spool).unwrap();
        let froms: Vec<GateId> = route.hops.iter().map(|h| h.from_gate).collect();
        assert_eq!(froms, vec![ac, cb]);
        assert_eq!(route.hops[0].to_gate, ca);
        // 100 km to the gate, 5 s + 40 min, 200 km across C, 5 s + 40 min.
        let expected = 200.0 + 5.0 + 2_400.0 + 400.0 + 5.0 + 2_400.0;
        assert!((route.duration_s - expected).abs() < 1e-6);

        // A fast direct lane wins.
        let (g, [a, b, _], [ab, ..]) = triangle(50);
        let route = plan(&g, a, Vec2::ZERO, RouteEnd::System(b), 500.0, spool).unwrap();
        assert_eq!(route.hops.len(), 1);
        assert_eq!(route.hops[0].from_gate, ab);
        assert!((route.duration_s - (200.0 + 5.0 + 3_000.0)).abs() < 1e-6);
    }

    #[test]
    fn an_avoided_gate_is_neither_departed_from_nor_arrived_at() {
        let spool = Ticks::from_secs(5);
        // The direct lane wins unless its gate is avoided (D-055, A1 plan §5.8).
        let (g, [a, b, _], [ab, ac, ba, _, _, cb]) = triangle(50);
        let avoid_ab = BTreeSet::from([ab]);
        let route = plan_avoiding(
            &g,
            a,
            Vec2::ZERO,
            RouteEnd::System(b),
            500.0,
            spool,
            &avoid_ab,
        )
        .unwrap();
        let froms: Vec<GateId> = route.hops.iter().map(|h| h.from_gate).collect();
        assert_eq!(froms, vec![ac, cb]);
        // Avoiding the arrival gate in B also forces the detour.
        let avoid_ba = BTreeSet::from([ba]);
        let route = plan_avoiding(
            &g,
            a,
            Vec2::ZERO,
            RouteEnd::System(b),
            500.0,
            spool,
            &avoid_ba,
        )
        .unwrap();
        assert_eq!(route.hops.len(), 2);
        // With both ways blocked there is no route.
        let both = BTreeSet::from([ab, cb]);
        assert!(
            plan_avoiding(&g, a, Vec2::ZERO, RouteEnd::System(b), 500.0, spool, &both).is_none()
        );
    }

    #[test]
    fn routes_to_a_point_include_the_last_leg_and_stay_local_when_cheaper() {
        let spool = Ticks::from_secs(5);
        let (g, [a, b, _], _) = triangle(50);
        let point = Vec2::new(-100_000.0, 50_000.0);
        let route = plan(&g, a, Vec2::ZERO, RouteEnd::Point(b, point), 500.0, spool).unwrap();
        assert_eq!(route.hops.len(), 1);
        assert!((route.duration_s - (200.0 + 5.0 + 3_000.0 + 100.0)).abs() < 1e-6);
        // In the same system: a straight leg, no hops.
        let local = plan(
            &g,
            a,
            Vec2::ZERO,
            RouteEnd::Point(a, Vec2::new(0.0, 5_000.0)),
            500.0,
            spool,
        )
        .unwrap();
        assert!(local.hops.is_empty());
        assert!((local.duration_s - 10.0).abs() < 1e-9);
    }

    #[test]
    fn no_route_without_lanes() {
        let mut ids = SimIdAllocator::default();
        let mut g = Galaxy::default();
        let sector = g.add_sector(&mut ids);
        let a = g.add_system(&mut ids, sector, SystemKind::Standard);
        let b = g.add_system(&mut ids, sector, SystemKind::Standard);
        g.add_gate(&mut ids, a, PositionMetres::new(1.0, 0.0));
        let spool = Ticks::from_secs(5);
        assert!(plan(&g, a, Vec2::ZERO, RouteEnd::System(b), 500.0, spool).is_none());
        assert!(plan(&g, a, Vec2::ZERO, RouteEnd::System(a), 0.0, spool).is_none());
    }
}
