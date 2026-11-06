//! Snapshot bytes are pinned (Design doc 11 §5, §8; D-048, D-049).
//!
//! The hashes below were recorded on commit `2f7b44f`, before the shared primitives moved to
//! `nomad-types` (D-086, doc 20 §3), by running `nomad-harness --galaxy --seed N` and
//! `nomad-harness --seed 7 --ticks 3000 --diagnostics 500`. The state hash is FNV-1a over
//! the full snapshot encoding (magic, format and body), so an equal hash means the move
//! left every snapshot byte unchanged.
//!
//! A change that is meant to alter the encoding bumps [`snapshot::FORMAT`] and records new
//! values here in the same commit (D-099 item 9: no migration before the first real
//! Universe). An unexplained mismatch is a determinism bug.

use nomad_sim::galaxy::{STAGE1_COMMANDERS, generate_stage1};
use nomad_sim::snapshot::{self, state_hash};
use nomad_sim::{CommanderId, OperatorInput, SeasonConfig, TickInputs, Ticks, UniverseState, step};

/// The commanders `nomad-harness --galaxy` generates.
const COMMANDERS: [CommanderId; STAGE1_COMMANDERS] = [
    CommanderId(1),
    CommanderId(2),
    CommanderId(3),
    CommanderId(4),
];

/// `(seed, state hash)` of the generated stage-1 Universe, as `nomad-harness --galaxy` prints.
const STAGE1_HASHES: [(u64, u64); 5] = [
    (0, 0xb47b_9e18_0c9d_a538),
    (1, 0xfa51_0dc4_d4ab_ab83),
    (2, 0x41fa_d8f1_d8f0_c47e),
    (7, 0xa8c1_b7d6_83de_bfcf),
    (99, 0xbff5_547b_2a01_e8e2),
];

/// `nomad-harness --seed 7 --ticks 3000 --diagnostics 500` (mean interval 10 s).
const DIAGNOSTIC_RUN_HASH: u64 = 0x27a2_ec5f_78c6_022b;

fn season(seed: u64) -> SeasonConfig {
    SeasonConfig {
        seed,
        season_start_unix_ms: 0,
    }
}

#[test]
fn snapshot_format_is_unchanged() {
    assert_eq!(snapshot::FORMAT, 2);
}

#[test]
fn generated_universe_bytes_are_unchanged() {
    for (seed, expected) in STAGE1_HASHES {
        let state = generate_stage1(season(seed), COMMANDERS).expect("stage-1 galaxy generates");
        let hash = state_hash(&state).expect("state hashes");
        assert_eq!(
            hash, expected,
            "seed {seed}: state hash {hash:016x}, expected {expected:016x}"
        );
    }
}

#[test]
fn diagnostic_run_bytes_are_unchanged() {
    // The same loop as the harness: diagnostics spawn on the first tick, then 2,999 more.
    let mut state = UniverseState::new(season(7));
    let spawn = TickInputs {
        operator: vec![OperatorInput::SpawnDiagnostics {
            count: 500,
            mean_interval: Ticks::from_secs(10),
        }],
        commanders: vec![],
    };
    step(&mut state, &spawn);
    for _ in 1..3_000 {
        step(&mut state, &TickInputs::default());
    }
    let hash = state_hash(&state).expect("state hashes");
    assert_eq!(
        hash, DIAGNOSTIC_RUN_HASH,
        "state hash {hash:016x}, expected {DIAGNOSTIC_RUN_HASH:016x}"
    );
}
