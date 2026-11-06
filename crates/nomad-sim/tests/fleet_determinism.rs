//! Determinism with ships, fleets and orders (S3 plan §7; D-049): the same seed and order
//! script give the same hash, snapshot and restore mid-run change nothing, and the order in
//! which commands arrive within a tick does not matter.

use std::collections::BTreeMap;

use nomad_sim::galaxy::{STAGE1_COMMANDERS, generate_stage1};
use nomad_sim::snapshot::{decode, encode, state_hash};
use nomad_sim::{
    Command, CommanderId, CommanderInput, CommanderInputKind, FleetOrder, HoldingOrder, Location,
    Place, Queueing, SeasonConfig, SystemId, TickInputs, UniverseState, step,
};

const COMMANDERS: [CommanderId; STAGE1_COMMANDERS] = [
    CommanderId(1),
    CommanderId(2),
    CommanderId(3),
    CommanderId(4),
];
const TICKS: u64 = 6_000;

fn universe(seed: u64) -> UniverseState {
    generate_stage1(
        SeasonConfig {
            seed,
            season_start_unix_ms: 1_793_923_200_000,
        },
        COMMANDERS,
    )
    .expect("stage 1 generates")
}

fn site_location(state: &UniverseState, system: SystemId, index: usize) -> Location {
    let site = state.galaxy().system(system).expect("system").sites[index];
    let mut position = state.galaxy().site(site).expect("site").position;
    position.y += 3_000.0;
    Location { system, position }
}

/// Commands by tick.
type Script = BTreeMap<u64, Vec<CommanderInput>>;

fn add(script: &mut Script, tick: u64, commander: CommanderId, seq: u64, command: Command) {
    script.entry(tick).or_default().push(CommanderInput {
        commander,
        client_seq: seq,
        kind: CommanderInputKind::Command(command),
    });
}

/// The order script, derived from the generated Universe only. Everyone launches on the
/// first tick and splits off the MULEs; then commander 1 jumps, 2 patrols, 3 moves and
/// docks, and 4 sends a rejected launch and later retreats.
fn script(state: &UniverseState) -> Script {
    let mut script = Script::new();
    for (n, &commander) in COMMANDERS.iter().enumerate() {
        let (holding, site) = state
            .holdings()
            .find(|(_, h)| h.owner == commander)
            .map(|(id, h)| (id, h.site))
            .expect("a mother station");
        let system = state.galaxy().site(site).expect("site").system;
        let fleet = state
            .fleet_ids()
            .into_iter()
            .find(|&f| state.fleet(f).is_some_and(|v| v.owner == commander.into()))
            .expect("a start-kit fleet");
        let ships = state.fleet(fleet).expect("fleet").ships;
        let order = |order, queue| Command::Fleet {
            fleet,
            order,
            queue,
        };
        let launch = Command::Holding {
            holding,
            order: HoldingOrder::LaunchReady,
        };
        add(&mut script, 0, commander, 1, launch);
        let split = Command::CreateFleet {
            ships: ships[3..].to_vec(),
        };
        add(&mut script, 1, commander, 2, split);
        let then: Vec<(u64, FleetOrder, Queueing)> = match n {
            0 => {
                let neighbour = *state.galaxy().neighbours(system).first().expect("lanes");
                vec![(2, FleetOrder::Jump { system: neighbour }, Queueing::Replace)]
            }
            1 => {
                let route = (1..4).map(|i| site_location(state, system, i)).collect();
                vec![(2, FleetOrder::Patrol { route }, Queueing::Replace)]
            }
            2 => {
                let to = site_location(state, system, 2);
                vec![
                    (2, FleetOrder::Move { to }, Queueing::Replace),
                    (2, FleetOrder::Dock { at: holding }, Queueing::Append),
                ]
            }
            _ => vec![
                // A rejected command is part of the script too: the fleet is in space.
                (2, FleetOrder::Launch, Queueing::Replace),
                (2_500, FleetOrder::Retreat { to: None }, Queueing::Replace),
            ],
        };
        for (seq, (tick, o, queue)) in (3..).zip(then) {
            add(&mut script, tick, commander, seq, order(o, queue));
        }
    }
    script
}

/// Runs the script for `TICKS` ticks; restores from a snapshot at each tick in `restore_at`;
/// with `reverse`, every tick's commands arrive in reverse order. Returns the final hash.
fn run(seed: u64, restore_at: &[u64], reverse: bool) -> u64 {
    let mut state = universe(seed);
    let script = script(&state);
    for t in 0..TICKS {
        if restore_at.contains(&t) {
            let bytes = encode(&state).expect("snapshot encodes");
            state = decode(&bytes).expect("snapshot decodes");
            assert_eq!(
                encode(&state).expect("snapshot encodes"),
                bytes,
                "canonical"
            );
        }
        let mut commanders = script.get(&t).cloned().unwrap_or_default();
        if reverse {
            commanders.reverse();
        }
        step(
            &mut state,
            &TickInputs {
                operator: vec![],
                commanders,
            },
        );
    }
    state_hash(&state).expect("state hashes")
}

#[test]
fn the_script_does_what_it_says() {
    let mut state = universe(7);
    let script = script(&state);
    let mut rejected = 0;
    let mut in_transit = false;
    for t in 0..TICKS {
        let commanders = script.get(&t).cloned().unwrap_or_default();
        let out = step(
            &mut state,
            &TickInputs {
                operator: vec![],
                commanders,
            },
        );
        rejected += out.commands.iter().filter(|c| c.outcome.is_err()).count();
        in_transit |= state.fleet_ids().into_iter().any(|f| {
            matches!(
                state.fleet(f).map(|v| v.place),
                Some(Place::InTransit { .. })
            )
        });
    }
    assert_eq!(
        rejected, 1,
        "only the launch of a fleet in space is rejected"
    );
    assert!(in_transit, "commander 1's fleet jumped");
    assert_eq!(
        state.fleet_ids().len(),
        8,
        "every commander split off the MULEs"
    );
}

#[test]
fn the_same_seed_and_script_give_the_same_hash() {
    assert_eq!(run(7, &[], false), run(7, &[], false));
    assert_ne!(run(7, &[], false), run(8, &[], false));
}

#[test]
fn snapshot_and_restore_mid_run_change_nothing() {
    // Before the launch, during the spool-up, in transit and late in the run.
    assert_eq!(
        run(7, &[], false),
        run(7, &[0, 3, 900, 3_001, 5_999], false)
    );
}

#[test]
fn the_order_in_which_commands_arrive_does_not_matter() {
    assert_eq!(run(7, &[], false), run(7, &[], true));
}
