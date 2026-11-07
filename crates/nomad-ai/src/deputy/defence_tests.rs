//! The Deputy against a fake view: threat and alert, the alert actions, fleet roles and
//! engagement (A1 plan §5.3 to §5.6; D-206, D-207).

use nomad_types::{
    Command, CommanderId, DirectControl, EconomyCommand, FleetDoctrine, FleetId, FleetOrder,
    FleetRole, HoldingId, HoldingOrder, Owner, PositionMetres, PowerProfile, Queueing, ShipClass,
    ShipId, StationAbility, SystemId, Ticks,
};

use super::fake::{Fake, balanced, id, idle_fleet};
use super::{DeputyEvent, DeputyMemory, Stagger, evaluate};
use crate::inbox::{CommandOutcome, InboxEntry, InboxEvent};
use crate::view::{
    FleetPlace, HoldingDefence, OwnFleet, OwnHolding, Sector, SensedFleet, SensedShip,
};
use crate::{Layer, Output};

const ALL: Stagger = Stagger { slot: 0, of: 1 };
const MOTHER: HoldingId = HoldingId(nomad_types::SimId::from_raw(16));
const GARRISON: FleetId = FleetId(nomad_types::SimId::from_raw(500));
const RESERVE: FleetId = FleetId(nomad_types::SimId::from_raw(600));
const RAIDERS: FleetId = FleetId(nomad_types::SimId::from_raw(900));
const HOME: u64 = 3;

fn system() -> SystemId {
    SystemId(id(HOME))
}

fn point(x: f64, y: f64) -> PositionMetres {
    PositionMetres::new(x, y)
}

fn here(x: f64) -> FleetPlace {
    FleetPlace::InSpace(nomad_types::Location {
        system: system(),
        position: point(x, 0.0),
    })
}

fn fleet(id_: FleetId, place: FleetPlace, ships: &[(u64, ShipClass)]) -> OwnFleet {
    OwnFleet {
        id: id_,
        place,
        ships: ships.iter().map(|&(s, c)| (ShipId(id(s)), c)).collect(),
        ..idle_fleet()
    }
}

/// The mother station at the origin of system 3, its 3 VX garrison and 2 MULE reserve docked.
fn scene() -> Fake {
    let mut fake = Fake::new();
    fake.holdings = vec![OwnHolding {
        id: MOTHER,
        system: system(),
        complete: true,
        centre: PositionMetres::ORIGIN,
    }];
    fake.fleets.insert(
        GARRISON,
        fleet(
            GARRISON,
            FleetPlace::Docked(MOTHER),
            &[
                (501, ShipClass::Vx),
                (502, ShipClass::Vx),
                (503, ShipClass::Vx),
            ],
        ),
    );
    fake.fleets.insert(
        RESERVE,
        fleet(
            RESERVE,
            FleetPlace::Docked(MOTHER),
            &[(601, ShipClass::Mule), (602, ShipClass::Mule)],
        ),
    );
    fake
}

/// A hostile fleet of `classes`, its ships at `xs` along the x axis, detected by the last
/// pass.
fn raiders(fake: &mut Fake, fleet: FleetId, ships: &[(u64, ShipClass, f64)]) {
    fake.sensed.push((
        system(),
        SensedFleet {
            id: fleet,
            owner: Owner::Commander(CommanderId(2)),
            position: point(ships.first().map_or(0.0, |s| s.2), 0.0),
            ships: ships
                .iter()
                .map(|&(s, class, x)| SensedShip {
                    id: ShipId(id(s)),
                    class,
                    position: point(x, 0.0),
                    shrouded: false,
                })
                .collect(),
        },
    ));
    fake.picture = fake.now;
}

fn run(fake: &mut Fake, memory: &mut DeputyMemory) -> Output {
    fake.now = fake.now.next();
    let mut out = Output::default();
    evaluate(fake, memory, ALL, &mut out);
    out
}

fn commands(out: &Output) -> Vec<Command> {
    out.commands.iter().map(|i| i.command.clone()).collect()
}

