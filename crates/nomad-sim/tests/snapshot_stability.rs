//! Snapshot bytes are pinned (Design doc 11 §5, §8; D-048, D-049).
//!
//! The hashes below are recorded by running `nomad-harness --galaxy --seed N` and
//! `nomad-harness --seed 7 --ticks 3000 --diagnostics 500`. The state hash is FNV-1a over
//! the full snapshot encoding (magic, format and body), so an equal hash means every
//! snapshot byte is unchanged. They held unchanged through the move of the shared
//! primitives to `nomad-types` (D-086, doc 20 §3) and the S3 task 0 move of rules back into
//! `nomad-sim`.
//!
//! A change that is meant to alter the encoding bumps [`snapshot::FORMAT`] and records new
//! values here in the same commit (D-099 item 9: no migration before the first real
//! Universe). An unexplained mismatch is a determinism bug. The values below are for the
//! newest version in the [`snapshot::FORMAT`] history.
//!
//! The state carries the season's rules (D-104), so a deliberate change to the embedded rules
//! file also changes these values without a format change: the schema stays readable. Last
//! re-recorded for format 14 (S8's final tick in the season configuration and the season
//! record, after format 13's diminishing yield), with rules format 9 (S6b's siege values and
//! D-224's growing outpost hull) and D-228's peripheral slots outside the dome. The harness's
//! seasons are 8 weeks long ([`SeasonConfig::new`], D-106).

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
    (0, 0x63df_e7c5_4723_b500),
    (1, 0x9fb3_a458_f354_1e19),
    (2, 0xe6a2_76db_8e50_1d18),
    (7, 0xa742_74e0_1b93_3e76),
    (99, 0x6a39_7f8c_2af5_ac0c),
];

/// `nomad-harness --seed 7 --ticks 3000 --diagnostics 500` (mean interval 10 s).
const DIAGNOSTIC_RUN_HASH: u64 = 0x51a6_61d4_f7f8_634c;

fn season(seed: u64) -> SeasonConfig {
    SeasonConfig::new(seed, 0)
}

#[test]
fn snapshot_format_is_unchanged() {
    assert_eq!(snapshot::FORMAT, 14);
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
