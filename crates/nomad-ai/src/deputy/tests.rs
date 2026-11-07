//! The Deputy against a fake view: rebuilding and the budget (A1 plan §5.7, D-208).

use nomad_types::{
    BuildItem, Command, EconomyCommand, FleetId, FleetOrder, FleetRole, HoldingId, ModuleKind,
    Owner, Queueing, ReplacementEntry, ShipClass, ShipId, Slot, Stock, SystemId, Tick, Ticks,
};

use super::fake::{Fake, at, id};
use super::{DeferReason, DeputyEvent, DeputyMemory, Rebuild, Stagger, evaluate};
use crate::inbox::{CommandOutcome, InboxEntry, InboxEvent};
use crate::view::{BuildCheck, FleetPlace, OwnFleet, OwnHolding};
use crate::{Layer, Output};

const ALL: Stagger = Stagger { slot: 0, of: 1 };
const CAP_WING: FleetId = FleetId(nomad_types::SimId::from_raw(500));
const MOTHER: HoldingId = HoldingId(nomad_types::SimId::from_raw(16));
const FORWARD: HoldingId = HoldingId(nomad_types::SimId::from_raw(17));

fn holding(h: HoldingId, system: u64) -> OwnHolding {
    OwnHolding {
        id: h,
        system: SystemId(id(system)),
        complete: true,
    }
}

/// Commander 1 with two holdings, CAP WING at 2 × VX in space, and "keep CAP WING at 3 × VX".
fn scene() -> Fake {
    let mut fake = Fake::new();
    fake.holdings = vec![holding(MOTHER, 3), holding(FORWARD, 4)];
    fake.route_ms.insert(MOTHER, 90_000);
    fake.route_ms.insert(FORWARD, 30_000);
    fake.fleets.insert(
        CAP_WING,
        OwnFleet {
            id: CAP_WING,
            place: FleetPlace::InSpace(at(4, 1_000.0)),
            ships: vec![
                (ShipId(id(501)), ShipClass::Vx),
                (ShipId(id(502)), ShipClass::Vx),
            ],
            busy: false,
        },
    );
    fake.replacement = vec![ReplacementEntry {
        fleet: CAP_WING,
        class: ShipClass::Vx,
        count: 3,
        role: None,
    }];
    // 10,000 alloy an hour: a 3,000 budget at Balanced's 30%.
    fake.income = Stock::units(10_000, 0);
    fake
}

/// A loss at the end of the tick last evaluated, as the simulation's step fills the inbox.
fn lost(fake: &mut Fake, fleet: FleetId, class: ShipClass) {
    fake.inbox.push(InboxEntry {
        tick: fake.now,
        event: InboxEvent::ShipLost {
            ship: ShipId(id(999)),
            class,
            fleet,
            by: Owner::Commander(nomad_types::CommanderId(2)),
        },
    });
}

/// Runs one evaluation on the next tick.
fn run(fake: &mut Fake, memory: &mut DeputyMemory) -> Output {
    fake.now = fake.now.next();
    let mut out = Output::default();
    evaluate(fake, memory, ALL, &mut out);
    out
}

fn commands(out: &Output) -> Vec<Command> {
    out.commands.iter().map(|i| i.command.clone()).collect()
}

fn build(holding: HoldingId, item: BuildItem) -> Command {
    Command::Economy(EconomyCommand::Build { holding, item })
}

#[test]
fn a_listed_loss_is_replaced_at_the_nearest_builder_with_the_rally_set() {
    let mut fake = scene();
    let mut memory = DeputyMemory::default();
    run(&mut fake, &mut memory);
    lost(&mut fake, CAP_WING, ShipClass::Vx);
    let out = run(&mut fake, &mut memory);
    assert_eq!(
        commands(&out),
        vec![
            Command::Economy(EconomyCommand::SetRally {
                holding: FORWARD,
                fleet: Some(CAP_WING),
            }),
            build(
                FORWARD,
                BuildItem::Ship {
                    class: ShipClass::Vx
                }
            ),
        ]
    );
    // Its latency counts from the loss (D-201).
    assert!(out.commands.iter().all(|i| i.trigger == Tick::new(2)));
    // One replacement for one loss: nothing more while it is on its way.
    assert!(run(&mut fake, &mut memory).commands.is_empty());
    // An unlisted class or another fleet is not replaced (D-016: no new projects).
    lost(&mut fake, CAP_WING, ShipClass::Lancer);
    lost(&mut fake, FleetId(id(777)), ShipClass::Vx);
    assert!(run(&mut fake, &mut memory).commands.is_empty());
}

