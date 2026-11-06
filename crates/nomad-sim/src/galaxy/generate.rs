//! The MVP stage-1 sector generator (Design doc 10 §4.4; D-043, D-044).
//!
//! One sector: four home systems on a ring with a standard system between each pair, a hub
//! linked to all four standard systems, and a raider haven linked to the hub. Systems are
//! created in the order H1, N1, H2, N2, H3, N3, H4, N4, C, R, so their identities follow
//! that order (doc 10 §4.4 diagram).
//!
//! Everything random is drawn from seeded streams with purpose
//! [`purpose::GALAXY_LAYOUT`]: per system for site and gate positions, Universe-wide for lane
//! transit times. The same season seed therefore gives the same galaxy (D-044, D-049).

use std::collections::VecDeque;
use std::f64::consts::TAU;

use thiserror::Error;

use crate::galaxy::{GateId, LaneKind, PositionKm, SystemId, SystemKind};
use crate::holding::HoldingError;
use crate::id::CommanderId;
use crate::rng::{RngStreams, StreamKey, purpose};
use crate::state::{SeasonConfig, UniverseState};
use crate::time::Ticks;

/// Commanders in MVP stage 1: 1 human and 3 AI, one per home system (doc 10 §3, D-038).
pub const STAGE1_COMMANDERS: usize = 4;

/// Systems on the ring: home and standard systems alternate (H1, N1, H2, …, N4).
const RING: usize = 2 * STAGE1_COMMANDERS;
/// Index of the hub in creation order.
const HUB: usize = RING;
/// Index of the raider haven in creation order.
const HAVEN: usize = RING + 1;

// Layout parameters. These are generator choices, not game rules: they are picked to satisfy
// the agreed constraints, and the validator checks the result.
//
// - Sites sit on two orbits (doc 10 §5 "orbit and angle"), gates on a ring at the edge
//   (doc 10 §4.1). Orbits and the edge are at least `PLACEMENT_SPACING_KM` apart radially,
//   and sites on one orbit are at least that far apart along the chord, so the 50 km
//   minimum site spacing (doc 10 §8) holds by construction with a 5 km margin.
// - The widest span is gate to gate across the system: at most 2 × 195 km = 390 km, which
//   takes 13 min at the slowest warship cruise speed of 500 m/s (doc 15 §12). That keeps
//   every in-system trip within 15 min (doc 10 §4.1).
// - The worst case of invariant 3 is then at most 4 × 70 min of transit plus two
//   site-to-gate legs (≤ 325 km each) and three gate-to-gate legs (≤ 390 km each), about
//   5.7 h, inside the 6 h limit (doc 10 §4.3).

/// Inner orbit radius range, km. Keeps sites well clear of the star.
const INNER_ORBIT_KM: (f64, f64) = (55.0, 65.0);
/// Outer orbit radius range, km.
const OUTER_ORBIT_KM: (f64, f64) = (120.0, 130.0);
/// Gate ring radius range, km: the system edge.
const EDGE_KM: (f64, f64) = (185.0, 195.0);
/// Minimum distance the generator keeps between sites: doc 10 §8's 50 km plus a margin.
const PLACEMENT_SPACING_KM: f64 = 55.0;
/// At most this many sites on the inner orbit (it has room for 3 at 55 km spacing).
const INNER_ORBIT_MAX_SITES: usize = 3;
/// Minimum angle between two gates of one system, so gates do not bunch up.
const GATE_MIN_SEPARATION_RAD: f64 = TAU / 6.0;
/// Jump lane transit range in whole minutes (doc 10 §4.2, D-043).
const JUMP_TRANSIT_MINUTES: (u64, u64) = (40, 70);

