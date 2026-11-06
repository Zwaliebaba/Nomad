//! Generator invariants (Design doc 10 §4.3) and the agreed scale values (D-043).
//!
//! [`validate`] checks a galaxy and reports what it measured, plus every violation it found.
//! The generator runs it on its own output, and tests run it over many seeds.
//!
//! Checked:
//!
//! - **Integrity:** every reference resolves and is mirrored (sector ↔ system, system ↔
//!   site and gate, gate ↔ lane); a lane joins two different systems; jump lanes stay in one
//!   sector and trunk lanes cross sectors (doc 10 §4.2).
//! - **Scale values (D-043, doc 10 §4.1, §8, §12):** sites and gates per system by kind, jump
//!   and trunk transit ranges, 50 km minimum site spacing, and in-system trips of at most
//!   15 min at the slowest warship's cruise speed.
//! - **Invariant 1:** the galaxy is connected, and each sector's internal graph has a cycle.
//! - **Invariant 2:** sector diameter of at most 4 jumps.
//! - **Invariant 3:** the worst-case trip between two sites of a sector takes at most 6 h for
//!   the slowest warship class: straight in-system legs at cruise speed plus lane transits.
//! - **Invariant 4:** home systems at least 2 jumps apart.
//! - **Invariant 5:** every home neighbourhood (the home and all systems within 2 jumps) has
//!   a total deposit value within ±10 % of every other home's, with the D-066 values
//!   (doc 14 §5). Also checked: at most 2 deposits per site.
//! - **Invariant 6:** at most 10 % dead ends, not counting raider havens.
//!
//! Not checked:
//!
//! - **Invariant 7** (each sector borders 2–4 others through trunk lanes) is about a
//!   multi-sector galaxy. MVP stage 1 has a single sector (doc 10 §3), so it cannot apply.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::RangeInclusive;

use thiserror::Error;

use crate::galaxy::{
    DepositValue, Galaxy, GateId, LaneId, LaneKind, MAX_DEPOSITS_PER_SITE, PositionKm, SectorId,
    SiteId, SystemId, SystemKind, is_claimable,
};
use crate::id::SimId;
use crate::time::{TICKS_PER_SECOND, Ticks};

/// The limits the validator checks against. [`Default`] gives the agreed starting values
/// (D-043), which the pacing simulation may tune (D-037).
#[derive(Clone, Debug, PartialEq)]
pub struct InvariantLimits {
    /// Invariant 2: maximum sector diameter.
    pub max_sector_diameter_jumps: u32,
    /// Invariant 3: maximum worst-case trip between two sites of a sector.
    pub max_sector_trip: Ticks,
    /// Invariant 4: minimum number of jumps between two home systems.
    pub min_home_separation_jumps: u32,
    /// Invariant 6: maximum share of dead ends among systems that are not raider havens.
    pub max_dead_end_percent: u32,
    /// Cruise speed of the slowest warship class, for invariant 3 and in-system trips. The
    /// WARDEN frigate's 500 m/s is a doc 15 §12 starting value.
    pub slowest_warship_cruise_m_per_s: f64,
    /// Longest trip between two points of one system at that speed (doc 10 §4.1).
    pub max_in_system_trip: Ticks,
    /// Minimum distance between two sites of one system (doc 10 §8).
    pub min_site_spacing_km: f64,
    /// Jump lane transit range (doc 10 §4.2).
    pub jump_transit: RangeInclusive<Ticks>,
    /// Trunk lane transit range (doc 10 §4.2).
    pub trunk_transit: RangeInclusive<Ticks>,
    /// Invariant 5: how far, in percent, a home neighbourhood's deposit value may lie from
    /// any other's.
    pub max_neighbourhood_spread_percent: u64,
}

impl Default for InvariantLimits {
    fn default() -> Self {
        Self {
            max_sector_diameter_jumps: 4,
            max_sector_trip: Ticks::from_hours(6),
            min_home_separation_jumps: 2,
            max_dead_end_percent: 10,
            slowest_warship_cruise_m_per_s: 500.0,
            max_in_system_trip: Ticks::from_minutes(15),
            min_site_spacing_km: 50.0,
            jump_transit: Ticks::from_minutes(40)..=Ticks::from_minutes(70),
            trunk_transit: Ticks::from_minutes(90)..=Ticks::from_minutes(150),
            max_neighbourhood_spread_percent: 10,
        }
    }
}

/// Sites a system of `kind` may have (D-043). `None` where nothing is agreed: raider havens.
pub const fn allowed_sites(kind: SystemKind) -> Option<RangeInclusive<usize>> {
    match kind {
        SystemKind::Home => Some(7..=7),
        SystemKind::Standard => Some(5..=7),
        SystemKind::Hub => Some(8..=9),
        SystemKind::RaiderHaven => None,
    }
}

/// Gates a system of `kind` may have (D-043). `None` for raider havens: doc 10 §4.4 links
/// the stage-1 haven to the hub alone, so the 2–4 rule does not apply to it.
pub const fn allowed_gates(kind: SystemKind) -> Option<RangeInclusive<usize>> {
    match kind {
        SystemKind::Home | SystemKind::Standard => Some(2..=4),
        SystemKind::Hub => Some(2..=5),
        SystemKind::RaiderHaven => None,
    }
}

/// The slowest trip between two sites of a sector (invariant 3).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorstTrip {
    /// One end.
    pub from: SiteId,
    /// The other end.
    pub to: SiteId,
    /// The fastest route between them, in seconds. Infinite if there is none.
    pub duration_s: f64,
}