#[test]
fn a_fleet_at_strength_is_not_topped_up() {
    let mut fake = scene();
    fake.replacement[0].count = 2;
    let mut memory = DeputyMemory::default();
    run(&mut fake, &mut memory);
    lost(&mut fake, CAP_WING, ShipClass::Vx);
    assert!(run(&mut fake, &mut memory).commands.is_empty());
}

#[test]
fn over_budget_is_deferred_once_and_retried_when_the_budget_changes() {
    let mut fake = scene();
    fake.income = Stock::ZERO;
    let mut memory = DeputyMemory::default();
    run(&mut fake, &mut memory);
    lost(&mut fake, CAP_WING, ShipClass::Vx);
    let out = run(&mut fake, &mut memory);
    assert!(out.commands.is_empty());
    let deferred = DeputyEvent::Deferred {
        what: Rebuild::Ship {
            fleet: CAP_WING,
            class: ShipClass::Vx,
        },
        reason: DeferReason::BudgetExhausted,
    };
    assert_eq!(out.events, vec![deferred]);
    // Reported once, and no retry while the budget stands.
    for _ in 0..10 {
        let out = run(&mut fake, &mut memory);
        assert!(out.commands.is_empty() && out.events.is_empty());
    }
    // An hour later the ledger has income: it is queued, its latency from now.
    fake.income = Stock::units(1_000, 0);
    fake.now = fake.now.after(Ticks::from_hours(1));
    let out = run(&mut fake, &mut memory);
    assert_eq!(
        commands(&out).last(),
        Some(&build(
            FORWARD,
            BuildItem::Ship {
                class: ShipClass::Vx
            }
        ))
    );
    assert!(out.commands.iter().all(|i| i.trigger == fake.now));
}

#[test]
fn spending_counts_against_the_rolling_day_and_refusals_give_it_back() {
    let mut fake = scene();
    fake.replacement[0].count = 9;
    // 3,000 budget, 1,000 a VX: three fit, the fourth is deferred.
    fake.price = Stock::units(1_000, 0);
    let mut memory = DeputyMemory::default();
    run(&mut fake, &mut memory);
    for _ in 0..4 {
        lost(&mut fake, CAP_WING, ShipClass::Vx);
    }
    let out = run(&mut fake, &mut memory);
    let builds: Vec<Command> = commands(&out)
        .into_iter()
        .filter(|c| matches!(c, Command::Economy(EconomyCommand::Build { .. })))
        .collect();
    assert_eq!(builds.len(), 3);
    assert_eq!(out.events.len(), 1);
    // One build is refused when applied: its spending comes back and it is retried.
    let tick = fake.now.get();
    fake.inbox.push(InboxEntry {
        tick: Tick::new(tick),
        event: InboxEvent::Outcome {
            layer: Layer::Deputy,
            command: builds[0].clone(),
            outcome: CommandOutcome::Rejected,
        },
    });
    let out = run(&mut fake, &mut memory);
    let rebuilt = commands(&out)
        .into_iter()
        .filter(|c| matches!(c, Command::Economy(EconomyCommand::Build { .. })))
        .count();
    assert_eq!(rebuilt, 1);
    // A day later the spending has rolled off and the deferred one goes too.
    fake.now = fake.now.after(Ticks::from_days(1));
    let out = run(&mut fake, &mut memory);
    assert!(commands(&out).contains(&build(
        FORWARD,
        BuildItem::Ship {
            class: ShipClass::Vx
        }
    )));
}

#[test]
fn full_queues_and_missing_builders_are_deferred() {
    let mut fake = scene();
    fake.checks.insert(MOTHER, BuildCheck::QueueFull);
    fake.checks.insert(FORWARD, BuildCheck::CannotBuild);
    let mut memory = DeputyMemory::default();
    run(&mut fake, &mut memory);
    lost(&mut fake, CAP_WING, ShipClass::Vx);
    let out = run(&mut fake, &mut memory);
    assert!(matches!(
        out.events.as_slice(),
        [DeputyEvent::Deferred {
            reason: DeferReason::QueueFull,
            ..
        }]
    ));
    fake.checks.insert(MOTHER, BuildCheck::CannotBuild);
    fake.income = Stock::units(20_000, 0);
    fake.now = fake.now.after(Ticks::from_hours(1));
    let out = run(&mut fake, &mut memory);
    assert!(matches!(
        out.events.as_slice(),
        [DeputyEvent::Deferred {
            reason: DeferReason::NoBuilder,
            ..
        }]
    ));
}

