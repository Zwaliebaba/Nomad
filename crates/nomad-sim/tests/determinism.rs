//! Determinism contract (Design doc 11 §5, D-049): the same build, snapshot and inputs give
//! bit-identical state. This test is the CI check from doc 11 §5.

use nomad_sim::snapshot::{decode, encode, state_hash};
use nomad_sim::{
    CommanderId, CommanderInput, CommanderInputKind, OperatorInput, SeasonConfig, TickInputs,
    Ticks, UniverseState, step,
};

const TICKS: u64 = 20_000; // about 33 minutes of Universe time

fn spawn_inputs() -> TickInputs {
    TickInputs {
        operator: vec![OperatorInput::SpawnDiagnostics {
            count: 500,
            mean_interval: Ticks::from_secs(5),
        }],
        commanders: vec![],
    }
}

/// Runs a season for `TICKS` ticks. If `restore_at` is set, the state is written to a
/// snapshot and read back at that tick, as the server does on restart.
fn run(seed: u64, restore_at: Option<u64>) -> u64 {
    let mut state = UniverseState::new(SeasonConfig {
        seed,
        season_start_unix_ms: 1_793_923_200_000,
    });
    step(&mut state, &spawn_inputs());
    for t in 1..TICKS {
        if Some(t) == restore_at {
            let bytes = encode(&state).expect("snapshot encodes");
            state = decode(&bytes).expect("snapshot decodes");
        }
        step(&mut state, &TickInputs::default());
    }
    state_hash(&state).expect("state hashes")
}

#[test]
fn same_seed_gives_identical_state() {
    assert_eq!(run(7, None), run(7, None));
}

#[test]
fn snapshot_restore_is_transparent() {
    assert_eq!(run(7, None), run(7, Some(TICKS / 2)));
}

#[test]
fn different_seeds_diverge() {
    assert_ne!(run(7, None), run(8, None));
}

#[test]
fn arrival_order_of_commander_inputs_does_not_matter() {
    let input = |commander, seq, kind| CommanderInput {
        commander: CommanderId(commander),
        client_seq: seq,
        kind,
    };
    let forward = vec![
        input(1, 1, CommanderInputKind::Connected),
        input(2, 1, CommanderInputKind::Connected),
        input(1, 2, CommanderInputKind::Disconnected),
    ];
    let mut reversed = forward.clone();
    reversed.reverse();

    let hash_after = |commanders: Vec<CommanderInput>| {
        let mut state = UniverseState::new(SeasonConfig {
            seed: 1,
            season_start_unix_ms: 0,
        });
        step(
            &mut state,
            &TickInputs {
                operator: vec![],
                commanders,
            },
        );
        state_hash(&state).expect("state hashes")
    };
    assert_eq!(hash_after(forward), hash_after(reversed));
}
