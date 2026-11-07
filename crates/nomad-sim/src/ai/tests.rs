//! The outbox, the competence gate, the inbox and the counters (A1 plan §4.3, §4.4, §4.8,
//! §9 "Competence", "Fairness" and "Determinism").

use nomad_ai::{CommandOutcome, DropReason, InboxEvent, Layer};
use nomad_types::{
    Command, CommanderId, Difficulty, FleetOrder, Owner, Queueing, RulesOfEngagement,
};

use super::gate::submit_for_test;
use crate::command::CommandRejection;
use crate::commander::{CommanderSetup, DEFAULT_WINDOW_START};
use crate::fleet::ShipClass;
use crate::galaxy::generate_stage1_roster;
use crate::geometry::Vec2;
use crate::input::{CommanderInput, CommanderInputKind, TickInputs};
use crate::rng::{RngStreams, StreamKey};
use crate::rules::Rules;
use crate::snapshot::{decode, encode, state_hash};
use crate::state::{SeasonConfig, UniverseState};
use crate::step::step;
use crate::testkit::{commander, home, idle, kit_fleet, spawn_fleet, stage1};
use crate::time::Tick;

/// Commander 1 is human, 2 to 4 are AI at Easy, Normal and Hard.
fn mixed(seed: u64) -> UniverseState {
    generate_stage1_roster(
        SeasonConfig::new(seed, 0),
        [
            CommanderSetup::human(CommanderId(1), DEFAULT_WINDOW_START),
            CommanderSetup::ai_at(CommanderId(2), Difficulty::Easy),
            CommanderSetup::ai_at(CommanderId(3), Difficulty::Normal),
            CommanderSetup::ai_at(CommanderId(4), Difficulty::Hard),
        ],
        Rules::embedded(),
    )
    .expect("stage 1 generates")
}

/// A command that is valid any time: the start kit's rules of engagement.
fn harmless(state: &UniverseState, n: u32) -> Command {
    Command::Fleet {
        fleet: kit_fleet(state, n),
        order: FleetOrder::SetRulesOfEngagement {
            rules: RulesOfEngagement::WeaponsFree,
        },
        queue: Queueing::Replace,
        retreat: false,
    }
}

/// The ticks on which `owner`'s AI commands had an outcome, with the outcome.
fn outcomes(state: &UniverseState, owner: Owner) -> Vec<(Tick, CommandOutcome)> {
    state
        .inbox(owner)
        .iter()
        .filter_map(|e| match e.event {
            InboxEvent::Outcome { outcome, .. } => Some((e.tick, outcome)),
            _ => None,
        })
        .collect()
}

/// Submits as phase 4 of the next tick would: on the tick being computed next.
fn submit(state: &mut UniverseState, n: u32, layer: Layer, trigger: Tick, command: Command) {
    let now = state.tick().next();
    submit_for_test(state, commander(n), layer, now, trigger, command);
}

#[test]
fn latency_counts_from_the_trigger_and_never_applies_on_the_issue_tick() {
    let mut state = mixed(1);
    idle(&mut state, 20);
    let now = state.tick().next();
    // Commander 3 (Normal): 6 ticks from a trigger 2 ticks ago, so 4 from now.
    let trigger = Tick::new(now.get() - 2);
    let command = harmless(&state, 3);
    submit(&mut state, 3, Layer::Deputy, trigger, command);
    // Commander 1's Deputy (human): 3 ticks, but a trigger long ago still waits a tick.
    let command = harmless(&state, 1);
    submit(&mut state, 1, Layer::Deputy, Tick::new(1), command);
    idle(&mut state, 10);
    assert_eq!(
        outcomes(&state, commander(3)),
        vec![(
            trigger.after(crate::time::Ticks::new(6)),
            CommandOutcome::Applied
        )]
    );
    assert_eq!(
        outcomes(&state, commander(1)),
        vec![(now.next(), CommandOutcome::Applied)]
    );
}

#[test]
fn the_rate_holds_in_every_ten_ticks_and_expired_commands_drop() {
    let mut state = mixed(2);
    idle(&mut state, 5);
    let trigger = state.tick();
    // Commander 2 is Easy: one command in any 10 ticks; 30 commands at once.
    for _ in 0..30 {
        let command = harmless(&state, 2);
        submit(&mut state, 2, Layer::Deputy, trigger, command);
    }
    idle(&mut state, 200);
    let applied: Vec<Tick> = outcomes(&state, commander(2))
        .into_iter()
        .filter(|(_, o)| *o == CommandOutcome::Applied)
        .map(|(t, _)| t)
        .collect();
    // Due 10 ticks after the trigger, then one every 10 ticks until 10 s of waiting.
    assert_eq!(applied.len(), 10);
    assert!(applied.windows(2).all(|w| w[1].get() - w[0].get() >= 10));
    let counters = &state.ai_counters()[&(commander(2), Layer::Deputy)];
    assert_eq!(counters.issued, 30);
    assert_eq!(counters.applied, 10);
    assert_eq!(counters.dropped.get(&DropReason::Expired), Some(&20));
    assert_eq!(state.ai_waiting(commander(2)), 0);
}

