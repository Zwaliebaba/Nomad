//! The stage-1 generator meets every checked invariant of Design doc 10 §4.3 for many seeds,
//! and the season seed alone decides the galaxy (D-044, D-049).

use nomad_sim::galaxy::{InvariantLimits, STAGE1_COMMANDERS, generate_stage1, validate};
use nomad_sim::snapshot::state_hash;
use nomad_sim::{CommanderId, ControlState, SeasonConfig, SystemKind};

const SEEDS: u64 = 200;
const COMMANDERS: [CommanderId; STAGE1_COMMANDERS] = [
    CommanderId(11),
    CommanderId(12),
    CommanderId(13),
    CommanderId(14),
];

fn season(seed: u64) -> SeasonConfig {
    SeasonConfig {
        seed,
        season_start_unix_ms: 1_793_923_200_000,
    }
}

#[test]
fn every_seed_passes_the_validator() {
    let limits = InvariantLimits::default();
    let mut slowest_trip_s: f64 = 0.0;
    for seed in 0..SEEDS {
        let state = generate_stage1(season(seed), COMMANDERS)
            .unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        let report = validate(state.galaxy(), &limits);
        assert!(report.is_valid(), "seed {seed}: {:?}", report.violations);

        let sector = &report.sectors[0];
        assert_eq!(sector.diameter_jumps, Some(4), "seed {seed}");
        assert_eq!(report.min_home_separation_jumps, Some(2), "seed {seed}");
        let trip = sector.worst_trip.expect("stage 1 has sites");
        slowest_trip_s = slowest_trip_s.max(trip.duration_s);

        // Four mother-station holdings, one per home system, none controlling anything yet.
        let homes: Vec<_> = state
            .holdings()
            .map(|(_, h)| state.galaxy().site(h.site).expect("site exists").system)
            .collect();
        assert_eq!(homes.len(), STAGE1_COMMANDERS);
        for home in &homes {
            let kind = state.galaxy().system(*home).expect("system exists").kind;
            assert_eq!(kind, SystemKind::Home);
        }
        assert!(
            state
                .control_states()
                .values()
                .all(|&c| c == ControlState::Open)
        );
    }
    assert!(slowest_trip_s <= 6.0 * 3_600.0);
}

#[test]
fn the_seed_alone_decides_the_galaxy() {
    let hash = |seed| state_hash(&generate_stage1(season(seed), COMMANDERS).expect("generates"));
    for seed in [0, 1, 99, u64::MAX] {
        assert_eq!(hash(seed).ok(), hash(seed).ok());
    }
    let distinct: std::collections::BTreeSet<_> =
        (0..20).map(|seed| hash(seed).expect("hashes")).collect();
    assert_eq!(distinct.len(), 20);
}