/// What the validator measured in one sector.
#[derive(Clone, Debug, PartialEq)]
pub struct SectorReport {
    /// The sector.
    pub sector: SectorId,
    /// Its systems, raider havens included.
    pub systems: usize,
    /// Its sites.
    pub sites: usize,
    /// Jump lanes inside it.
    pub jump_lanes: usize,
    /// Most jumps between two of its systems, or `None` if it is not connected.
    pub diameter_jumps: Option<u32>,
    /// Independent cycles in its system graph (edges − systems + components). Invariant 1
    /// needs at least one.
    pub cycle_rank: usize,
    /// Systems, other than raider havens, with at most one neighbouring system.
    pub dead_ends: usize,
    /// Invariant 3's worst case, or `None` with fewer than two sites.
    pub worst_trip: Option<WorstTrip>,
}

/// What the validator measured and found.
#[derive(Clone, Debug, PartialEq)]
pub struct GalaxyReport {
    /// One report per sector, in sector order.
    pub sectors: Vec<SectorReport>,
    /// Fewest jumps between two home systems, or `None` with fewer than two connected homes.
    pub min_home_separation_jumps: Option<u32>,
    /// The longest straight trip between two points of one system, in seconds.
    pub longest_in_system_trip_s: f64,
    /// The smallest distance between two sites of one system, in km.
    pub closest_sites_km: Option<f64>,
    /// Invariant 5: each home system's neighbourhood deposit value.
    pub home_neighbourhoods: BTreeMap<SystemId, DepositValue>,
    /// Everything that is wrong, in the order it was found.
    pub violations: Vec<Violation>,
}

impl GalaxyReport {
    /// Whether the galaxy satisfies every checked invariant.
    pub fn is_valid(&self) -> bool {
        self.violations.is_empty()
    }
}

/// One way a galaxy breaks an invariant or an agreed value.
#[derive(Clone, Debug, PartialEq, Error)]
pub enum Violation {
    /// An object refers to another that is missing or does not refer back.
    #[error("{from:?} refers to {to:?}, which is missing or does not refer back")]
    BrokenLink {
        /// The referring object.
        from: SimId,
        /// The object referred to.
        to: SimId,
    },
    /// A lane does not join two different systems.
    #[error("lane {0:?} does not join two different systems")]
    LaneLoop(LaneId),
    /// A jump lane joins systems of different sectors (doc 10 §4.2).
    #[error("jump lane {0:?} leaves its sector")]
    JumpLaneLeavesSector(LaneId),
    /// A trunk lane joins systems of one sector (doc 10 §4.2).
    #[error("trunk lane {0:?} stays inside one sector")]
    TrunkLaneInsideSector(LaneId),
    /// A system has a number of sites its kind does not allow (D-043).
    #[error("{kind:?} system {system:?} has {count} sites")]
    SiteCount {
        /// The system.
        system: SystemId,
        /// Its kind.
        kind: SystemKind,
        /// Its sites.
        count: usize,
    },
    /// A system has a number of gates its kind does not allow (D-043).
    #[error("{kind:?} system {system:?} has {count} gates")]
    GateCount {
        /// The system.
        system: SystemId,
        /// Its kind.
        kind: SystemKind,
        /// Its gates.
        count: usize,
    },
    /// A lane's transit is outside its kind's range (D-043).
    #[error("{kind:?} lane {lane:?} has a transit of {transit:?}")]
    TransitOutOfRange {
        /// The lane.
        lane: LaneId,
        /// Its kind.
        kind: LaneKind,
        /// Its transit.
        transit: Ticks,
    },
    /// Two sites of one system are closer than the minimum spacing (doc 10 §8).
    #[error("sites {a:?} and {b:?} are {distance_km:.1} km apart")]
    SitesTooClose {
        /// One site.
        a: SiteId,
        /// The other.
        b: SiteId,
        /// Their distance.
        distance_km: f64,
    },
    /// Crossing a system takes longer than allowed (doc 10 §4.1).
    #[error("crossing system {system:?} takes {duration_s:.0} s")]
    InSystemTripTooLong {
        /// The system.
        system: SystemId,
        /// Its longest straight trip.
        duration_s: f64,
    },
    /// Invariant 1: some systems cannot reach the others.
    #[error("the galaxy is not connected")]
    GalaxyDisconnected,
    /// Invariant 1: a sector's internal graph has no cycle.
    #[error("sector {0:?} has no cycle")]
    NoCycle(SectorId),
    /// Invariant 2: a sector's systems cannot all reach each other through its jump lanes.
    #[error("sector {0:?} is not connected")]
    SectorDisconnected(SectorId),
    /// Invariant 2: a sector is too wide.
    #[error("sector {sector:?} has a diameter of {jumps} jumps")]
    DiameterTooLarge {
        /// The sector.
        sector: SectorId,
        /// Its diameter.
        jumps: u32,
    },
    /// Invariant 3: a trip between two sites of a sector takes too long.
    #[error("the trip from {from:?} to {to:?} in sector {sector:?} takes {duration_s:.0} s")]
    TripTooLong {
        /// The sector.
        sector: SectorId,
        /// One end.
        from: SiteId,
        /// The other end.
        to: SiteId,
        /// Its fastest route.
        duration_s: f64,
    },
    /// Invariant 4: two home systems are too close.
    #[error("home systems {a:?} and {b:?} are {jumps} jumps apart")]
    HomesTooClose {
        /// One home.
        a: SystemId,
        /// The other.
        b: SystemId,
        /// Jumps between them.
        jumps: u32,
    },
    /// Invariant 6: too many dead ends.
    #[error("sector {sector:?} has {dead_ends} dead ends among {systems} systems")]
    TooManyDeadEnds {
        /// The sector.
        sector: SectorId,
        /// Dead ends.
        dead_ends: usize,
        /// Systems that are not raider havens.
        systems: usize,
    },
    /// A site has more deposits than allowed (D-066).
    #[error("site {site:?} has {count} deposits")]
    TooManyDeposits {
        /// The site.
        site: SiteId,
        /// Its deposits.
        count: usize,
    },
    /// Invariant 5: the richest and poorest home neighbourhoods are too far apart.
    #[error(
        "home neighbourhoods are unbalanced: {richest:?} has {richest_value}, \
         {poorest:?} has {poorest_value}"
    )]
    NeighbourhoodImbalance {
        /// The home with the richest neighbourhood.
        richest: SystemId,
        /// Its neighbourhood value.
        richest_value: DepositValue,
        /// The home with the poorest neighbourhood.
        poorest: SystemId,
        /// Its neighbourhood value.
        poorest_value: DepositValue,
    },
}

