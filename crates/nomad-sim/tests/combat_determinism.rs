//! Determinism with ship combat (S5 plan §8; D-049): the same seed and script give the same
//! hash, snapshot and restore mid-battle (projectiles in flight, locks building) change
//! nothing, and the order in which commands arrive within a tick does not matter.

use nomad_sim::galaxy::{STAGE1_COMMANDERS, generate_stage1};
use nomad_sim::scenario::place_fleet;
use nomad_sim::snapshot::{decode, encode, state_hash};
use nomad_sim::{
    CombatEvent, Command, CommanderId, CommanderInput, CommanderInputKind, FleetId, FleetOrder,
    Location, Owner, Queueing, RulesOfEngagement, SeasonConfig, ShipClass, SystemId, TickInputs,
    UniverseState, Vec2, step,
};

const VX: ShipClass = ShipClass::Vx;
const LANCER: ShipClass = ShipClass::Lancer;
const WARDEN: ShipClass = ShipClass::Warden;
const MULE: ShipClass = ShipClass::Mule;

const COMMANDERS: [CommanderId; STAGE1_COMMANDERS] = [
    CommanderId(1),
    CommanderId(2),
    CommanderId(3),
    CommanderId(4),
];
const TICKS: u64 = 2_400;

/// Two mixed fleets of commanders 1 and 2, 4 km apart near a site of commander 1's home
/// system, and a third fleet of commander 3 that attack-moves through the fight.
struct Battle {
    state: UniverseState,
    system: SystemId,
    fleets: [FleetId; 3],
    centre: Vec2,
}

fn battle(seed: u64) -> Battle {
    let mut state = generate_stage1(SeasonConfig::new(seed, 1_793_923_200_000), COMMANDERS)
        .expect("stage 1 generates");
    let home = state
        .holdings()
        .find(|(_, h)| h.owner == CommanderId(1))
        .map(|(_, h)| h.site)
        .expect("a mother station");
    let site = state.galaxy().site(home).expect("site");
    let system = site.system;
    let centre = Vec2::from(site.position) + Vec2::new(9_000.0, 0.0);
    let owner = |n| Owner::Commander(CommanderId(n));
    let a = place_fleet(
        &mut state,
        owner(1),
        system,
        centre,
        0.0,
        &[VX, VX, VX, LANCER, LANCER, WARDEN],
    );
    let b = place_fleet(
        &mut state,
        owner(2),
        system,
        centre + Vec2::new(4_000.0, 0.0),
        std::f64::consts::PI,
        &[VX, VX, LANCER, LANCER, WARDEN, MULE],
    );
    let c = place_fleet(
        &mut state,
        owner(3),
        system,
        centre + Vec2::new(2_000.0, -6_000.0),
        0.0,
        &[VX, VX, LANCER],
    );
    Battle {
        state,
        system,
        fleets: [a, b, c],
        centre,
    }
}

/// The commands of tick 0: 1 attacks 2, 2 attack-moves at 1, 3 attack-moves north through
/// the fight on return fire.
fn orders(b: &Battle) -> Vec<CommanderInput> {
    let [a, f2, f3] = b.fleets;
    let input = |n: u32, seq: u64, fleet, order| CommanderInput {
        commander: CommanderId(n),
        client_seq: seq,
        kind: CommanderInputKind::Command(Command::Fleet {
            fleet,
            order,
            queue: Queueing::Replace,
        }),
    };
    let at = |p: Vec2| Location {
        system: b.system,
        position: p.into(),
    };
    vec![
        input(
            1,
            1,
            a,
            FleetOrder::Attack {
                target: f2.sim_id(),
            },
        ),
        input(2, 1, f2, FleetOrder::AttackMove { to: at(b.centre) }),
        input(
            3,
            1,
            f3,
            FleetOrder::SetRulesOfEngagement {
                rules: RulesOfEngagement::ReturnFire,
            },
        ),
        input(
            3,
            2,
            f3,
            FleetOrder::AttackMove {
                to: at(b.centre + Vec2::new(2_000.0, 6_000.0)),
            },
        ),
    ]
}

/// Runs the battle for `TICKS`, restoring from a snapshot before each tick in `restore_at`,
/// and reversing the order of the tick-0 commands if `reverse`. Returns the final hash and
/// the number of ships destroyed.
fn run(seed: u64, restore_at: &[u64], reverse: bool) -> (u64, usize) {
    let mut b = battle(seed);
    let mut first = orders(&b);
    if reverse {
        first.reverse();
    }
    let mut destroyed = 0;
    for t in 0..TICKS {
        if restore_at.contains(&t) {
            let bytes = encode(&b.state).expect("encodes");
            let restored = decode(&bytes).expect("decodes");
            assert_eq!(restored, b.state, "restore at tick {t}");
            assert_eq!(encode(&restored).expect("encodes"), bytes);
            b.state = restored;
        }
        let inputs = TickInputs {
            operator: vec![],
            commanders: if t == 0 { first.clone() } else { vec![] },
        };
        let out = step(&mut b.state, &inputs);
        if t == 0 {
            assert!(
                out.commands.iter().all(|c| c.outcome.is_ok()),
                "{:?}",
                out.commands
            );
        }
        destroyed += out
            .combat_events
            .iter()
            .filter(|e| matches!(e, CombatEvent::ShipDestroyed { .. }))
            .count();
        assert!(
            b.state.fleet_problems().is_empty(),
            "tick {t}: {:?}",
            b.state.fleet_problems()
        );
    }
    (state_hash(&b.state).expect("hashes"), destroyed)
}

#[test]
fn the_same_battle_gives_the_same_hash() {
    let (hash, destroyed) = run(11, &[], false);
    assert!(destroyed > 0, "the battle destroyed ships");
    assert_eq!(run(11, &[], false), (hash, destroyed));
    // Another seed scatters other shots.
    assert_ne!(run(12, &[], false).0, hash);
}

#[test]
fn snapshot_and_restore_mid_battle_change_nothing() {
    let reference = run(11, &[], false);
    // Before the first shot, while projectiles fly and locks build, and late in the fight.
    assert_eq!(run(11, &[1, 25, 26, 301, 977, 1_800], false), reference);
}

#[test]
fn command_order_within_a_tick_does_not_matter() {
    assert_eq!(run(11, &[], true), run(11, &[], false));
}
