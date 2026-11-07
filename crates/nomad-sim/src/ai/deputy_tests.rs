//! The Deputy inside the step: rebuilding through the outbox and the validator (A1 plan
//! §5.7; D-208).

use nomad_ai::{CommandOutcome, DeferReason, DeputyEvent, InboxEntry, InboxEvent, Layer, Rebuild};
use nomad_types::{
    BuildItem, Command, CommanderId, DoctrineCommand, EconomyCommand, FleetOrder, ModuleKind,
    Queueing, ReplacementEntry, SimId, Slot,
};

use crate::combat::CombatEvent;
use crate::economy::Stock;
use crate::fleet::{ShipClass, ShipId};
use crate::geometry::Vec2;
use crate::holding::HoldingId;
use crate::input::TickInputs;
use crate::state::UniverseState;
use crate::step::{TickOutputs, step};
use crate::testkit::{commander, home, idle, kit_fleet, send, site_position, spawn_fleet, stage1};

/// Gives commander `n` an income over the last hour, as an hour of production would.
fn income(state: &mut UniverseState, n: u32, alloy: u64) {
    let stats = state.economy.stats.entry(CommanderId(n)).or_default();
    stats.minutes.push_back((0, Stock::units(alloy, 0)));
}

/// The VX builds queued at `holding`.
fn queued_vx(state: &UniverseState, holding: HoldingId) -> usize {
    state
        .holding_economy(holding)
        .unwrap()
        .queue
        .iter()
        .filter(|q| {
            matches!(
                q.item,
                BuildItem::Ship {
                    class: ShipClass::Vx
                }
            )
        })
        .count()
}

fn keep(fleet: crate::fleet::FleetId, count: u32) -> Command {
    Command::Doctrine(DoctrineCommand::SetReplacement {
        entries: vec![ReplacementEntry {
            fleet,
            class: ShipClass::Vx,
            count,
            role: None,
        }],
    })
}

/// A1 plan §9 scenario 4 in part: a lost ship of a listed fleet is replaced at the mother
/// station within the budget, through the outbox and the validator.
#[test]
fn the_deputy_replaces_a_listed_loss_within_the_budget() {
    let mut state = stage1(8);
    let (mother, system) = home(&state, 1);
    let base = site_position(&state, system, 0) + Vec2::new(8_000.0, 0.0);
    let cap_wing = spawn_fleet(&mut state, commander(1), system, base, &[ShipClass::Vx; 2]);
    // 20,000 alloy an hour: a 6,000 budget at Balanced's 30%, five VX at 1,200.
    income(&mut state, 1, 20_000);
    assert_eq!(
        send(&mut state, 1, keep(cap_wing, 2)).commands[0].outcome,
        Ok(())
    );
    let raiders = spawn_fleet(
        &mut state,
        commander(2),
        system,
        base + Vec2::new(1_200.0, 0.0),
        &[ShipClass::Vx; 8],
    );
    let attack = Command::Fleet {
        fleet: raiders,
        order: FleetOrder::Attack {
            target: cap_wing.sim_id(),
        },
        queue: Queueing::Replace,
        retreat: false,
    };
    send(&mut state, 2, attack);
    let before = queued_vx(&state, mother);
    let mut first_loss = None;
    for _ in 0..900 {
        let out = step(&mut state, &TickInputs::default());
        if first_loss.is_none()
            && out
                .combat_events
                .iter()
                .any(|e| matches!(e, CombatEvent::ShipDestroyed { .. }))
        {
            first_loss = Some(out.tick);
        }
        if queued_vx(&state, mother) >= before + 2 {
            break;
        }
    }
    let first_loss = first_loss.expect("eight VX destroy two");
    // Both losses replaced, through the validator, no sooner than the human's latency.
    assert_eq!(queued_vx(&state, mother), before + 2);
    let counters = &state.ai_counters()[&(commander(1), Layer::Deputy)];
    assert!(counters.applied >= 2);
    assert_eq!(counters.rejected_total(), 0);
    let applied_at = state
        .inbox(commander(1))
        .iter()
        .find(|e| {
            matches!(
                &e.event,
                InboxEvent::Outcome {
                    command: Command::Economy(EconomyCommand::Build { .. }),
                    outcome: CommandOutcome::Applied,
                    ..
                }
            )
        })
        .map(|e| e.tick)
        .unwrap();
    assert!(applied_at.get() >= first_loss.get() + 3);
}

#[test]
fn without_income_the_replacement_is_deferred_and_reported() {
    let mut state = stage1(9);
    let (mother, _) = home(&state, 1);
    let kit = kit_fleet(&state, 1);
    // Count 4 with 3 VX in the kit: a reported loss calls for one more.
    send(&mut state, 1, keep(kit, 4));
    let now = state.tick();
    // As a tick's combat would report it (D-202).
    state
        .ai_runtime
        .inboxes
        .entry(commander(1))
        .or_default()
        .push(InboxEntry {
            tick: now,
            event: InboxEvent::ShipLost {
                ship: ShipId(SimId::from_raw(9_999_999)),
                class: ShipClass::Vx,
                fleet: kit,
                by: commander(2),
            },
        });
    let before = queued_vx(&state, mother);
    let outs = idle(&mut state, 20);
    let events: Vec<_> = outs.iter().flat_map(|o| o.deputy_events.iter()).collect();
    assert_eq!(
        events,
        vec![&(
            CommanderId(1),
            DeputyEvent::Deferred {
                what: Rebuild::Ship {
                    fleet: kit,
                    class: ShipClass::Vx
                },
                reason: DeferReason::BudgetExhausted,
            }
        )]
    );
    assert_eq!(queued_vx(&state, mother), before);
}