#[test]
fn a_lost_fleet_is_rebuilt_as_a_new_one_which_the_entry_then_keeps() {
    let mut fake = scene();
    fake.fleets.clear();
    fake.replacement[0].role = Some(FleetRole::Garrison { holding: MOTHER });
    fake.rally.insert(FORWARD, FleetId(id(888)));
    let mut memory = DeputyMemory::default();
    run(&mut fake, &mut memory);
    lost(&mut fake, CAP_WING, ShipClass::Vx);
    let out = run(&mut fake, &mut memory);
    let issued = commands(&out);
    // The rally is cleared, so the ship forms a new fleet.
    assert_eq!(
        issued[0],
        Command::Economy(EconomyCommand::SetRally {
            holding: FORWARD,
            fleet: None,
        })
    );
    let tick = fake.now.get();
    fake.inbox.push(InboxEntry {
        tick: Tick::new(tick),
        event: InboxEvent::Outcome {
            layer: Layer::Deputy,
            command: issued[1].clone(),
            outcome: CommandOutcome::Applied,
        },
    });
    let new_fleet = FleetId(id(900));
    fake.inbox.push(InboxEntry {
        tick: Tick::new(tick),
        event: InboxEvent::ShipBuilt {
            holding: FORWARD,
            ship: ShipId(id(901)),
            class: ShipClass::Vx,
            fleet: new_fleet,
        },
    });
    run(&mut fake, &mut memory);
    // A loss from the new fleet is the entry's.
    fake.fleets.insert(
        new_fleet,
        OwnFleet {
            id: new_fleet,
            place: FleetPlace::Docked(FORWARD),
            ships: vec![],
            busy: false,
        },
    );
    fake.rally.insert(FORWARD, new_fleet);
    lost(&mut fake, new_fleet, ShipClass::Vx);
    let out = run(&mut fake, &mut memory);
    assert_eq!(
        commands(&out),
        vec![build(
            FORWARD,
            BuildItem::Ship {
                class: ShipClass::Vx
            }
        )]
    );
}

#[test]
fn a_destroyed_module_is_rebuilt_in_its_slot_if_doctrine_says_so() {
    let mut fake = scene();
    let mut memory = DeputyMemory::default();
    run(&mut fake, &mut memory);
    let destroyed = |fake: &mut Fake, kind| {
        let tick = fake.now.get();
        fake.inbox.push(InboxEntry {
            tick: Tick::new(tick),
            event: InboxEvent::ModuleDestroyed {
                holding: MOTHER,
                module: id(321),
                kind,
                slot: Slot::Solar,
            },
        });
    };
    destroyed(&mut fake, ModuleKind::Solar);
    let out = run(&mut fake, &mut memory);
    assert_eq!(
        commands(&out),
        vec![build(
            MOTHER,
            BuildItem::Module {
                kind: ModuleKind::Solar,
                slot: Slot::Solar,
            }
        )]
    );
    // Off in the holding's doctrine: nothing.
    fake.holding_doctrine.rebuild_modules = false;
    destroyed(&mut fake, ModuleKind::Solar);
    assert!(run(&mut fake, &mut memory).commands.is_empty());
    // A slot someone filled already is left alone.
    fake.holding_doctrine.rebuild_modules = true;
    fake.checks.insert(MOTHER, BuildCheck::SlotTaken);
    destroyed(&mut fake, ModuleKind::Solar);
    let out = run(&mut fake, &mut memory);
    assert!(out.commands.is_empty() && out.events.is_empty());
}

const MULES: FleetId = FleetId(nomad_types::SimId::from_raw(600));
const GATE: nomad_types::GateId = nomad_types::GateId(nomad_types::SimId::from_raw(70));

/// MULES runs FORWARD (system 4) → MOTHER (system 3) and holds at FORWARD for GATE.
fn convoy_scene() -> Fake {
    let mut fake = scene();
    fake.fleets.insert(
        MULES,
        OwnFleet {
            id: MULES,
            place: FleetPlace::Docked(FORWARD),
            ships: vec![(ShipId(id(601)), ShipClass::Mule)],
            busy: false,
        },
    );
    fake.convoys = vec![crate::view::OwnConvoy {
        fleet: MULES,
        route: vec![FORWARD, MOTHER],
        next: 1,
        held_at: Some(GATE),
    }];
    fake
}