/// Checks `galaxy` against doc 10 §4.3 and the D-043 values in `limits`.
pub fn validate(galaxy: &Galaxy, limits: &InvariantLimits) -> GalaxyReport {
    let mut violations = Vec::new();
    check_integrity(galaxy, &mut violations);
    check_lanes(galaxy, limits, &mut violations);
    let (longest_in_system_trip_s, closest_sites_km) =
        check_systems(galaxy, limits, &mut violations);

    let sectors = galaxy
        .sectors()
        .map(|(id, _)| check_sector(galaxy, id, limits, &mut violations))
        .collect();

    if !is_connected(galaxy) {
        violations.push(Violation::GalaxyDisconnected);
    }
    let min_home_separation_jumps = check_home_separation(galaxy, limits, &mut violations);

    for (site, s) in galaxy.sites() {
        if s.deposits.len() > MAX_DEPOSITS_PER_SITE {
            violations.push(Violation::TooManyDeposits {
                site,
                count: s.deposits.len(),
            });
        }
    }
    let home_neighbourhoods = galaxy.home_neighbourhood_values();
    if let Some(violation) = neighbourhood_imbalance(
        &home_neighbourhoods,
        limits.max_neighbourhood_spread_percent,
    ) {
        violations.push(violation);
    }

    GalaxyReport {
        sectors,
        min_home_separation_jumps,
        longest_in_system_trip_s,
        closest_sites_km,
        home_neighbourhoods,
        violations,
    }
}

/// Invariant 5: `None` if every value lies within `spread_percent` of every other value.
///
/// "Within ±10 % of every other home's" must hold in both directions for every pair, so the
/// binding pair is the richest against the poorest: `richest − poorest ≤ 10 % × poorest`.
/// Integer arithmetic only.
pub(crate) fn neighbourhood_imbalance(
    values: &BTreeMap<SystemId, DepositValue>,
    spread_percent: u64,
) -> Option<Violation> {
    // Ties go to the lowest identity, so the report is deterministic.
    let mut richest = values.iter().next()?;
    let mut poorest = richest;
    for entry in values {
        if entry.1 > richest.1 {
            richest = entry;
        }
        if entry.1 < poorest.1 {
            poorest = entry;
        }
    }
    let spread = u128::from(richest.1.milli().saturating_sub(poorest.1.milli()));
    let allowed = u128::from(spread_percent) * u128::from(poorest.1.milli());
    (spread * 100 > allowed).then_some(Violation::NeighbourhoodImbalance {
        richest: *richest.0,
        richest_value: *richest.1,
        poorest: *poorest.0,
        poorest_value: *poorest.1,
    })
}

fn check_integrity(galaxy: &Galaxy, out: &mut Vec<Violation>) {
    let mut link = |ok: bool, from: SimId, to: SimId| {
        if !ok {
            out.push(Violation::BrokenLink { from, to });
        }
    };
    for (id, sector) in galaxy.sectors() {
        for &system in &sector.systems {
            let ok = galaxy.system(system).is_some_and(|s| s.sector == id);
            link(ok, id.sim_id(), system.sim_id());
        }
    }
    for (id, system) in galaxy.systems() {
        let ok = galaxy
            .sector(system.sector)
            .is_some_and(|s| s.systems.contains(&id));
        link(ok, id.sim_id(), system.sector.sim_id());
        for &site in &system.sites {
            let ok = galaxy.site(site).is_some_and(|s| s.system == id);
            link(ok, id.sim_id(), site.sim_id());
        }
        for &gate in &system.gates {
            let ok = galaxy.gate(gate).is_some_and(|g| g.system == id);
            link(ok, id.sim_id(), gate.sim_id());
        }
    }
    for (id, site) in galaxy.sites() {
        let ok = galaxy
            .system(site.system)
            .is_some_and(|s| s.sites.contains(&id));
        link(ok, id.sim_id(), site.system.sim_id());
    }
    for (id, gate) in galaxy.gates() {
        let ok = galaxy
            .system(gate.system)
            .is_some_and(|s| s.gates.contains(&id));
        link(ok, id.sim_id(), gate.system.sim_id());
        for &lane in &gate.lanes {
            let ok = galaxy.lane(lane).is_some_and(|l| l.other_end(id).is_some());
            link(ok, id.sim_id(), lane.sim_id());
        }
    }
    for (id, lane) in galaxy.lanes() {
        for gate in [lane.gate_a, lane.gate_b] {
            let ok = galaxy.gate(gate).is_some_and(|g| g.lanes.contains(&id));
            link(ok, id.sim_id(), gate.sim_id());
        }
    }
}