/// The simulation applies every command issued, as its inbox will report.
fn applied(fake: &mut Fake, out: &Output) {
    for issue in &out.commands {
        fake.inbox.push(InboxEntry {
            tick: fake.now,
            event: InboxEvent::Outcome {
                layer: Layer::Deputy,
                command: issue.command.clone(),
                outcome: CommandOutcome::Applied,
            },
        });
    }
}

fn to(fleet: FleetId, order: FleetOrder, queue: Queueing) -> Command {
    Command::Fleet {
        fleet,
        order,
        queue,
        retreat: false,
    }
}

fn profile(p: PowerProfile) -> Command {
    Command::Economy(EconomyCommand::SetPowerProfile {
        holding: MOTHER,
        profile: p,
    })
}

#[test]
fn a_hostile_in_the_zone_starts_an_alert_that_launches_the_garrison_only() {
    let mut fake = scene();
    let mut memory = DeputyMemory::default();
    assert!(commands(&run(&mut fake, &mut memory)).is_empty(), "calm");
    // Inside the 10 km zone (Balanced, doc 13 §9).
    raiders(
        &mut fake,
        RAIDERS,
        &[(901, ShipClass::RaiderStrikeCraft, 8_000.0)],
    );
    let seen = fake.picture;
    let out = run(&mut fake, &mut memory);
    assert_eq!(
        out.events,
        vec![DeputyEvent::AlertStarted { holding: MOTHER }]
    );
    // The alert profile first, then the garrison's launch; the MULE reserve stays docked.
    assert_eq!(
        commands(&out),
        vec![
            profile(PowerProfile::ShieldsFirst),
            to(GARRISON, FleetOrder::Launch, Queueing::Replace),
        ]
    );
    // The latency counts from the sensors pass that saw them (D-201).
    assert!(out.commands.iter().all(|i| i.trigger == seen));
}

#[test]
fn a_hostile_outside_the_zone_or_fire_outside_every_zone_starts_no_alert() {
    let mut fake = scene();
    let mut memory = DeputyMemory::default();
    raiders(
        &mut fake,
        RAIDERS,
        &[(901, ShipClass::RaiderStrikeCraft, 11_000.0)],
    );
    // An own fleet hit 20 km out: outside the zone.
    let picket = FleetId(id(700));
    fake.fleets.insert(
        picket,
        fleet(picket, here(20_000.0), &[(701, ShipClass::Vx)]),
    );
    fake.inbox.push(InboxEntry {
        tick: fake.now,
        event: InboxEvent::ShipDamaged {
            ship: ShipId(id(701)),
            fleet: picket,
            by: Owner::Commander(CommanderId(2)),
            by_fleet: Some(RAIDERS),
            hull: 10,
        },
    });
    let out = run(&mut fake, &mut memory);
    assert!(out.events.is_empty(), "{:?}", out.events);
    assert!(commands(&out).is_empty());
}

#[test]
fn fire_on_the_holding_starts_an_alert_and_it_ends_60_s_after_the_last_event() {
    let mut fake = scene();
    let mut memory = DeputyMemory::default();
    run(&mut fake, &mut memory);
    let hit = fake.now;
    fake.inbox.push(InboxEntry {
        tick: hit,
        event: InboxEvent::SectorDamaged {
            holding: MOTHER,
            sector: 1,
            by: Owner::Commander(CommanderId(2)),
            by_fleet: None,
            amount_milli: 1_000,
        },
    });
    let out = run(&mut fake, &mut memory);
    assert_eq!(
        out.events,
        vec![DeputyEvent::AlertStarted { holding: MOTHER }]
    );
    assert!(out.commands.iter().all(|i| i.trigger == hit));
    applied(&mut fake, &out);
    // The garrison is out; nothing in sight, so it holds by the core.
    if let Some(f) = fake.fleets.get_mut(&GARRISON) {
        f.place = here(0.0);
    }
    fake.profiles.insert(MOTHER, PowerProfile::ShieldsFirst);
    assert!(run(&mut fake, &mut memory).events.is_empty());
    // 60 s after the hit, the alert ends: the profile returns and the garrison docks.
    fake.now = hit.after(Ticks::from_secs(60));
    let out = run(&mut fake, &mut memory);
    assert_eq!(
        out.events,
        vec![DeputyEvent::AlertEnded { holding: MOTHER }]
    );
    assert_eq!(
        commands(&out),
        vec![
            profile(PowerProfile::Balanced),
            to(GARRISON, FleetOrder::Dock { at: MOTHER }, Queueing::Replace),
        ]
    );
}

