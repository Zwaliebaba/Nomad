//! The stage-1 generator meets every checked invariant of Design doc 10 §4.3 for many seeds,
//! follows the deposit distribution of doc 14 §5 (D-066), and the season seed alone decides
//! the galaxy (D-044, D-049).

use nomad_sim::galaxy::{
    Deposit, DepositKind, InvariantLimits, MAX_DEPOSITS_PER_SITE, Richness, STAGE1_COMMANDERS,
    generate_stage1, validate,
};
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
    SeasonConfig::new(seed, 1_793_923_200_000)
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

#[test]
fn deposits_follow_doc_14_for_every_seed() {
    for seed in 0..SEEDS {
        let state = generate_stage1(season(seed), COMMANDERS).expect("generates");
        let galaxy = state.galaxy();
        let mother_sites: Vec<_> = state.holdings().map(|(_, h)| h.site).collect();
        for (id, system) in galaxy.systems() {
            let deposits: Vec<Deposit> = system
                .sites
                .iter()
                .flat_map(|&s| galaxy.site(s).expect("site exists").deposits.clone())
                .collect();
            let veins: Vec<Richness> = deposits
                .iter()
                .filter(|d| d.kind == DepositKind::CrystalVein)
                .map(|d| d.richness)
                .collect();
            let ore = deposits.iter().filter(|d| d.kind == DepositKind::OreField);
            for &site in &system.sites {
                let on_site = &galaxy.site(site).expect("site exists").deposits;
                assert!(on_site.len() <= MAX_DEPOSITS_PER_SITE, "seed {seed}");
                let site_veins = on_site
                    .iter()
                    .filter(|d| d.kind == DepositKind::CrystalVein)
                    .count();
                assert!(site_veins <= 1, "seed {seed}: one vein per site");
                if mother_sites.contains(&site) {
                    assert_eq!(on_site, &vec![Deposit::ore(Richness::Standard)]);
                }
            }
            match system.kind {
                SystemKind::Home => {
                    assert_eq!(
                        veins.len(),
                        1,
                        "seed {seed} {id:?}: at most 1, here exactly 1"
                    );
                    let modest = |r: &Richness| matches!(r, Richness::Poor | Richness::Standard);
                    assert!(veins.iter().all(modest));
                    assert!(ore.map(|d| d.richness).all(|r| modest(&r)));
                }
                SystemKind::Standard => {
                    assert!((1..=2).contains(&veins.len()), "seed {seed} {id:?}");
                    assert!(deposits.iter().all(|d| d.richness == Richness::Standard));
                }
                SystemKind::Hub => {
                    assert!(!veins.is_empty(), "seed {seed}: the hub has crystal");
                    assert!(deposits.iter().all(|d| d.richness == Richness::Rich));
                }
                SystemKind::RaiderHaven => assert!(deposits.is_empty()),
            }
        }
        // Invariant 5, re-stated from the values: richest − poorest ≤ 10 % of the poorest.
        let values: Vec<u64> = galaxy
            .home_neighbourhood_values()
            .values()
            .map(|v| v.milli())
            .collect();
        assert_eq!(values.len(), STAGE1_COMMANDERS);
        let (lo, hi) = (values.iter().min(), values.iter().max());
        let (lo, hi) = (*lo.expect("four homes"), *hi.expect("four homes"));
        assert!((hi - lo) * 10 <= lo, "seed {seed}: {values:?}");
    }
}