fn hop(from: u64, to: u64, system: u64) -> crate::view::RouteHop {
    crate::view::RouteHop {
        from_gate: nomad_types::GateId(id(from)),
        to_gate: nomad_types::GateId(id(to)),
        to_system: SystemId(id(system)),
    }
}

#[test]
fn a_held_convoy_takes_a_detour_and_resumes_at_its_stop() {
    let mut fake = convoy_scene();
    // Around GATE: 4 → 5 → 3; each jump on its own avoids GATE as well.
    fake.routes = vec![
        (
            crate::view::RouteTo::Holding(MOTHER),
            vec![GATE],
            vec![hop(80, 81, 5), hop(82, 83, 3)],
        ),
        (
            crate::view::RouteTo::System(SystemId(id(5))),
            vec![],
            vec![hop(80, 81, 5)],
        ),
        (
            crate::view::RouteTo::System(SystemId(id(3))),
            vec![],
            vec![hop(82, 83, 3)],
        ),
    ];
    let mut memory = DeputyMemory::default();
    let out = run(&mut fake, &mut memory);
    let order = |order, queue| Command::Fleet {
        fleet: MULES,
        order,
        queue,
        retreat: false,
    };
    let dock = order(FleetOrder::Dock { at: MOTHER }, Queueing::Append);
    assert_eq!(
        commands(&out),
        vec![
            order(FleetOrder::Launch, Queueing::Replace),
            order(
                FleetOrder::Jump {
                    system: SystemId(id(5))
                },
                Queueing::Append
            ),
            order(
                FleetOrder::Jump {
                    system: SystemId(id(3))
                },
                Queueing::Append
            ),
            dock.clone(),
        ]
    );
    assert_eq!(
        out.events,
        vec![DeputyEvent::ConvoyRerouted {
            fleet: MULES,
            gate: GATE
        }]
    );
    // The orders end the convoy route (S4); the fleet travels, then docks at MOTHER.
    fake.convoys.clear();
    if let Some(f) = fake.fleets.get_mut(&MULES) {
        f.busy = true;
        f.place = FleetPlace::Travelling;
    }
    let tick = fake.now;
    fake.inbox.push(InboxEntry {
        tick,
        event: InboxEvent::Outcome {
            layer: Layer::Deputy,
            command: dock,
            outcome: CommandOutcome::Applied,
        },
    });
    assert!(run(&mut fake, &mut memory).commands.is_empty());
    if let Some(f) = fake.fleets.get_mut(&MULES) {
        f.busy = false;
        f.place = FleetPlace::Docked(MOTHER);
    }
    let out = run(&mut fake, &mut memory);
    assert_eq!(
        commands(&out),
        vec![Command::Economy(EconomyCommand::Convoy {
            fleet: MULES,
            route: vec![FORWARD, MOTHER],
        })]
    );
    assert_eq!(
        out.events,
        vec![DeputyEvent::ConvoyResumed { fleet: MULES }]
    );
}

#[test]
fn without_a_detour_the_convoy_holds_and_is_reported_once() {
    let mut fake = convoy_scene();
    // The only route to MOTHER uses GATE.
    fake.routes = vec![(
        crate::view::RouteTo::Holding(MOTHER),
        vec![],
        vec![nomad_types_hop()],
    )];
    let mut memory = DeputyMemory::default();
    let out = run(&mut fake, &mut memory);
    assert!(out.commands.is_empty());
    assert_eq!(
        out.events,
        vec![DeputyEvent::ConvoyHeld {
            fleet: MULES,
            gate: GATE
        }]
    );
    assert!(run(&mut fake, &mut memory).events.is_empty());
    // The gate clears: the simulation sets out again, and the Deputy reports it.
    fake.convoys[0].held_at = None;
    let out = run(&mut fake, &mut memory);
    assert!(out.commands.is_empty());
    assert_eq!(
        out.events,
        vec![DeputyEvent::ConvoyResumed { fleet: MULES }]
    );
}

fn nomad_types_hop() -> crate::view::RouteHop {
    crate::view::RouteHop {
        from_gate: GATE,
        to_gate: nomad_types::GateId(id(71)),
        to_system: SystemId(id(3)),
    }
}
