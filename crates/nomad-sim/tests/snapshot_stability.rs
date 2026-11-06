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
//!
//! Format 3 appends the catch-up flag (one byte, `false` here) to the state (N2 plan §4.5).
//! The values were re-recorded with the same commands. With the format field set back to 2
//! and the trailing byte removed, each new encoding hashes to its format-2 value, so the
//! rest of the encoding did not change.

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
    (0, 0x1101_be9a_2cbb_2781),
    (1, 0x9f08_61cc_9353_5ec8),
    (2, 0xcd6a_1621_1c67_371f),
    (7, 0x2c9e_d6ee_6b21_5a90),
    (99, 0x55d3_808d_f94b_11fb),
];

/// `nomad-harness --seed 7 --ticks 3000 --diagnostics 500` (mean interval 10 s).
const DIAGNOSTIC_RUN_HASH: u64 = 0x84b3_8ea0_6910_b3f4;

fn season(seed: u64) -> SeasonConfig {
    SeasonConfig {
        seed,
        season_start_unix_ms: 0,
    }
}

#[test]
fn snapshot_format_is_unchanged() {
    assert_eq!(snapshot::FORMAT, 3);
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