/// Why a galaxy could not be generated.
#[derive(Clone, Debug, PartialEq, Error)]
pub enum GenerationError {
    /// The same commander was given twice.
    #[error("commander {0:?} was given more than once")]
    DuplicateCommander(CommanderId),
    /// A mother-station holding could not be placed.
    #[error("could not place a mother-station holding: {0}")]
    Holding(#[from] HoldingError),
}

/// Number of sites the stage-1 generator gives a system of `kind` (doc 10 §4.4, D-043).
///
/// The raider haven has no sites: doc 10 counts none for it, and what raiders operate from
/// is not designed yet.
pub const fn stage1_sites(kind: SystemKind) -> usize {
    match kind {
        SystemKind::Home => 7,
        SystemKind::Standard => 6,
        SystemKind::Hub => 8,
        SystemKind::RaiderHaven => 0,
    }
}

/// Generates a new MVP stage-1 Universe for `season` (doc 10 §4.4).
///
/// `commanders[i]` gets a mother-station holding, with its core complete, on the first site
/// of home system H(i+1).
pub fn generate_stage1(
    season: SeasonConfig,
    commanders: [CommanderId; STAGE1_COMMANDERS],
) -> Result<UniverseState, GenerationError> {
    for (i, commander) in commanders.iter().enumerate() {
        if commanders[..i].contains(commander) {
            return Err(GenerationError::DuplicateCommander(*commander));
        }
    }

    let mut state = UniverseState::new(season);
    let UniverseState {
        ids, rng, galaxy, ..
    } = &mut state;

    let sector = galaxy.add_sector(ids);
    let kinds = stage1_kinds();
    let systems: Vec<SystemId> = kinds
        .iter()
        .map(|&kind| galaxy.add_system(ids, sector, kind))
        .collect();

    let plan = stage1_lanes();
    let mut free_gates: Vec<VecDeque<GateId>> = Vec::with_capacity(systems.len());
    for (index, (&system, &kind)) in systems.iter().zip(&kinds).enumerate() {
        let degree = plan
            .iter()
            .filter(|&&(a, b)| a == index || b == index)
            .count();
        let layout = draw_layout(rng, system, stage1_sites(kind), degree);
        for position in layout.sites {
            galaxy.add_site(ids, system, position);
        }
        free_gates.push(
            layout
                .gates
                .into_iter()
                .map(|position| galaxy.add_gate(ids, system, position))
                .collect(),
        );
    }

    let transit_key = StreamKey::global(purpose::GALAXY_LAYOUT);
    let (lo, hi) = JUMP_TRANSIT_MINUTES;
    for &(a, b) in &plan {
        let mut take = |index: usize| {
            free_gates[index]
                .pop_front()
                .expect("invariant: each system has one gate per lane it is on")
        };
        let (gate_a, gate_b) = (take(a), take(b));
        let minutes = lo + rng.below(transit_key, hi - lo + 1);
        galaxy.add_lane(
            ids,
            gate_a,
            gate_b,
            Ticks::from_minutes(minutes),
            LaneKind::Jump,
        );
    }

    let homes = systems
        .iter()
        .zip(&kinds)
        .filter(|&(_, &kind)| kind == SystemKind::Home)
        .map(|(&system, _)| system);
    let mother_sites: Vec<_> = homes
        .filter_map(|home| state.galaxy.system(home)?.sites.first().copied())
        .collect();
    for (site, commander) in mother_sites.into_iter().zip(commanders) {
        state.insert_holding(site, commander, true)?;
    }
    Ok(state)
}

/// System kinds in creation order: H1, N1, …, H4, N4, C, R.
fn stage1_kinds() -> Vec<SystemKind> {
    let ring = (0..RING).map(|i| {
        if i % 2 == 0 {
            SystemKind::Home
        } else {
            SystemKind::Standard
        }
    });
    ring.chain([SystemKind::Hub, SystemKind::RaiderHaven])
        .collect()
}

/// Jump lanes as pairs of creation indices: the ring, the hub spokes to the standard
/// systems, and the haven link (doc 10 §4.4 diagram). 13 lanes.
fn stage1_lanes() -> Vec<(usize, usize)> {
    let mut lanes: Vec<(usize, usize)> = (0..RING).map(|i| (i, (i + 1) % RING)).collect();
    lanes.extend((1..RING).step_by(2).map(|standard| (standard, HUB)));
    lanes.push((HUB, HAVEN));
    lanes
}

struct Layout {
    sites: Vec<PositionKm>,
    gates: Vec<PositionKm>,
}

fn draw_layout(rng: &mut RngStreams, system: SystemId, sites: usize, gates: usize) -> Layout {
    let key = StreamKey {
        scope: system.sim_id().get(),
        purpose: purpose::GALAXY_LAYOUT,
    };
    let mut positions = Vec::with_capacity(sites);
    if sites > 0 {
        let inner = 1 + rng.index(key, sites.min(INNER_ORBIT_MAX_SITES));
        for (count, band) in [(inner, INNER_ORBIT_KM), (sites - inner, OUTER_ORBIT_KM)] {
            let radius_km = between(rng, key, band);
            let min_angle = chord_angle_rad(radius_km, PLACEMENT_SPACING_KM);
            positions.extend(
                spaced_angles(rng, key, count, min_angle)
                    .into_iter()
                    .map(|angle| PositionKm::from_polar(radius_km, angle)),
            );
        }
        // Which site lands on which orbit should not follow identity order.
        for i in (1..positions.len()).rev() {
            positions.swap(i, rng.index(key, i + 1));
        }
    }
    let edge_km = between(rng, key, EDGE_KM);
    let gates = spaced_angles(rng, key, gates, GATE_MIN_SEPARATION_RAD)
        .into_iter()
        .map(|angle| PositionKm::from_polar(edge_km, angle))
        .collect();
    Layout {
        sites: positions,
        gates,
    }
}

/// A uniform value in `[lo, hi)`.
fn between(rng: &mut RngStreams, key: StreamKey, (lo, hi): (f64, f64)) -> f64 {
    lo + rng.unit_f64(key) * (hi - lo)
}

/// The angle between two points on a circle of `radius_km` that are `chord_km` apart.
fn chord_angle_rad(radius_km: f64, chord_km: f64) -> f64 {
    2.0 * libm::asin((chord_km / (2.0 * radius_km)).min(1.0))
}

/// `count` random angles, each at least `min_gap_rad` from its neighbours around the circle.
///
/// The free angle left over after the minimum gaps is cut at random points, and one minimum
/// gap is added per angle, so every gap, including the one that wraps around, is at least
/// `min_gap_rad`.
fn spaced_angles(rng: &mut RngStreams, key: StreamKey, count: usize, min_gap_rad: f64) -> Vec<f64> {
    if count == 0 {
        return Vec::new();
    }
    let slack = (TAU - count as f64 * min_gap_rad).max(0.0);
    let offset = rng.unit_f64(key) * TAU;
    let mut cuts: Vec<f64> = (0..count).map(|_| rng.unit_f64(key) * slack).collect();
    cuts.sort_by(f64::total_cmp);
    cuts.iter()
        .enumerate()
        .map(|(i, cut)| offset + cut + i as f64 * min_gap_rad)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ControlState;
    use crate::snapshot::state_hash;

    const COMMANDERS: [CommanderId; STAGE1_COMMANDERS] = [
        CommanderId(1),
        CommanderId(2),
        CommanderId(3),
        CommanderId(4),
    ];

    fn season(seed: u64) -> SeasonConfig {
        SeasonConfig {
            seed,
            season_start_unix_ms: 0,
        }
    }

    #[test]
    fn stage1_shape_matches_doc_10() {
        let state = generate_stage1(season(1), COMMANDERS).unwrap();
        let g = state.galaxy();
        assert_eq!(g.sectors().count(), 1);
        let kinds: Vec<SystemKind> = g.systems().map(|(_, s)| s.kind).collect();
        assert_eq!(kinds, stage1_kinds());
        assert_eq!(g.sites().count(), 60);
        assert_eq!(g.lanes().count(), 13);
        assert_eq!(g.gates().count(), 26);
        for (_, system) in g.systems() {
            assert_eq!(system.sites.len(), stage1_sites(system.kind));
            let gates = match system.kind {
                SystemKind::Home => 2,
                SystemKind::Standard => 3,
                SystemKind::Hub => 5,
                SystemKind::RaiderHaven => 1,
            };
            assert_eq!(system.gates.len(), gates);
        }
        for (_, lane) in g.lanes() {
            let minutes = lane.transit.get() / Ticks::from_minutes(1).get();
            assert!((40..=70).contains(&minutes));
            assert_eq!(lane.transit, Ticks::from_minutes(minutes));
        }
    }

    #[test]
    fn each_commander_controls_nothing_yet_but_holds_a_home_site() {
        let state = generate_stage1(season(2), COMMANDERS).unwrap();
        let holdings: Vec<_> = state.holdings().map(|(_, h)| h.clone()).collect();
        assert_eq!(holdings.len(), STAGE1_COMMANDERS);
        for (holding, commander) in holdings.iter().zip(COMMANDERS) {
            assert_eq!(holding.owner, commander);
            assert!(holding.core_complete);
            let system = state.galaxy().site(holding.site).unwrap().system;
            assert_eq!(
                state.galaxy().system(system).unwrap().kind,
                SystemKind::Home
            );
        }
        // One holding of seven sites is no majority, and there is one owner per home.
        let states = state.control_states();
        assert_eq!(states.len(), 9);
        assert!(states.values().all(|&s| s == ControlState::Open));
    }

    #[test]
    fn same_seed_same_galaxy_different_seed_different_galaxy() {
        let a = state_hash(&generate_stage1(season(9), COMMANDERS).unwrap()).unwrap();
        let b = state_hash(&generate_stage1(season(9), COMMANDERS).unwrap()).unwrap();
        let c = state_hash(&generate_stage1(season(10), COMMANDERS).unwrap()).unwrap();
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn rejects_duplicate_commanders() {
        let twice = [
            CommanderId(1),
            CommanderId(2),
            CommanderId(1),
            CommanderId(4),
        ];
        assert_eq!(
            generate_stage1(season(1), twice).unwrap_err(),
            GenerationError::DuplicateCommander(CommanderId(1))
        );
    }

    #[test]
    fn spaced_angles_keep_their_gap_including_wrap_around() {
        let mut rng = RngStreams::new(4);
        let key = StreamKey::global(purpose::GALAXY_LAYOUT);
        for count in 1..=6 {
            let gap = TAU / 6.0;
            let angles = spaced_angles(&mut rng, key, count, gap);
            assert_eq!(angles.len(), count);
            for pair in angles.windows(2) {
                assert!(pair[1] - pair[0] >= gap - 1e-12);
            }
            let wrap = angles[0] + TAU - angles[count - 1];
            assert!(wrap >= gap - 1e-12);
        }
    }
}