#[test]
fn deputy_commands_drain_before_strategist_commands_due_on_the_same_tick() {
    let mut state = mixed(3);
    idle(&mut state, 5);
    let trigger = state.tick();
    // Commander 3 (Normal, two per window): Strategist first, then two Deputy commands.
    let command = harmless(&state, 3);
    submit(&mut state, 3, Layer::Strategist, trigger, command.clone());
    submit(&mut state, 3, Layer::Deputy, trigger, command.clone());
    submit(&mut state, 3, Layer::Deputy, trigger, command);
    idle(&mut state, 8);
    let layers: Vec<Layer> = state
        .inbox(commander(3))
        .iter()
        .filter_map(|e| match e.event {
            InboxEvent::Outcome { layer, .. } => Some(layer),
            _ => None,
        })
        .collect();
    assert_eq!(layers, vec![Layer::Deputy, Layer::Deputy]);
    assert_eq!(state.ai_waiting(commander(3)), 1);
}

#[test]
fn an_invalid_command_is_rejected_alike_from_a_client_and_from_the_outbox() {
    let mut state = mixed(4);
    idle(&mut state, 5);
    let (_, system) = home(&state, 1);
    // Commander 1 orders commander 2's fleet: not the owner.
    let rival = spawn_fleet(
        &mut state,
        commander(2),
        system,
        Vec2::ZERO,
        &[ShipClass::Vx],
    );
    let command = Command::Fleet {
        fleet: rival,
        order: FleetOrder::Launch,
        queue: Queueing::Replace,
        retreat: false,
    };
    let out = step(
        &mut state,
        &TickInputs {
            operator: vec![],
            commanders: vec![CommanderInput {
                commander: CommanderId(1),
                client_seq: 1,
                kind: CommanderInputKind::Command(command.clone()),
            }],
        },
    );
    assert_eq!(out.commands[0].outcome, Err(CommandRejection::NotOwner));
    let trigger = state.tick();
    submit(&mut state, 1, Layer::Deputy, trigger, command);
    let outs = idle(&mut state, 5);
    // AI outcomes stay out of the client answers (A1 plan §4.3).
    assert!(outs.iter().all(|o| o.commands.is_empty()));
    let counters = &state.ai_counters()[&(commander(1), Layer::Deputy)];
    assert_eq!(counters.rejected.get(&CommandRejection::NotOwner), Some(&1));
    assert_eq!(counters.applied, 0);
    assert_eq!(
        outcomes(&state, commander(1)),
        vec![(
            trigger.after(crate::time::Ticks::new(3)),
            CommandOutcome::Rejected
        )]
    );
}

/// A1 plan §9 "Competence": over random scenarios, no AI command applies before its trigger
/// plus the latency or on its issue tick, no owner exceeds its rate in any 10 ticks, and
/// every issued command is accounted for.
#[test]
fn competence_holds_over_random_scenarios() {
    const SCRIPT: StreamKey = StreamKey {
        scope: 0,
        purpose: 9_001,
    };
    for seed in 0..6 {
        let mut state = mixed(seed);
        let mut rng = RngStreams::new(seed);
        // (owner, issue tick, trigger, layer) of every submitted command, in submit order.
        let mut sent: Vec<(u32, Tick, Tick, Layer)> = Vec::new();
        for _ in 0..300 {
            let now = state.tick().next();
            for n in 1..=4 {
                if rng.below(SCRIPT, 4) == 0 {
                    let back = rng.below(SCRIPT, 12);
                    let trigger = Tick::new(now.get().saturating_sub(back));
                    let layer = if rng.below(SCRIPT, 3) == 0 {
                        Layer::Strategist
                    } else {
                        Layer::Deputy
                    };
                    let command = harmless(&state, n);
                    submit_for_test(&mut state, commander(n), layer, now, trigger, command);
                    sent.push((n, now, trigger, layer));
                }
            }
            step(&mut state, &TickInputs::default());
            for n in 1..=4u32 {
                let owner = commander(n);
                let latency = super::competence(&state, owner).latency_ticks;
                let rate = super::competence(&state, owner).commands_per_window;
                let applied: Vec<Tick> = outcomes(&state, owner)
                    .into_iter()
                    .filter(|(_, o)| !matches!(o, CommandOutcome::Dropped(_)))
                    .map(|(t, _)| t)
                    .collect();
                let mine: Vec<_> = sent.iter().filter(|s| s.0 == n).collect();
                // Every outcome follows a submit at least the latency after its trigger.
                for &t in &applied {
                    assert!(
                        mine.iter()
                            .any(|s| t.get() >= s.2.get() + latency && t > s.1),
                        "seed {seed}: commander {n} applied on {t:?} too early"
                    );
                }
                for w in applied.windows(usize::try_from(rate).unwrap() + 1) {
                    assert!(
                        w[w.len() - 1].get() - w[0].get() >= 10,
                        "seed {seed}: commander {n} beat its rate: {w:?}"
                    );
                }
            }
        }
        for n in 1..=4u32 {
            let owner = commander(n);
            let mut issued = 0;
            let mut settled = 0;
            for layer in [Layer::Deputy, Layer::Strategist] {
                if let Some(c) = state.ai_counters().get(&(owner, layer)) {
                    issued += c.issued;
                    settled += c.applied + c.rejected_total() + c.dropped_total();
                }
            }
            let waiting = u64::try_from(state.ai_waiting(owner)).unwrap();
            assert_eq!(issued, settled + waiting, "seed {seed}: commander {n}");
            assert_eq!(
                issued,
                u64::try_from(sent.iter().filter(|s| s.0 == n).count()).unwrap()
            );
        }
    }
}