fn check_lanes(galaxy: &Galaxy, limits: &InvariantLimits, out: &mut Vec<Violation>) {
    for (id, lane) in galaxy.lanes() {
        let range = match lane.kind {
            LaneKind::Jump => &limits.jump_transit,
            LaneKind::Trunk => &limits.trunk_transit,
        };
        if !range.contains(&lane.transit) {
            out.push(Violation::TransitOutOfRange {
                lane: id,
                kind: lane.kind,
                transit: lane.transit,
            });
        }
        let Some((a, b)) = galaxy.lane_systems(lane) else {
            continue; // Reported as a broken link.
        };
        if a == b {
            out.push(Violation::LaneLoop(id));
            continue;
        }
        let sector_of = |s| galaxy.system(s).map(|s| s.sector);
        let same_sector = sector_of(a) == sector_of(b);
        match lane.kind {
            LaneKind::Jump if !same_sector => out.push(Violation::JumpLaneLeavesSector(id)),
            LaneKind::Trunk if same_sector => out.push(Violation::TrunkLaneInsideSector(id)),
            LaneKind::Jump | LaneKind::Trunk => {}
        }
    }
}

/// Per-system checks. Returns the longest in-system trip (s) and the closest site pair (km).
fn check_systems(
    galaxy: &Galaxy,
    limits: &InvariantLimits,
    out: &mut Vec<Violation>,
) -> (f64, Option<f64>) {
    let max_trip_s = secs(limits.max_in_system_trip);
    let speed = limits.slowest_warship_cruise_m_per_s;
    let mut longest_s: f64 = 0.0;
    let mut closest_km: Option<f64> = None;
    for (id, system) in galaxy.systems() {
        if let Some(range) = allowed_sites(system.kind)
            && !range.contains(&system.sites.len())
        {
            out.push(Violation::SiteCount {
                system: id,
                kind: system.kind,
                count: system.sites.len(),
            });
        }
        if let Some(range) = allowed_gates(system.kind)
            && !range.contains(&system.gates.len())
        {
            out.push(Violation::GateCount {
                system: id,
                kind: system.kind,
                count: system.gates.len(),
            });
        }

        let sites: Vec<(SiteId, PositionKm)> = system
            .sites
            .iter()
            .filter_map(|&s| Some((s, galaxy.site(s)?.position)))
            .collect();
        for (i, &(a, pa)) in sites.iter().enumerate() {
            for &(b, pb) in &sites[i + 1..] {
                let distance_km = pa.distance_km(pb);
                closest_km = Some(closest_km.map_or(distance_km, |c| c.min(distance_km)));
                if distance_km < limits.min_site_spacing_km {
                    out.push(Violation::SitesTooClose { a, b, distance_km });
                }
            }
        }

        let points: Vec<PositionKm> = sites
            .iter()
            .map(|&(_, p)| p)
            .chain(
                system
                    .gates
                    .iter()
                    .filter_map(|&g| Some(galaxy.gate(g)?.position)),
            )
            .collect();
        let mut system_longest_s: f64 = 0.0;
        for (i, &a) in points.iter().enumerate() {
            for &b in &points[i + 1..] {
                system_longest_s = system_longest_s.max(leg_s(a, b, speed));
            }
        }
        longest_s = longest_s.max(system_longest_s);
        if system_longest_s > max_trip_s {
            out.push(Violation::InSystemTripTooLong {
                system: id,
                duration_s: system_longest_s,
            });
        }
    }
    (longest_s, closest_km)
}

