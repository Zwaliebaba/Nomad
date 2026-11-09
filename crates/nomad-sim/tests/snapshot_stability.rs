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
//! re-recorded for format 17 (A1a's AI memory, outboxes, inboxes and counters, after S7's
//! format 16), with rules format 11 (A1a's AI section), and again for the D-282 and D-284
//! rules values (F 2.5, storage 5,000 A base and 1,500 A a rack, LANCER hull 1,400) with no
//! format change, and for format 18 (A1b: the attacking fleet in the inbox's damage
//! entries), the start kit as two fleets (D-286) and the Deputy's alerts and fleet memory,
//! with rules format 12 (the Deputy's timings); then for format 19 (A3's raider factions and
//! haven launch timers), with rules format 13 (the raid parameters). The harness's seasons
//! are 8 weeks long ([`SeasonConfig::new`], D-106).

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
    (0, 0xa10a_def3_691d_62cb),
    (1, 0x0506_47aa_a419_e201),
    (2, 0xda37_e2a8_5480_98bc),
    (7, 0xcc05_6e95_795b_a1f7),
    (99, 0x8a22_bc00_7042_3e80),
];

/// `nomad-harness --seed 7 --ticks 3000 --diagnostics 500` (mean interval 10 s).
const DIAGNOSTIC_RUN_HASH: u64 = 0x0956_4d89_26ca_be04;

fn season(seed: u64) -> SeasonConfig {
    SeasonConfig::new(seed, 0)
}

#[test]
fn snapshot_format_is_unchanged() {
    assert_eq!(snapshot::FORMAT, 19);
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
