//! The raider director against a fake view (A3 plan §9 "Director tests").

use nomad_types::{
    Command, CommanderId, FleetId, FleetOrder, HoldingId, ModuleKind, Owner, PositionMetres,
    Queueing, ShipClass, ShipId, SimId, Slot, SystemId, Tick,
};

use super::fake::{Contact, Fake};
use super::{DirectorEvent, DirectorMemory, Withdrawal, evaluate};
use crate::inbox::{CommandOutcome, DropReason, FleetHappening, InboxEntry, InboxEvent};
use crate::view::{FleetPlace, OwnFleet, PublicHolding, RaidOrder, SensedHolding, SensedModule};
use crate::{Layer, Output};

const fn id(raw: u64) -> SimId {
    SimId::from_raw(raw)
}

const HAVEN: SystemId = SystemId(id(9));
const HUB: SystemId = SystemId(id(8));
/// Its raw id is a multiple of 5, so it is due on ticks divisible by 5.
const GROUP: FleetId = FleetId(id(500));
const TARGET: CommanderId = CommanderId(2);
const OTHER: CommanderId = CommanderId(3);
const OUTPOST: HoldingId = HoldingId(id(40));
const STATION: HoldingId = HoldingId(id(41));

fn at(system: SystemId, x: f64, y: f64) -> nomad_types::Location {
    nomad_types::Location {
        system,
        position: PositionMetres::new(x, y),
    }
}

/// Two strike craft raiding commander 2 in the hub, still at the haven.
fn scene() -> Fake {
    let mut fake = Fake::new();
    fake.now = Tick::new(10);
    fake.orders = vec![RaidOrder {
        fleet: GROUP,
        haven: HAVEN,
        target: TARGET,
        system: HUB,
        value: 3_200,
        launched: Tick::new(10),
    }];
    fake.fleets.insert(
        GROUP,
        OwnFleet {
            id: GROUP,
            place: FleetPlace::InSpace(at(HAVEN, 0.0, 0.0)),
            ships: vec![
                (ShipId(id(501)), ShipClass::RaiderStrikeCraft),
                (ShipId(id(502)), ShipClass::RaiderStrikeCraft),
            ],
            busy: false,
            hull: 1_400,
            max_hull: 1_400,
            order: None,
            rules: nomad_types::RulesOfEngagement::WeaponsFree,
            engaged: false,
        },
    );
    fake
}

/// Evaluates on the tick `now` (the group is due on multiples of 5).
fn run_at(fake: &mut Fake, memory: &mut DirectorMemory, now: u64) -> Output {
    fake.now = Tick::new(now);
    let mut out = Output::default();
    evaluate(fake, memory, &mut out);
    out
}

fn orders(out: &Output) -> Vec<FleetOrder> {
    out.commands
        .iter()
        .map(|issue| match &issue.command {
            Command::Fleet {
                fleet,
                order,
                queue,
                ..
            } => {
                assert_eq!((*fleet, *queue), (GROUP, Queueing::Replace));
                order.clone()
            }
            other => panic!("not a fleet command: {other:?}"),
        })
        .collect()
}

fn outcome(fake: &mut Fake, order: FleetOrder, outcome: CommandOutcome) {
    fake.inbox.push(InboxEntry {
        tick: fake.now,
        event: InboxEvent::Outcome {
            layer: Layer::Director,
            command: Command::Fleet {
                fleet: GROUP,
                order,
                queue: Queueing::Replace,
                retreat: false,
            },
            outcome,
        },
    });
}

/// The group arrives in the hub on tick `tick`: the inbox says so, and it is there.
fn arrive(fake: &mut Fake, tick: u64) {
    fake.inbox.push(InboxEntry {
        tick: Tick::new(tick),
        event: InboxEvent::Fleet {
            fleet: GROUP,
            what: FleetHappening::Arrived { system: HUB },
        },
    });
    if let Some(f) = fake.fleets.get_mut(&GROUP) {
        f.place = FleetPlace::InSpace(at(HUB, 0.0, 0.0));
        f.busy = false;
    }
}