fn check_sector(
    galaxy: &Galaxy,
    sector: SectorId,
    limits: &InvariantLimits,
    out: &mut Vec<Violation>,
) -> SectorReport {
    let systems: Vec<SystemId> = galaxy
        .sector(sector)
        .map(|s| {
            s.systems
                .iter()
                .copied()
                .filter(|&id| galaxy.system(id).is_some())
                .collect()
        })
        .unwrap_or_default();
    let members: BTreeSet<SystemId> = systems.iter().copied().collect();
    let internal = |lane: &crate::galaxy::Lane| lane.kind == LaneKind::Jump;
    let adjacency: BTreeMap<SystemId, BTreeSet<SystemId>> = systems
        .iter()
        .map(|&id| {
            let near = galaxy
                .neighbours_filtered(id, internal)
                .into_iter()
                .filter(|n| members.contains(n))
                .collect();
            (id, near)
        })
        .collect();

    // Invariant 2: diameter, by breadth-first search from every system.
    let mut diameter = Some(0_u32);
    for &id in &systems {
        let jumps = galaxy.jumps_from_filtered(id, internal);
        let reached: Vec<u32> = systems
            .iter()
            .filter_map(|s| jumps.get(s).copied())
            .collect();
        if reached.len() < systems.len() {
            diameter = None;
            break;
        }
        let widest = reached.into_iter().max().unwrap_or(0);
        diameter = diameter.map(|d| d.max(widest));
    }
    match diameter {
        None => out.push(Violation::SectorDisconnected(sector)),
        Some(jumps) if jumps > limits.max_sector_diameter_jumps => {
            out.push(Violation::DiameterTooLarge { sector, jumps });
        }
        Some(_) => {}
    }

    // Invariant 1: a cycle exists if edges − vertices + components > 0.
    let edges = adjacency.values().map(BTreeSet::len).sum::<usize>() / 2;
    let cycle_rank = (edges + components(&adjacency)).saturating_sub(systems.len());
    if cycle_rank == 0 {
        out.push(Violation::NoCycle(sector));
    }

    // Invariant 6: dead ends among systems that are not raider havens. Lanes of any kind
    // count, so a border system with one jump lane and a trunk lane is not a dead end.
    let candidates: Vec<SystemId> = systems
        .iter()
        .copied()
        .filter(|&id| galaxy.system(id).is_some_and(|s| is_claimable(s.kind)))
        .collect();
    let dead_ends = candidates
        .iter()
        .filter(|&&id| galaxy.neighbours(id).len() <= 1)
        .count();
    let allowed = u64::from(limits.max_dead_end_percent);
    if as_u64(dead_ends) * 100 > allowed * as_u64(candidates.len()) {
        out.push(Violation::TooManyDeadEnds {
            sector,
            dead_ends,
            systems: candidates.len(),
        });
    }

    // Invariant 3.
    let worst_trip = worst_trip(galaxy, &systems, limits.slowest_warship_cruise_m_per_s);
    if let Some(trip) = worst_trip
        && trip.duration_s > secs(limits.max_sector_trip)
    {
        out.push(Violation::TripTooLong {
            sector,
            from: trip.from,
            to: trip.to,
            duration_s: trip.duration_s,
        });
    }

    SectorReport {
        sector,
        systems: systems.len(),
        sites: systems
            .iter()
            .filter_map(|&id| galaxy.system(id))
            .map(|s| s.sites.len())
            .sum(),
        jump_lanes: galaxy
            .lanes()
            .filter(|(_, lane)| {
                lane.kind == LaneKind::Jump
                    && galaxy
                        .lane_systems(lane)
                        .is_some_and(|(a, b)| members.contains(&a) && members.contains(&b))
            })
            .count(),
        diameter_jumps: diameter,
        cycle_rank,
        dead_ends,
        worst_trip,
    }
}

/// Shortest travel times between all gates of `systems`.
struct GateTimes {
    gates: Vec<(GateId, PositionKm)>,
    index: BTreeMap<GateId, usize>,
    /// Row-major `gates.len()²` matrix of seconds; infinite where there is no route.
    time: Vec<f64>,
}

/// Floyd–Warshall over the gates of `systems`: straight legs between gates of one system,
/// and jump lanes between systems.
fn gate_times(galaxy: &Galaxy, systems: &[SystemId], speed_m_per_s: f64) -> GateTimes {
    let gates: Vec<(GateId, PositionKm)> = systems
        .iter()
        .filter_map(|&id| galaxy.system(id))
        .flat_map(|s| s.gates.iter())
        .filter_map(|&g| Some((g, galaxy.gate(g)?.position)))
        .collect();
    let index: BTreeMap<GateId, usize> = gates
        .iter()
        .enumerate()
        .map(|(i, &(g, _))| (g, i))
        .collect();
    let n = gates.len();
    let mut time = vec![f64::INFINITY; n * n];
    for i in 0..n {
        time[i * n + i] = 0.0;
    }
    for &id in systems {
        let Some(system) = galaxy.system(id) else {
            continue;
        };
        let local: Vec<usize> = system
            .gates
            .iter()
            .filter_map(|g| index.get(g).copied())
            .collect();
        for &i in &local {
            for &j in &local {
                time[i * n + j] = time[i * n + j].min(leg_s(gates[i].1, gates[j].1, speed_m_per_s));
            }
        }
    }
    for (i, &(gate, _)) in gates.iter().enumerate() {
        let Some(g) = galaxy.gate(gate) else { continue };
        for lane in g.lanes.iter().filter_map(|&l| galaxy.lane(l)) {
            if lane.kind != LaneKind::Jump {
                continue;
            }
            if let Some(&j) = lane.other_end(gate).and_then(|far| index.get(&far)) {
                time[i * n + j] = time[i * n + j].min(secs(lane.transit));
            }
        }
    }
    for k in 0..n {
        for i in 0..n {
            let via = time[i * n + k];
            if via == f64::INFINITY {
                continue;
            }
            for j in 0..n {
                let candidate = via + time[k * n + j];
                if candidate < time[i * n + j] {
                    time[i * n + j] = candidate;
                }
            }
        }
    }

    GateTimes { gates, index, time }
}