#[test]
fn a_snapshot_while_commands_wait_changes_nothing() {
    let mut state = mixed(5);
    idle(&mut state, 12);
    let trigger = state.tick();
    for n in 1..=4 {
        for _ in 0..3 {
            let command = harmless(&state, n);
            submit(&mut state, n, Layer::Deputy, trigger, command);
        }
    }
    // Inside the latency of most of them.
    idle(&mut state, 2);
    assert!((1..=4).all(|n| state.ai_waiting(commander(n)) > 0));
    let mut restored = decode(&encode(&state).unwrap()).unwrap();
    assert_eq!(restored, state);
    idle(&mut state, 60);
    idle(&mut restored, 60);
    assert_eq!(state_hash(&state).unwrap(), state_hash(&restored).unwrap());
}

#[test]
fn the_inbox_tells_the_owner_of_its_losses_and_forgets_after_60_s() {
    let mut state = stage1(6);
    let (_, system) = home(&state, 1);
    // Inside an ops area, where fleets fight at tactical speed (doc 15 §3.3).
    let base = crate::testkit::site_position(&state, system, 0) + Vec2::new(8_000.0, 0.0);
    let victim = spawn_fleet(&mut state, commander(1), system, base, &[ShipClass::Vx]);
    let ship = state.fleet(victim).unwrap().ships[0];
    let attacker = spawn_fleet(
        &mut state,
        commander(2),
        system,
        base + Vec2::new(1_200.0, 0.0),
        &[ShipClass::Vx; 6],
    );
    let attack = Command::Fleet {
        fleet: attacker,
        order: FleetOrder::Attack {
            target: victim.sim_id(),
        },
        queue: Queueing::Replace,
        retreat: false,
    };
    let out = crate::testkit::send(&mut state, 2, attack);
    assert_eq!(out.commands[0].outcome, Ok(()), "the attack is accepted");
    let mut lost_at = None;
    for _ in 0..600 {
        step(&mut state, &TickInputs::default());
        if state.fleet(victim).is_none() {
            lost_at = Some(state.tick());
            break;
        }
    }
    let lost_at = lost_at.expect("six VX destroy one");
    let lost = |state: &UniverseState, owner| {
        state.inbox(owner).iter().any(|e| {
            matches!(e.event, InboxEvent::ShipLost { ship: s, fleet: f, class: ShipClass::Vx, by, .. }
                if s == ship && f == victim && by == commander(2))
        })
    };
    assert!(lost(&state, commander(1)));
    assert!(state.inbox(commander(1)).iter().any(|e| matches!(
        e.event,
        InboxEvent::ShipDamaged { ship: s, by, .. } if s == ship && by == commander(2)
    )));
    assert!(state.inbox(commander(1)).iter().any(|e| matches!(
        e.event,
        InboxEvent::FleetLost { fleet } if fleet == victim
    )));
    // The attacker learns nothing of it through the inbox.
    assert!(!lost(&state, commander(2)));
    // 60 s later it is gone.
    idle(&mut state, 600);
    assert!(!lost(&state, commander(1)));
    assert!(state.inbox(commander(1)).iter().all(|e| e.tick > lost_at));
}