/// A group that has arrived in the hub on tick 1,000 and is past its shroud on 1,100.
fn raiding() -> (Fake, DirectorMemory) {
    let mut fake = scene();
    let mut memory = DirectorMemory::default();
    run_at(&mut fake, &mut memory, 10);
    arrive(&mut fake, 1_000);
    (fake, memory)
}

fn contact(raw: u64, fleet: u64, owner: CommanderId, class: ShipClass, x: f64) -> Contact {
    Contact {
        ship: ShipId(id(raw)),
        fleet: FleetId(id(fleet)),
        owner: Owner::Commander(owner),
        class,
        position: PositionMetres::new(x, 0.0),
        shrouded: false,
    }
}

fn module(raw: u64, kind: ModuleKind, slot: Slot, x: f64, y: f64) -> SensedModule {
    SensedModule {
        id: id(raw),
        kind,
        slot,
        position: PositionMetres::new(x, y),
        disabled: false,
    }
}

/// Commander 2's completed station 20 km out: core, an inner refinery, a turret at the
/// centre, a solar array 3 km out (beyond the turret's 2.5 km), a harvester 1 km out.
fn station() -> SensedHolding {
    SensedHolding {
        holding: STATION,
        owner: TARGET,
        outpost: false,
        position: PositionMetres::new(20_000.0, 0.0),
        modules: vec![
            module(60, ModuleKind::Core, Slot::Core, 20_000.0, 0.0),
            module(61, ModuleKind::Refinery, Slot::Inner(0), 20_300.0, 0.0),
            module(62, ModuleKind::Turret, Slot::Inner(1), 20_000.0, 300.0),
            module(63, ModuleKind::Solar, Slot::Solar, 20_000.0, 3_000.0),
            module(64, ModuleKind::Harvester, Slot::Deposit(0), 21_000.0, 0.0),
        ],
    }
}

fn attacked(out: &Output) -> Option<SimId> {
    orders(out).into_iter().find_map(|o| match o {
        FleetOrder::Attack { target } => Some(target),
        _ => None,
    })
}

#[test]
fn a_new_group_jumps_to_its_target_once_and_waits_for_the_outcome() {
    let mut fake = scene();
    let mut memory = DirectorMemory::default();
    let out = run_at(&mut fake, &mut memory, 10);
    assert_eq!(orders(&out), [FleetOrder::Jump { system: HUB }]);
    // Waiting in the outbox: not again.
    assert!(run_at(&mut fake, &mut memory, 15).commands.is_empty());
    // Applied, and the fleet travels: nothing.
    outcome(
        &mut fake,
        FleetOrder::Jump { system: HUB },
        CommandOutcome::Applied,
    );
    fake.fleets.get_mut(&GROUP).unwrap().busy = true;
    assert!(run_at(&mut fake, &mut memory, 20).commands.is_empty());
    // Dropped instead: again.
    outcome(
        &mut fake,
        FleetOrder::Jump { system: HUB },
        CommandOutcome::Dropped(DropReason::Expired),
    );
    assert_eq!(
        orders(&run_at(&mut fake, &mut memory, 25)),
        [FleetOrder::Jump { system: HUB }]
    );
    // In transit nothing is ordered.
    fake.fleets.get_mut(&GROUP).unwrap().place = FleetPlace::Travelling;
    assert!(run_at(&mut fake, &mut memory, 30).commands.is_empty());
}

#[test]
fn groups_are_evaluated_at_2_hz_by_their_id() {
    let mut fake = scene();
    let mut memory = DirectorMemory::default();
    for now in 11..15 {
        assert!(run_at(&mut fake, &mut memory, now).commands.is_empty());
    }
    assert_eq!(run_at(&mut fake, &mut memory, 15).commands.len(), 1);
}