/// The slowest of the fastest routes between any two sites of `systems`, travelling the
/// sector's jump lanes and straight in-system legs at `speed_m_per_s` (invariant 3).
///
/// All-pairs shortest times between the sector's gates (Floyd–Warshall), then for each pair
/// of sites the best combination of a leg to a gate, a gate-to-gate route and a leg from a
/// gate. A direct leg is used for two sites of one system.
fn worst_trip(galaxy: &Galaxy, systems: &[SystemId], speed_m_per_s: f64) -> Option<WorstTrip> {
    let GateTimes { gates, index, time } = gate_times(galaxy, systems, speed_m_per_s);
    let n = gates.len();

    // (site, its system's gate indices, position)
    let mut sites: Vec<(SiteId, SystemId, PositionKm)> = Vec::new();
    let mut gates_of: BTreeMap<SystemId, Vec<usize>> = BTreeMap::new();
    for &id in systems {
        let Some(system) = galaxy.system(id) else {
            continue;
        };
        gates_of.insert(
            id,
            system
                .gates
                .iter()
                .filter_map(|g| index.get(g).copied())
                .collect(),
        );
        sites.extend(
            system
                .sites
                .iter()
                .filter_map(|&s| Some((s, id, galaxy.site(s)?.position))),
        );
    }

    let mut worst: Option<WorstTrip> = None;
    for (i, &(a, sys_a, pa)) in sites.iter().enumerate() {
        for &(b, sys_b, pb) in &sites[i + 1..] {
            let mut best = if sys_a == sys_b {
                leg_s(pa, pb, speed_m_per_s)
            } else {
                f64::INFINITY
            };
            for &g in gates_of.get(&sys_a).into_iter().flatten() {
                let out_s = leg_s(pa, gates[g].1, speed_m_per_s);
                for &h in gates_of.get(&sys_b).into_iter().flatten() {
                    let total = out_s + time[g * n + h] + leg_s(gates[h].1, pb, speed_m_per_s);
                    best = best.min(total);
                }
            }
            if worst.is_none_or(|w| best > w.duration_s) {
                worst = Some(WorstTrip {
                    from: a,
                    to: b,
                    duration_s: best,
                });
            }
        }
    }
    worst
}

/// Invariant 4. Returns the fewest jumps between two connected home systems.
fn check_home_separation(
    galaxy: &Galaxy,
    limits: &InvariantLimits,
    out: &mut Vec<Violation>,
) -> Option<u32> {
    let homes: Vec<SystemId> = galaxy
        .systems()
        .filter(|(_, s)| s.kind == SystemKind::Home)
        .map(|(id, _)| id)
        .collect();
    let mut closest = None;
    for (i, &a) in homes.iter().enumerate() {
        let jumps = galaxy.jumps_from(a);
        for &b in &homes[i + 1..] {
            let Some(&n) = jumps.get(&b) else { continue };
            closest = Some(closest.map_or(n, |c: u32| c.min(n)));
            if n < limits.min_home_separation_jumps {
                out.push(Violation::HomesTooClose { a, b, jumps: n });
            }
        }
    }
    closest
}

/// Invariant 1: every system reaches every other through lanes of any kind.
fn is_connected(galaxy: &Galaxy) -> bool {
    let Some((first, _)) = galaxy.systems().next() else {
        return true;
    };
    galaxy.jumps_from(first).len() == galaxy.systems().count()
}

/// Connected components of an undirected graph.
fn components(adjacency: &BTreeMap<SystemId, BTreeSet<SystemId>>) -> usize {
    let mut seen = BTreeSet::new();
    let mut count = 0;
    for &start in adjacency.keys() {
        if !seen.insert(start) {
            continue;
        }
        count += 1;
        let mut stack = vec![start];
        while let Some(id) = stack.pop() {
            for &next in adjacency.get(&id).into_iter().flatten() {
                if seen.insert(next) {
                    stack.push(next);
                }
            }
        }
    }
    count
}

/// Travel time in seconds for a straight leg at `speed_m_per_s`.
fn leg_s(a: PositionKm, b: PositionKm, speed_m_per_s: f64) -> f64 {
    a.distance_km(b) * 1_000.0 / speed_m_per_s
}

/// A duration in seconds.
fn secs(duration: Ticks) -> f64 {
    duration.get() as f64 / TICKS_PER_SECOND as f64
}