/// On alert, a launched garrison in space with the raiders in sight.
fn engaged_scene() -> (Fake, DeputyMemory) {
    let mut fake = scene();
    let mut memory = DeputyMemory::default();
    run(&mut fake, &mut memory);
    if let Some(f) = fake.fleets.get_mut(&GARRISON) {
        f.place = here(0.0);
    }
    (fake, memory)
}

fn attack(target: u64) -> Command {
    to(
        GARRISON,
        FleetOrder::Attack { target: id(target) },
        Queueing::Replace,
    )
}

#[test]
fn targets_go_by_the_counter_triangle_then_distance_and_never_beyond_the_leash() {
    let (mut fake, mut memory) = engaged_scene();
    // A MULE at 3 km (50% for the VX cannon here), a strike craft at 6 km (100%), and a
    // strike craft at 13 km, beyond the 12 km leash.
    raiders(
        &mut fake,
        RAIDERS,
        &[
            (901, ShipClass::Mule, 3_000.0),
            (902, ShipClass::RaiderStrikeCraft, 6_000.0),
            (903, ShipClass::RaiderStrikeCraft, 13_000.0),
        ],
    );
    let out = run(&mut fake, &mut memory);
    assert!(
        commands(&out).contains(&attack(902)),
        "{:?}",
        commands(&out)
    );
    applied(&mut fake, &out);
    // While it attacks it, a nearer strike craft does not change the choice.
    if let Some(f) = fake.fleets.get_mut(&GARRISON) {
        f.order = Some(FleetOrder::Attack { target: id(902) });
    }
    fake.sensed.clear();
    raiders(
        &mut fake,
        RAIDERS,
        &[
            (902, ShipClass::RaiderStrikeCraft, 6_000.0),
            (904, ShipClass::RaiderStrikeCraft, 2_000.0),
        ],
    );
    assert!(commands(&run(&mut fake, &mut memory)).is_empty());
    // Only the one beyond the leash is left: dropped, and the garrison holds by the core.
    if let Some(f) = fake.fleets.get_mut(&GARRISON) {
        f.place = here(4_000.0);
    }
    fake.sensed.clear();
    raiders(
        &mut fake,
        RAIDERS,
        &[(903, ShipClass::RaiderStrikeCraft, 13_000.0)],
    );
    let out = run(&mut fake, &mut memory);
    assert_eq!(
        commands(&out),
        vec![to(
            GARRISON,
            FleetOrder::Move {
                to: nomad_types::Location {
                    system: system(),
                    position: PositionMetres::ORIGIN,
                },
            },
            Queueing::Replace,
        )]
    );
}

#[test]
fn hostiles_that_damaged_our_structures_come_first() {
    let (mut fake, mut memory) = engaged_scene();
    let bombers = FleetId(id(950));
    raiders(
        &mut fake,
        RAIDERS,
        &[(901, ShipClass::RaiderStrikeCraft, 2_000.0)],
    );
    raiders(&mut fake, bombers, &[(951, ShipClass::Mule, 9_000.0)]);
    fake.inbox.push(InboxEntry {
        tick: fake.now,
        event: InboxEvent::ModuleDamaged {
            holding: MOTHER,
            module: id(42),
            by: Owner::Commander(CommanderId(2)),
            by_fleet: Some(bombers),
            hull: 50,
        },
    });
    let out = run(&mut fake, &mut memory);
    assert!(
        commands(&out).contains(&attack(951)),
        "{:?}",
        commands(&out)
    );
}