#[test]
fn the_clock_starts_when_the_shroud_ends_and_withdraws_the_group_after_15_minutes() {
    let (mut fake, mut memory) = raiding();
    fake.public.insert(
        HUB,
        vec![PublicHolding {
            holding: STATION,
            owner: TARGET,
            outpost: false,
            position: PositionMetres::new(20_000.0, 0.0),
        }],
    );
    // The inbox is read on the next call: the clock starts at 1,000 + the 10 s shroud.
    let out = run_at(&mut fake, &mut memory, 1_005);
    assert_eq!(
        out.director_events,
        [DirectorEvent::ClockStarted { fleet: GROUP }]
    );
    assert_eq!(memory.clock_started(GROUP), Some(Tick::new(1_100)));
    // Shrouded: nothing yet.
    assert!(out.commands.is_empty());
    // Then, with nothing sensed, it approaches the station: 5 km short, on its side.
    let out = run_at(&mut fake, &mut memory, 1_100);
    assert_eq!(
        orders(&out),
        [FleetOrder::AttackMove {
            to: at(HUB, 15_000.0, 0.0)
        }]
    );
    // Moving on, the approach stands.
    outcome(&mut fake, orders(&out)[0].clone(), CommandOutcome::Applied);
    fake.fleets.get_mut(&GROUP).unwrap().place = FleetPlace::InSpace(at(HUB, 4_000.0, 0.0));
    fake.fleets.get_mut(&GROUP).unwrap().busy = true;
    assert!(run_at(&mut fake, &mut memory, 10_095).commands.is_empty());
    // 15 minutes after the shroud (D-313): home.
    let out = run_at(&mut fake, &mut memory, 10_100);
    assert_eq!(orders(&out), [FleetOrder::Jump { system: HAVEN }]);
    assert_eq!(
        out.director_events,
        [DirectorEvent::Withdrew {
            fleet: GROUP,
            reason: Withdrawal::Clock
        }]
    );
    assert_eq!(
        memory.withdrawal(GROUP),
        Some((Tick::new(10_100), Withdrawal::Clock))
    );
    // Withdrawing, it attacks nothing, even with a target in front of it.
    fake.contacts
        .insert(HUB, vec![contact(70, 71, TARGET, ShipClass::Mule, 4_500.0)]);
    assert!(run_at(&mut fake, &mut memory, 10_105).commands.is_empty());
}

#[test]
fn it_withdraws_at_half_its_value_lost() {
    let (mut fake, mut memory) = raiding();
    run_at(&mut fake, &mut memory, 1_100);
    // One of two strike craft lost: exactly half.
    fake.fleets.get_mut(&GROUP).unwrap().ships.pop();
    let out = run_at(&mut fake, &mut memory, 1_105);
    assert_eq!(orders(&out), [FleetOrder::Jump { system: HAVEN }]);
    assert_eq!(
        out.director_events,
        [DirectorEvent::Withdrew {
            fleet: GROUP,
            reason: Withdrawal::Losses
        }]
    );
}

#[test]
fn it_withdraws_early_from_more_than_twice_its_value_within_5_km() {
    let (mut fake, mut memory) = raiding();
    run_at(&mut fake, &mut memory, 1_005);
    // 5 VX of commander 3 (6,000) within 5 km: not more than twice 3,200, so it stays; and
    // they are not its target's, so it does not attack them.
    let vx = |raw, x| contact(raw, 80, OTHER, ShipClass::Vx, x);
    fake.contacts.insert(
        HUB,
        vec![
            vx(81, 1_000.0),
            vx(82, 1_000.0),
            vx(83, 2_000.0),
            vx(84, 4_900.0),
            vx(85, 3_000.0),
        ],
    );
    let out = run_at(&mut fake, &mut memory, 1_100);
    assert!(out.director_events.is_empty());
    assert!(attacked(&out).is_none());
    // A sixth, 7,200 > 6,400: it withdraws. One more beyond 5 km would not have counted.
    fake.contacts.get_mut(&HUB).unwrap().push(vx(86, 4_000.0));
    let out = run_at(&mut fake, &mut memory, 1_105);
    assert_eq!(orders(&out), [FleetOrder::Jump { system: HAVEN }]);
    assert_eq!(
        out.director_events,
        [DirectorEvent::Withdrew {
            fleet: GROUP,
            reason: Withdrawal::StrongerForce
        }]
    );
}