fn as_u64(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::galaxy::{Deposit, Richness, STAGE1_COMMANDERS, generate_stage1};
    use crate::id::{CommanderId, SimIdAllocator};
    use crate::state::SeasonConfig;

    const COMMANDERS: [CommanderId; STAGE1_COMMANDERS] = [
        CommanderId(1),
        CommanderId(2),
        CommanderId(3),
        CommanderId(4),
    ];

    /// A generated stage-1 galaxy and an allocator that continues after its identities.
    fn stage1(seed: u64) -> (Galaxy, SimIdAllocator) {
        let state = generate_stage1(
            SeasonConfig {
                seed,
                season_start_unix_ms: 0,
            },
            COMMANDERS,
        )
        .unwrap();
        (state.galaxy, state.ids)
    }

    fn report(galaxy: &Galaxy) -> GalaxyReport {
        validate(galaxy, &InvariantLimits::default())
    }

    /// Systems in creation order: H1, N1, H2, N2, H3, N3, H4, N4, C, R.
    fn systems(g: &Galaxy) -> Vec<SystemId> {
        g.systems().map(|(id, _)| id).collect()
    }

    /// The lane between systems `a` and `b`.
    fn lane_between(g: &Galaxy, a: SystemId, b: SystemId) -> LaneId {
        g.lanes()
            .find(|(_, lane)| {
                g.lane_systems(lane)
                    .is_some_and(|ends| ends == (a, b) || ends == (b, a))
            })
            .map(|(id, _)| id)
            .unwrap()
    }

    fn remove_lane(g: &mut Galaxy, lane: LaneId) {
        let removed = g.lanes.remove(&lane).unwrap();
        for gate in [removed.gate_a, removed.gate_b] {
            g.gates.get_mut(&gate).unwrap().lanes.retain(|&l| l != lane);
        }
    }

    fn has(report: &GalaxyReport, matches: impl Fn(&Violation) -> bool) -> bool {
        report.violations.iter().any(matches)
    }

    #[test]
    fn generated_sector_passes_with_the_expected_shape() {
        let (g, _) = stage1(3);
        let r = report(&g);
        assert!(r.is_valid(), "{:?}", r.violations);
        let sector = &r.sectors[0];
        assert_eq!(sector.systems, 10);
        assert_eq!(sector.sites, 60);
        assert_eq!(sector.jump_lanes, 13);
        assert_eq!(sector.diameter_jumps, Some(4));
        // 13 lanes − 10 systems + 1 component: the ring plus one cycle per extra spoke.
        assert_eq!(sector.cycle_rank, 4);
        assert_eq!(sector.dead_ends, 0);
        assert_eq!(r.min_home_separation_jumps, Some(2));
        let trip = sector.worst_trip.unwrap();
        // At least 4 transits of 40 min; at most the 6 h limit.
        assert!(trip.duration_s >= 4.0 * 40.0 * 60.0);
        assert!(trip.duration_s <= 6.0 * 3_600.0);
        assert!(r.closest_sites_km.unwrap() >= 50.0);
        assert!(r.longest_in_system_trip_s <= 15.0 * 60.0);
    }

    #[test]
    fn worst_trip_routes_through_the_shortest_path() {
        // Two systems, one lane of 50 min, gates 100 km from each site: 2 × 200 s + 3,000 s.
        let mut ids = SimIdAllocator::default();
        let mut g = Galaxy::default();
        let sector = g.add_sector(&mut ids);
        let a = g.add_system(&mut ids, sector, SystemKind::Standard);
        let b = g.add_system(&mut ids, sector, SystemKind::Standard);
        let sa = g.add_site(&mut ids, a, PositionKm::ORIGIN);
        let sb = g.add_site(&mut ids, b, PositionKm::ORIGIN);
        let ga = g.add_gate(&mut ids, a, PositionKm { x: 100.0, y: 0.0 });
        let gb = g.add_gate(&mut ids, b, PositionKm { x: 0.0, y: 100.0 });
        g.add_lane(&mut ids, ga, gb, Ticks::from_minutes(50), LaneKind::Jump);
        let trip = worst_trip(&g, &[a, b], 500.0).unwrap();
        assert_eq!((trip.from, trip.to), (sa, sb));
        assert!((trip.duration_s - 3_400.0).abs() < 1e-9);
    }

    #[test]
    fn rejects_a_dead_end() {
        let (mut g, _) = stage1(3);
        let s = systems(&g);
        let lane = lane_between(&g, s[0], s[1]);
        remove_lane(&mut g, lane);
        let r = report(&g);
        assert!(has(&r, |v| matches!(
            v,
            Violation::TooManyDeadEnds {
                dead_ends: 1,
                systems: 9,
                ..
            }
        )));
    }

    #[test]
    fn rejects_slow_lanes_and_too_long_trips() {
        let (mut g, _) = stage1(3);
        for lane in g.lanes.values_mut() {
            lane.transit = Ticks::from_minutes(89);
        }
        let r = report(&g);
        assert!(has(&r, |v| matches!(
            v,
            Violation::TransitOutOfRange { .. }
        )));
        assert!(has(&r, |v| matches!(v, Violation::TripTooLong { .. })));
    }

    #[test]
    fn rejects_neighbouring_homes() {
        let (mut g, mut ids) = stage1(3);
        let s = systems(&g);
        let ga = g.add_gate(&mut ids, s[0], PositionKm { x: 0.0, y: 190.0 });
        let gb = g.add_gate(&mut ids, s[2], PositionKm { x: 0.0, y: -190.0 });
        g.add_lane(&mut ids, ga, gb, Ticks::from_minutes(50), LaneKind::Jump);
        let r = report(&g);
        assert_eq!(r.min_home_separation_jumps, Some(1));
        assert!(has(&r, |v| matches!(
            v,
            Violation::HomesTooClose { jumps: 1, .. }
        )));
    }

    #[test]
    fn rejects_a_wide_sector() {
        let (mut g, _) = stage1(3);
        let s = systems(&g);
        // Keep only the N1 spoke: R → C → N1 → H2 → N2 → H3 → N3 is 6 jumps.
        for standard in [s[3], s[5], s[7]] {
            let lane = lane_between(&g, standard, s[8]);
            remove_lane(&mut g, lane);
        }
        let r = report(&g);
        assert_eq!(r.sectors[0].diameter_jumps, Some(6));
        assert!(has(&r, |v| matches!(
            v,
            Violation::DiameterTooLarge { jumps: 6, .. }
        )));
    }

    #[test]
    fn rejects_a_tree() {
        let (mut g, _) = stage1(3);
        let s = systems(&g);
        // Cut the ring after each standard system: what is left is a tree.
        for i in (1..8).step_by(2) {
            let lane = lane_between(&g, s[i], s[(i + 1) % 8]);
            remove_lane(&mut g, lane);
        }
        let r = report(&g);
        assert_eq!(r.sectors[0].cycle_rank, 0);
        assert!(has(&r, |v| matches!(v, Violation::NoCycle(_))));
        assert!(has(&r, |v| matches!(
            v,
            Violation::TooManyDeadEnds { dead_ends: 4, .. }
        )));
    }

    #[test]
    fn rejects_a_disconnected_galaxy() {
        let (mut g, _) = stage1(3);
        let s = systems(&g);
        let lane = lane_between(&g, s[8], s[9]);
        remove_lane(&mut g, lane);
        let r = report(&g);
        assert!(has(&r, |v| matches!(v, Violation::GalaxyDisconnected)));
        assert!(has(&r, |v| matches!(v, Violation::SectorDisconnected(_))));
        assert_eq!(r.sectors[0].diameter_jumps, None);
    }

    #[test]
    fn rejects_crowded_sites_and_wide_systems() {
        let (mut g, _) = stage1(3);
        let s = systems(&g);
        let home = g.system(s[0]).unwrap().clone();
        let p = g.site(home.sites[0]).unwrap().position;
        g.sites.get_mut(&home.sites[1]).unwrap().position = PositionKm {
            x: p.x + 30.0,
            y: p.y,
        };
        g.gates.get_mut(&home.gates[0]).unwrap().position = PositionKm { x: 500.0, y: 0.0 };
        let r = report(&g);
        assert!(has(&r, |v| matches!(
            v,
            Violation::SitesTooClose { distance_km, .. } if (distance_km - 30.0).abs() < 1e-9
        )));
        assert!(has(
            &r,
            |v| matches!(v, Violation::InSystemTripTooLong { system, .. } if *system == s[0])
        ));
    }

    #[test]
    fn rejects_wrong_counts_and_lane_kinds() {
        let (mut g, mut ids) = stage1(3);
        let s = systems(&g);
        g.add_site(&mut ids, s[0], PositionKm { x: 0.0, y: 0.0 });
        let extra: Vec<GateId> = (0..3)
            .map(|_| g.add_gate(&mut ids, s[1], PositionKm { x: 190.0, y: 0.0 }))
            .collect();
        let loop_lane = g.add_lane(
            &mut ids,
            extra[0],
            extra[1],
            Ticks::from_minutes(50),
            LaneKind::Jump,
        );
        let trunk = lane_between(&g, s[0], s[1]);
        g.lanes.get_mut(&trunk).unwrap().kind = LaneKind::Trunk;
        let r = report(&g);
        assert!(has(&r, |v| matches!(
            v,
            Violation::SiteCount { count: 8, .. }
        )));
        assert!(has(&r, |v| matches!(
            v,
            Violation::GateCount { count: 6, .. }
        )));
        assert!(has(&r, |v| *v == Violation::LaneLoop(loop_lane)));
        assert!(has(&r, |v| *v == Violation::TrunkLaneInsideSector(trunk)));
        assert!(has(
            &r,
            |v| matches!(v, Violation::TransitOutOfRange { lane, .. } if *lane == trunk)
        ));
    }

    #[test]
    fn neighbourhood_balance_boundary_is_ten_percent_of_the_poorest() {
        let mut ids = SimIdAllocator::default();
        let mut g = Galaxy::default();
        let sector = g.add_sector(&mut ids);
        let a = g.add_system(&mut ids, sector, SystemKind::Home);
        let b = g.add_system(&mut ids, sector, SystemKind::Home);
        let values = |x, y| {
            BTreeMap::from([
                (a, DepositValue::from_milli(x)),
                (b, DepositValue::from_milli(y)),
            ])
        };
        assert_eq!(neighbourhood_imbalance(&values(100_000, 110_000), 10), None);
        assert_eq!(neighbourhood_imbalance(&values(110_000, 100_000), 10), None);
        assert_eq!(
            neighbourhood_imbalance(&values(100_000, 110_001), 10),
            Some(Violation::NeighbourhoodImbalance {
                richest: b,
                richest_value: DepositValue::from_milli(110_001),
                poorest: a,
                poorest_value: DepositValue::from_milli(100_000),
            })
        );
        assert_eq!(neighbourhood_imbalance(&values(0, 0), 10), None);
        assert_eq!(neighbourhood_imbalance(&BTreeMap::new(), 10), None);
    }

    #[test]
    fn rejects_unbalanced_homes_and_crowded_deposits() {
        let (mut g, _) = stage1(3);
        let s = systems(&g);
        let r = report(&g);
        assert_eq!(r.home_neighbourhoods.len(), 4);
        // H1's neighbourhood is H1, N1, N4, H2, H4 and C, which does not include H3.
        let h1 = g.neighbourhood(s[0], 2);
        assert_eq!(h1, BTreeSet::from([s[0], s[1], s[7], s[2], s[6], s[8]]));

        for system in [s[0], s[1], s[7]] {
            let sites = g.system(system).unwrap().sites.clone();
            for site in sites {
                g.set_deposits(site, Vec::new());
            }
        }
        let site = g.system(s[8]).unwrap().sites[0];
        g.set_deposits(site, vec![Deposit::ore(Richness::Rich); 3]);
        let r = report(&g);
        assert!(has(&r, |v| matches!(
            v,
            Violation::NeighbourhoodImbalance { poorest, .. } if *poorest == s[0]
        )));
        assert!(has(&r, |v| *v == Violation::TooManyDeposits { site, count: 3 }));
    }

    #[test]
    fn rejects_broken_links() {
        let (mut g, _) = stage1(3);
        let s = systems(&g);
        let gate = g.system(s[0]).unwrap().gates[0];
        let bogus = SiteId(gate.sim_id());
        g.systems.get_mut(&s[0]).unwrap().sites.push(bogus);
        let r = report(&g);
        assert!(has(&r, |v| *v
            == Violation::BrokenLink {
                from: s[0].sim_id(),
                to: gate.sim_id()
            }));
    }
}