#[test]
fn a_fleet_below_its_threshold_retreats_and_a_direct_order_only_with_the_opt_in() {
    let (mut fake, mut memory) = engaged_scene();
    raiders(
        &mut fake,
        RAIDERS,
        &[(901, ShipClass::RaiderStrikeCraft, 2_000.0)],
    );
    if let Some(f) = fake.fleets.get_mut(&GARRISON) {
        // 39% against Balanced's 40%.
        f.hull = 39;
    }
    let retreat = to(
        GARRISON,
        FleetOrder::Retreat { to: None },
        Queueing::Replace,
    );
    let (mut direct, mut direct_memory) = (fake.clone(), memory.clone());
    let (mut opted, mut opted_memory) = (fake.clone(), memory.clone());
    let out = run(&mut fake, &mut memory);
    assert!(commands(&out).contains(&retreat));
    // Retreating, it is left to it: no attack on the raiders in reach of the holding.
    applied(&mut fake, &out);
    if let Some(f) = fake.fleets.get_mut(&GARRISON) {
        f.order = Some(FleetOrder::Retreat { to: None });
    }
    assert!(
        commands(&run(&mut fake, &mut memory)).is_empty(),
        "a retreat is not undone"
    );
    // Under a direct order, nothing; with the retreat opt-in, the retreat (RK-10).
    direct
        .control
        .insert(GARRISON, DirectControl { retreat: false });
    let out = run(&mut direct, &mut direct_memory);
    assert!(
        !commands(&out)
            .iter()
            .any(|c| matches!(c, Command::Fleet { fleet, .. } if *fleet == GARRISON))
    );
    opted
        .control
        .insert(GARRISON, DirectControl { retreat: true });
    assert!(commands(&run(&mut opted, &mut opted_memory)).contains(&retreat));
}

/// *Proposed* reading (fleets.rs): the fleet's maximum hull counts the ships it lost since it
/// last docked, so focused fire that destroys one ship at a time still brings a retreat.
#[test]
fn ships_lost_since_docking_count_towards_the_retreat_threshold() {
    let (mut fake, mut memory) = engaged_scene();
    raiders(
        &mut fake,
        RAIDERS,
        &[(901, ShipClass::RaiderStrikeCraft, 2_000.0)],
    );
    let retreat = to(
        GARRISON,
        FleetOrder::Retreat { to: None },
        Queueing::Replace,
    );
    // Two of its three VX destroyed; the survivor is undamaged (the fake's VX has 100 hull).
    for ship in [502, 503] {
        fake.inbox.push(InboxEntry {
            tick: fake.now,
            event: InboxEvent::ShipLost {
                ship: ShipId(id(ship)),
                class: ShipClass::Vx,
                fleet: GARRISON,
                by: Owner::Commander(CommanderId(2)),
            },
        });
    }
    if let Some(f) = fake.fleets.get_mut(&GARRISON) {
        f.ships.truncate(1);
    }
    let (mut docked, mut docked_memory) = (fake.clone(), memory.clone());
    // 100 of 300: below Balanced's 40%.
    assert!(commands(&run(&mut fake, &mut memory)).contains(&retreat));
    // Docking forgets the losses: launched again, the survivor is at full hull.
    docked.inbox.push(InboxEntry {
        tick: docked.now,
        event: InboxEvent::Fleet {
            fleet: GARRISON,
            what: crate::inbox::FleetHappening::Docked { holding: MOTHER },
        },
    });
    assert!(!commands(&run(&mut docked, &mut docked_memory)).contains(&retreat));
}