#[test]
fn targets_follow_the_priority_order() {
    let (mut fake, mut memory) = raiding();
    run_at(&mut fake, &mut memory, 1_005);
    let mut now = 1_100;
    let mut next = |fake: &mut Fake, memory: &mut DirectorMemory| {
        let out = run_at(fake, memory, now);
        now += 5;
        attacked(&out)
    };
    // 4. Peripheral structures: the solar array first, though the harvester is nearer;
    // never the core, the refinery or the turret inside the dome.
    fake.sensed.insert(HUB, vec![station()]);
    assert_eq!(next(&mut fake, &mut memory), Some(id(63)));
    // Disabled, it is done with; then the harvester.
    fake.sensed.get_mut(&HUB).unwrap()[0].modules[3].disabled = true;
    assert_eq!(next(&mut fake, &mut memory), Some(id(64)));
    // 3. A weaker fleet's ships come before structures: 2 VX (2,400 < 3,200), nearest first.
    fake.contacts.insert(
        HUB,
        vec![
            contact(90, 92, TARGET, ShipClass::Vx, 8_000.0),
            contact(91, 92, TARGET, ShipClass::Vx, 7_000.0),
            // A stronger fleet of commander 2's (a WARDEN, 5,800), and another commander's
            // MULE, are not targets. All beyond 5 km, so the group stays.
            contact(93, 94, TARGET, ShipClass::Warden, 6_000.0),
            contact(95, 96, OTHER, ShipClass::Mule, 5_500.0),
        ],
    );
    assert_eq!(next(&mut fake, &mut memory), Some(id(91)));
    // 2. An outpost under construction comes before ships.
    fake.sensed.get_mut(&HUB).unwrap().push(SensedHolding {
        holding: OUTPOST,
        owner: TARGET,
        outpost: true,
        position: PositionMetres::new(-8_000.0, 0.0),
        modules: vec![module(97, ModuleKind::Core, Slot::Core, -8_000.0, 0.0)],
    });
    assert_eq!(next(&mut fake, &mut memory), Some(OUTPOST.sim_id()));
    // 1. MULEs first of all: one inside the turret's reach, one outside it, farther away.
    fake.contacts.get_mut(&HUB).unwrap().extend([
        contact(98, 99, TARGET, ShipClass::Mule, 20_500.0),
        contact(100, 99, TARGET, ShipClass::Mule, 25_000.0),
    ]);
    assert_eq!(next(&mut fake, &mut memory), Some(id(100)));
    // The attack applied and runs: the target is kept.
    outcome(
        &mut fake,
        FleetOrder::Attack { target: id(100) },
        CommandOutcome::Applied,
    );
    fake.fleets.get_mut(&GROUP).unwrap().busy = true;
    assert_eq!(next(&mut fake, &mut memory), None);
}

#[test]
fn shrouded_ships_are_no_targets() {
    let (mut fake, mut memory) = raiding();
    run_at(&mut fake, &mut memory, 1_005);
    let mut mule = contact(70, 71, TARGET, ShipClass::Mule, 1_000.0);
    mule.shrouded = true;
    fake.contacts.insert(HUB, vec![mule]);
    assert_eq!(attacked(&run_at(&mut fake, &mut memory, 1_100)), None);
    fake.contacts.get_mut(&HUB).unwrap()[0].shrouded = false;
    assert_eq!(
        attacked(&run_at(&mut fake, &mut memory, 1_105)),
        Some(id(70))
    );
}