#[test]
fn the_deputy_rebuilds_a_destroyed_module_in_its_slot() {
    let mut state = stage1(10);
    let (mother, _) = home(&state, 1);
    income(&mut state, 1, 20_000);
    idle(&mut state, 5);
    let solar = state.economy.holdings[&mother].slots[&Slot::Solar];
    let mut events = Vec::new();
    assert!(crate::economy::destroy_module(
        &mut state,
        solar,
        &mut events
    ));
    // As the tick that destroyed it would report it (D-202).
    let now = state.tick();
    super::inbox::collect(
        &mut state,
        now,
        &TickOutputs {
            tick: now,
            economy_events: events,
            ..TickOutputs::default()
        },
    );
    idle(&mut state, 20);
    let solar_again = BuildItem::Module {
        kind: ModuleKind::Solar,
        slot: Slot::Solar,
    };
    let queue = state.holding_economy(mother).unwrap().queue;
    assert!(
        queue.iter().any(|q| q.item == solar_again),
        "the solar array is queued again in its slot"
    );
}

/// D-055, D-210 (A1 plan §9 scenario 3 in part): a convoy whose next leg passes a gate that
/// detected armed hostiles contest (D-285) holds at its last stop, and the Deputy either takes it around the gate
/// or reports the hold; it never sets out through the contested gate.
#[test]
fn a_convoy_holds_at_a_contested_gate_and_never_passes_it() {
    let mut state = stage1(7);
    let (mother, home_system) = home(&state, 1);
    // A remote holding of commander 1, outside the home system.
    let site = state
        .galaxy()
        .sites()
        .find(|(id, s)| {
            s.system != home_system
                && state.holding_at(*id).is_none()
                && state
                    .galaxy()
                    .system(s.system)
                    .is_some_and(|sys| crate::galaxy::is_claimable(sys.kind))
        })
        .map(|(id, _)| id)
        .expect("a free site elsewhere");
    let remote = state.insert_holding(site, CommanderId(1), true).unwrap();
    crate::economy::spawn_module(&mut state, remote, Slot::Core, ModuleKind::Core, None);
    let remote_system = state.galaxy().site(site).unwrap().system;
    // The gate the convoy's first leg departs from.
    let start = state.launch_point(mother).unwrap();
    let route = crate::fleet::route::plan(
        state.galaxy(),
        home_system,
        start.into(),
        crate::fleet::route::RouteEnd::System(remote_system),
        100.0,
        crate::time::Ticks::from_secs(5),
    )
    .unwrap();
    let gate = route.hops[0].from_gate;
    let gate_at = Vec2::from(state.galaxy().gate(gate).unwrap().position);
    spawn_fleet(
        &mut state,
        commander(2),
        home_system,
        gate_at + Vec2::new(1_000.0, 0.0),
        &[ShipClass::Vx; 2],
    );
    // A commander-1 picket by the gate detects them: only detected hostiles contest a gate
    // (D-285).
    spawn_fleet(
        &mut state,
        commander(1),
        home_system,
        gate_at + Vec2::new(-1_500.0, 0.0),
        &[ShipClass::Vx],
    );
    let kit = crate::testkit::kit_mules(&state, 1);
    let convoy = Command::Economy(EconomyCommand::Convoy {
        fleet: kit,
        route: vec![remote, mother],
    });
    assert_eq!(send(&mut state, 1, convoy).commands[0].outcome, Ok(()));
    let outs = idle(&mut state, 300);
    let held = outs.iter().flat_map(|o| o.economy_events.iter()).any(|e| {
        matches!(e, crate::economy::EconomyEvent::ConvoyHeld { fleet, gate: g } if *fleet == kit && *g == gate)
    });
    assert!(held, "the convoy holds for the contested gate");
    let deputy: Vec<&DeputyEvent> = outs
        .iter()
        .flat_map(|o| o.deputy_events.iter().map(|(_, e)| e))
        .collect();
    assert!(
        deputy.iter().any(|e| matches!(
            e,
            DeputyEvent::ConvoyHeld { fleet, gate: g } | DeputyEvent::ConvoyRerouted { fleet, gate: g }
                if *fleet == kit && *g == gate
        )),
        "the Deputy reroutes or reports the hold: {deputy:?}"
    );
    let through_gate = outs.iter().flat_map(|o| o.fleet_events.iter()).any(|e| {
        matches!(e, crate::fleet::FleetEvent::SpoolUpStarted { fleet, gate: g, .. } if *fleet == kit && *g == gate)
    });
    assert!(!through_gate, "it never spools up at the contested gate");
    // On this seed a detour exists: the convoy set out on it, by ordinary orders.
    if deputy
        .iter()
        .any(|e| matches!(e, DeputyEvent::ConvoyRerouted { .. }))
    {
        let view = state.fleet(kit).unwrap();
        assert!(!matches!(view.place, crate::fleet::Place::Docked(h) if h == mother));
        assert!(view.order.is_some());
        assert_eq!(
            view.direct_control, None,
            "the Deputy's orders are doctrine"
        );
    }
}