#[test]
fn shld_surge_and_flak_screen_fire_on_their_preset_triggers() {
    let (mut fake, mut memory) = engaged_scene();
    let mut sectors = [Sector {
        strength_milli: 150_000,
        max_milli: 150_000,
        open: false,
    }; 4];
    // East at 29%, below Balanced's 30%; south lower still, but open.
    sectors[1].strength_milli = 43_500;
    sectors[2] = Sector {
        strength_milli: 0,
        max_milli: 150_000,
        open: true,
    };
    fake.defence.insert(
        MOTHER,
        HoldingDefence {
            dome: true,
            sectors,
        },
    );
    // Eight light hostiles within 3 km of the dome: Balanced's FLAK SCREEN count.
    let ships: Vec<(u64, ShipClass, f64)> = (0..8)
        .map(|i| (901 + i, ShipClass::RaiderStrikeCraft, 2_500.0))
        .collect();
    raiders(&mut fake, RAIDERS, &ships);
    let surge = Command::Holding {
        holding: MOTHER,
        order: HoldingOrder::Ability {
            ability: StationAbility::ShieldSurge { sector: 1 },
        },
    };
    let flak = Command::Holding {
        holding: MOTHER,
        order: HoldingOrder::Ability {
            ability: StationAbility::FlakScreen,
        },
    };
    let out = run(&mut fake, &mut memory);
    assert!(commands(&out).contains(&surge), "{:?}", commands(&out));
    // One ability at a time; once the surge is answered, the screen.
    applied(&mut fake, &out);
    fake.refused.push(surge.clone());
    let out = run(&mut fake, &mut memory);
    assert!(commands(&out).contains(&flak), "{:?}", commands(&out));
    // Seven are not enough, and Trader never screens.
    let mut fewer = engaged_scene();
    raiders(&mut fewer.0, RAIDERS, &ships[..7]);
    fewer.0.refused.push(surge);
    assert!(!commands(&run(&mut fewer.0, &mut fewer.1)).contains(&flak));
    let mut trader = engaged_scene();
    trader.0.holding_doctrine.flak_light_hostiles = None;
    raiders(&mut trader.0, RAIDERS, &ships);
    assert!(!commands(&run(&mut trader.0, &mut trader.1)).contains(&flak));
}

#[test]
fn patrols_and_escorts_run_their_orders_and_a_released_escort_falls_back() {
    let mut fake = scene();
    let route = vec![
        nomad_types::Location {
            system: system(),
            position: point(20_000.0, 0.0),
        },
        nomad_types::Location {
            system: system(),
            position: point(20_000.0, 20_000.0),
        },
    ];
    fake.fleet_roles.insert(
        GARRISON,
        FleetDoctrine {
            role: Some(FleetRole::Patrol {
                route: route.clone(),
            }),
            ..balanced().fleet()
        },
    );
    let mut memory = DeputyMemory::default();
    let out = run(&mut fake, &mut memory);
    assert_eq!(
        commands(&out),
        vec![
            to(GARRISON, FleetOrder::Launch, Queueing::Replace),
            to(GARRISON, FleetOrder::Patrol { route }, Queueing::Append),
        ]
    );
    // An escort of a fleet that is gone is released to its default role: the garrison of
    // the holding it is docked at, so it stays docked.
    let mut fake = scene();
    fake.fleet_roles.insert(
        GARRISON,
        FleetDoctrine {
            role: Some(FleetRole::Escort {
                fleet: FleetId(id(4_242)),
            }),
            ..balanced().fleet()
        },
    );
    let mut memory = DeputyMemory::default();
    assert!(commands(&run(&mut fake, &mut memory)).is_empty());
    // An escort of an own convoy in space guards it.
    let mut fake = scene();
    if let Some(f) = fake.fleets.get_mut(&RESERVE) {
        f.place = here(5_000.0);
    }
    fake.convoys.push(crate::view::OwnConvoy {
        fleet: RESERVE,
        route: vec![MOTHER],
        next: 0,
        held_at: None,
    });
    fake.fleet_roles.insert(
        GARRISON,
        FleetDoctrine {
            role: Some(FleetRole::Escort { fleet: RESERVE }),
            ..balanced().fleet()
        },
    );
    let mut memory = DeputyMemory::default();
    assert_eq!(
        commands(&run(&mut fake, &mut memory)),
        vec![
            to(GARRISON, FleetOrder::Launch, Queueing::Replace),
            to(
                GARRISON,
                FleetOrder::Guard {
                    object: RESERVE.sim_id()
                },
                Queueing::Append
            ),
        ]
    );
}