#[test]
fn it_never_orders_an_attack_on_a_dome_or_a_module_inside_one() {
    let (mut fake, mut memory) = raiding();
    run_at(&mut fake, &mut memory, 1_005);
    let mut inside = station();
    inside
        .modules
        .retain(|m| !matches!(m.slot, Slot::Solar | Slot::Deposit(_)));
    fake.sensed.insert(HUB, vec![inside.clone()]);
    fake.public.insert(
        HUB,
        vec![PublicHolding {
            holding: STATION,
            owner: TARGET,
            outpost: false,
            position: inside.position,
        }],
    );
    let forbidden: Vec<SimId> = inside
        .modules
        .iter()
        .map(|m| m.id)
        .chain([STATION.sim_id()])
        .collect();
    for now in (1_100..1_500).step_by(5) {
        let out = run_at(&mut fake, &mut memory, now);
        for order in orders(&out) {
            if let FleetOrder::Attack { target } = order {
                assert!(!forbidden.contains(&target), "{target:?}");
            }
        }
    }
}

#[test]
fn a_refused_target_is_not_chosen_again_nor_a_refused_approach_repeated() {
    let (mut fake, mut memory) = raiding();
    run_at(&mut fake, &mut memory, 1_005);
    fake.contacts.insert(
        HUB,
        vec![
            contact(70, 71, TARGET, ShipClass::Mule, 1_000.0),
            contact(72, 73, TARGET, ShipClass::Mule, 2_000.0),
        ],
    );
    assert_eq!(
        attacked(&run_at(&mut fake, &mut memory, 1_100)),
        Some(id(70))
    );
    outcome(
        &mut fake,
        FleetOrder::Attack { target: id(70) },
        CommandOutcome::Rejected,
    );
    assert_eq!(
        attacked(&run_at(&mut fake, &mut memory, 1_105)),
        Some(id(72))
    );
    // With no target left, the approach; refused, it is not tried again.
    fake.contacts.clear();
    fake.public.insert(
        HUB,
        vec![PublicHolding {
            holding: STATION,
            owner: TARGET,
            outpost: false,
            position: PositionMetres::new(20_000.0, 0.0),
        }],
    );
    let out = run_at(&mut fake, &mut memory, 1_110);
    let approach = orders(&out);
    assert!(matches!(
        approach.as_slice(),
        [FleetOrder::AttackMove { .. }]
    ));
    outcome(&mut fake, approach[0].clone(), CommandOutcome::Rejected);
    assert!(run_at(&mut fake, &mut memory, 1_115).commands.is_empty());
}

#[test]
fn a_group_that_is_gone_is_forgotten() {
    let (mut fake, mut memory) = raiding();
    run_at(&mut fake, &mut memory, 1_005);
    assert!(!memory.is_empty());
    fake.orders.clear();
    fake.fleets.clear();
    run_at(&mut fake, &mut memory, 1_010);
    assert!(memory.is_empty());
}

#[test]
fn an_approach_prefers_an_outpost_and_stops_short_of_it() {
    let (mut fake, mut memory) = raiding();
    run_at(&mut fake, &mut memory, 1_005);
    fake.public.insert(
        HUB,
        vec![
            PublicHolding {
                holding: STATION,
                owner: TARGET,
                outpost: false,
                position: PositionMetres::new(10_000.0, 0.0),
            },
            PublicHolding {
                holding: OUTPOST,
                owner: TARGET,
                outpost: true,
                position: PositionMetres::new(0.0, -20_000.0),
            },
            // Another commander's holding is not the target's.
            PublicHolding {
                holding: HoldingId(id(42)),
                owner: OTHER,
                outpost: true,
                position: PositionMetres::new(1_000.0, 0.0),
            },
        ],
    );
    assert_eq!(
        orders(&run_at(&mut fake, &mut memory, 1_100)),
        [FleetOrder::AttackMove {
            to: at(HUB, 0.0, -15_000.0)
        }]
    );
}