#[test]
fn a_released_fleet_in_space_returns_to_its_role() {
    // RK-10, D-162: once direct control ends, the fleet's doctrine applies again.
    let mut fake = scene();
    let mut memory = DeputyMemory::default();
    run(&mut fake, &mut memory);
    if let Some(f) = fake.fleets.get_mut(&GARRISON) {
        f.place = here(30_000.0);
    }
    fake.control
        .insert(GARRISON, DirectControl { retreat: false });
    assert!(commands(&run(&mut fake, &mut memory)).is_empty());
    fake.control.clear();
    let (mut full, mut full_memory) = (fake.clone(), memory.clone());
    assert_eq!(
        commands(&run(&mut fake, &mut memory)),
        vec![to(
            GARRISON,
            FleetOrder::Dock { at: MOTHER },
            Queueing::Replace
        )]
    );
    // Docking full: it station-keeps by the core instead (D-102).
    full.no_room.push(GARRISON);
    assert!(matches!(
        commands(&run(&mut full, &mut full_memory))[..],
        [Command::Fleet {
            order: FleetOrder::Move { .. },
            ..
        }]
    ));
}

/// D-071, A1 plan §5.6: a damaged fleet with no alert repairs at dock. A patrol above its
/// retreat threshold keeps patrolling, and a docked one launches again only once repaired;
/// an escort that is not engaged goes to dock.
#[test]
fn damaged_fleets_repair_at_dock_but_a_patrol_above_its_threshold_keeps_patrolling() {
    let route = vec![nomad_types::Location {
        system: system(),
        position: point(20_000.0, 0.0),
    }];
    let patrol = FleetOrder::Patrol {
        route: route.clone(),
    };
    let mut fake = scene();
    fake.fleet_roles.insert(
        GARRISON,
        FleetDoctrine {
            role: Some(FleetRole::Patrol { route }),
            ..balanced().fleet()
        },
    );
    if let Some(f) = fake.fleets.get_mut(&GARRISON) {
        f.hull = 80;
    }
    let mut memory = DeputyMemory::default();
    assert!(
        commands(&run(&mut fake, &mut memory)).is_empty(),
        "docked and damaged: it repairs first"
    );
    if let Some(f) = fake.fleets.get_mut(&GARRISON) {
        f.hull = 100;
    }
    assert_eq!(
        commands(&run(&mut fake, &mut memory)),
        vec![
            to(GARRISON, FleetOrder::Launch, Queueing::Replace),
            to(GARRISON, patrol.clone(), Queueing::Append),
        ]
    );
    // On its route at 80%, above Balanced's 40%: no return to dock.
    let mut fake = scene();
    let mut memory = DeputyMemory::default();
    run(&mut fake, &mut memory);
    fake.fleet_roles.insert(
        GARRISON,
        FleetDoctrine {
            role: Some(FleetRole::Patrol {
                route: vec![nomad_types::Location {
                    system: system(),
                    position: point(20_000.0, 0.0),
                }],
            }),
            ..balanced().fleet()
        },
    );
    if let Some(f) = fake.fleets.get_mut(&GARRISON) {
        f.place = here(20_000.0);
        f.hull = 80;
        f.order = Some(patrol);
    }
    assert!(commands(&run(&mut fake, &mut memory)).is_empty());
    // An escort at 80%, not engaged and its escortee not under fire: to dock.
    fake.fleet_roles.insert(
        GARRISON,
        FleetDoctrine {
            role: Some(FleetRole::Escort { fleet: RESERVE }),
            ..balanced().fleet()
        },
    );
    if let Some(f) = fake.fleets.get_mut(&RESERVE) {
        f.place = here(20_000.0);
    }
    let escort: Vec<Command> = commands(&run(&mut fake, &mut memory))
        .into_iter()
        .filter(|c| matches!(c, Command::Fleet { fleet, .. } if *fleet == GARRISON))
        .collect();
    assert_eq!(
        escort,
        vec![to(
            GARRISON,
            FleetOrder::Retreat { to: None },
            Queueing::Replace
        )]
    );
}
