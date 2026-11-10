//! Doctrine commands, presets and overrides, and direct control (A1 plan §5.1, §5.2, §9
//! "Validator" and "direct control").

use std::collections::BTreeSet;

use nomad_ai::{CommandOutcome, DropReason, InboxEvent, Layer};
use nomad_types::{
    Command, CommanderId, DoctrineCommand, DoctrinePreset, EconomyCommand, FleetDoctrine,
    FleetOrder, FleetRole, HoldingDoctrine, Location, Owner, PowerProfile, Queueing,
    ReplacementEntry, RulesOfEngagement,
};

use crate::ai::gate::submit_for_test;
use crate::command::tests::doctrine_reasons;
use crate::command::{CommandRejection as R, CommandSource, validate_command_from};
use crate::fleet::{FleetId, ShipClass};
use crate::geometry::Vec2;
use crate::input::{CommanderInput, CommanderInputKind, TickInputs};
use crate::state::UniverseState;
use crate::step::step;
use crate::testkit::{commander, home, idle, kit_fleet, send, site_position, spawn_fleet, stage1};
use crate::time::{Tick, Ticks};

fn doctrine(command: DoctrineCommand) -> Command {
    Command::Doctrine(command)
}

fn fleet_order(fleet: FleetId, order: FleetOrder, retreat: bool) -> Command {
    Command::Fleet {
        fleet,
        order,
        queue: Queueing::Replace,
        retreat,
    }
}

fn holding_doctrine() -> HoldingDoctrine {
    HoldingDoctrine {
        zone_radius_m: 5_000,
        zone_rules: RulesOfEngagement::HoldFire,
        alert_profile: PowerProfile::ProductionFirst,
        surge_below_permille: 100,
        flak_light_hostiles: Some(3),
        evacuation_radius_m: 4_000,
        rebuild_modules: false,
    }
}

fn fleet_doctrine(role: Option<FleetRole>) -> FleetDoctrine {
    FleetDoctrine {
        role,
        leash_m: 7_000,
        retreat_below_permille: 250,
        repair_at_dock: false,
    }
}

/// The last outcome of `owner`'s AI commands.
fn last_outcome(state: &UniverseState, owner: Owner) -> Option<CommandOutcome> {
    state.inbox(owner).iter().rev().find_map(|e| match e.event {
        InboxEvent::Outcome { outcome, .. } => Some(outcome),
        _ => None,
    })
}

/// Runs until `owner`'s AI commands have all had an outcome.
fn settle(state: &mut UniverseState, owner: Owner) {
    for _ in 0..30 {
        if state.ai_waiting(owner) == 0 {
            return;
        }
        step(state, &TickInputs::default());
    }
    panic!("AI commands still waiting");
}

fn submit(state: &mut UniverseState, n: u32, layer: Layer, command: Command) {
    let now = state.tick().next();
    submit_for_test(state, commander(n), layer, now, now, command);
}

#[test]
fn presets_and_overrides_resolve() {
    let mut state = stage1(1);
    let (mother, _) = home(&state, 1);
    let kit = kit_fleet(&state, 1);
    let rules = state.rules().ai.clone();
    // Balanced by default (D-082).
    assert_eq!(
        state.doctrine_preset(CommanderId(1)),
        DoctrinePreset::Balanced
    );
    assert_eq!(
        state.holding_doctrine(mother),
        Some(rules.preset(DoctrinePreset::Balanced).holding)
    );
    send(
        &mut state,
        1,
        doctrine(DoctrineCommand::SetPreset {
            preset: DoctrinePreset::Fortress,
        }),
    );
    assert_eq!(
        state.holding_doctrine(mother),
        Some(rules.preset(DoctrinePreset::Fortress).holding)
    );
    assert_eq!(
        state.fleet_doctrine(kit),
        Some(rules.preset(DoctrinePreset::Fortress).fleet())
    );
    // Overrides replace the preset's values; None follows the preset again.
    let out = send(
        &mut state,
        1,
        doctrine(DoctrineCommand::SetHolding {
            holding: mother,
            doctrine: Some(holding_doctrine()),
        }),
    );
    assert_eq!(out.commands[0].outcome, Ok(()));
    assert_eq!(state.holding_doctrine(mother), Some(holding_doctrine()));
    let role = Some(FleetRole::Garrison { holding: mother });
    send(
        &mut state,
        1,
        doctrine(DoctrineCommand::SetFleet {
            fleet: kit,
            doctrine: Some(fleet_doctrine(role.clone())),
        }),
    );
    assert_eq!(state.fleet_doctrine(kit), Some(fleet_doctrine(role)));
    send(
        &mut state,
        1,
        doctrine(DoctrineCommand::SetHolding {
            holding: mother,
            doctrine: None,
        }),
    );
    assert_eq!(
        state.holding_doctrine(mother),
        Some(rules.preset(DoctrinePreset::Fortress).holding)
    );
    // The replacement list, as set.
    let entries = vec![ReplacementEntry {
        fleet: kit,
        class: ShipClass::Vx,
        count: 3,
        role: None,
    }];
    send(
        &mut state,
        1,
        doctrine(DoctrineCommand::SetReplacement {
            entries: entries.clone(),
        }),
    );
    assert_eq!(state.doctrine(CommanderId(1)).replacement, entries);
    // Private: the own view shows it; another commander's doctrine is untouched.
    let own = state.view(commander(1)).own_doctrine().unwrap();
    assert_eq!(own.preset, DoctrinePreset::Fortress);
    assert_eq!(
        state.doctrine_preset(CommanderId(2)),
        DoctrinePreset::Balanced
    );
}

#[test]
#[expect(clippy::too_many_lines, reason = "one case per refusal, in one table")]
fn every_doctrine_refusal_is_produced() {
    let mut state = stage1(2);
    let (mother, system) = home(&state, 1);
    let (rival_mother, _) = home(&state, 2);
    let kit = kit_fleet(&state, 1);
    let rival_kit = kit_fleet(&state, 2);
    let mut seen = BTreeSet::new();
    let mut check = |state: &UniverseState, source, command: DoctrineCommand| {
        let result = validate_command_from(state, commander(1), source, &doctrine(command));
        if let Err(reason) = result {
            seen.insert(reason);
        }
        result
    };
    let client = CommandSource::Client;
    let bad_holding = HoldingDoctrine {
        surge_below_permille: 1_001,
        ..holding_doctrine()
    };
    let far = Location {
        system,
        position: nomad_types::PositionMetres::new(1.0e9, 0.0),
    };
    let cases: Vec<(CommandSource, DoctrineCommand, R)> = vec![
        (
            CommandSource::Ai(Layer::Deputy),
            DoctrineCommand::SetPreset {
                preset: DoctrinePreset::Trader,
            },
            R::DoctrineByDeputy,
        ),
        (
            client,
            DoctrineCommand::SetHolding {
                holding: mother,
                doctrine: Some(bad_holding),
            },
            R::InvalidDoctrine,
        ),
        (
            client,
            DoctrineCommand::SetHolding {
                holding: rival_mother,
                doctrine: None,
            },
            R::NotOwner,
        ),
        (
            client,
            DoctrineCommand::SetFleet {
                fleet: kit,
                doctrine: Some(FleetDoctrine {
                    leash_m: 0,
                    ..fleet_doctrine(None)
                }),
            },
            R::InvalidDoctrine,
        ),
        (
            client,
            DoctrineCommand::SetFleet {
                fleet: rival_kit,
                doctrine: None,
            },
            R::NotOwner,
        ),
        (
            client,
            DoctrineCommand::SetFleet {
                fleet: kit,
                doctrine: Some(fleet_doctrine(Some(FleetRole::Garrison {
                    holding: rival_mother,
                }))),
            },
            R::HoldingNotOwned,
        ),
        (
            client,
            DoctrineCommand::SetFleet {
                fleet: kit,
                doctrine: Some(fleet_doctrine(Some(FleetRole::Escort { fleet: kit }))),
            },
            R::SelfTarget,
        ),
        (
            client,
            DoctrineCommand::SetFleet {
                fleet: kit,
                doctrine: Some(fleet_doctrine(Some(FleetRole::Patrol { route: vec![] }))),
            },
            R::EmptyRoute,
        ),
        (
            client,
            DoctrineCommand::SetFleet {
                fleet: kit,
                doctrine: Some(fleet_doctrine(Some(FleetRole::Patrol { route: vec![far] }))),
            },
            R::InvalidPosition,
        ),
        (
            client,
            DoctrineCommand::SetReplacement {
                entries: vec![
                    ReplacementEntry {
                        fleet: kit,
                        class: ShipClass::Vx,
                        count: 3,
                        role: None,
                    };
                    2
                ],
            },
            R::InvalidDoctrine,
        ),
        (
            client,
            DoctrineCommand::SetReplacement {
                entries: vec![ReplacementEntry {
                    fleet: kit,
                    class: ShipClass::RaiderStrikeCraft,
                    count: 1,
                    role: None,
                }],
            },
            R::NotBuildable,
        ),
        (
            client,
            DoctrineCommand::SetReplacement {
                entries: vec![ReplacementEntry {
                    fleet: kit,
                    class: ShipClass::Vx,
                    count: 0,
                    role: None,
                }],
            },
            R::InvalidDoctrine,
        ),
        (
            client,
            DoctrineCommand::Release { fleet: kit },
            R::NotUnderDirectControl,
        ),
    ];
    for (source, command, expected) in cases {
        assert_eq!(
            check(&state, source, command.clone()),
            Err(expected),
            "{command:?}"
        );
    }
    // Too many entries: one more than the rules allow.
    let max = state.rules().ai.max_replacement_entries;
    let classes = [
        ShipClass::Vx,
        ShipClass::Lancer,
        ShipClass::Warden,
        ShipClass::Mule,
    ];
    let base = site_position(&state, system, 0) + Vec2::new(9_000.0, 0.0);
    let fleets: Vec<FleetId> = (0..=max / 4)
        .map(|i| {
            spawn_fleet(
                &mut state,
                commander(1),
                system,
                base + Vec2::new(0.0, f64::from(i) * 500.0),
                &[ShipClass::Vx],
            )
        })
        .collect();
    let entries: Vec<ReplacementEntry> = fleets
        .iter()
        .flat_map(|&fleet| {
            classes.iter().map(move |&class| ReplacementEntry {
                fleet,
                class,
                count: 1,
                role: None,
            })
        })
        .take(usize::try_from(max).unwrap() + 1)
        .collect();
    let check = |state: &UniverseState, command| {
        validate_command_from(state, commander(1), client, &doctrine(command))
    };
    assert_eq!(
        check(&state, DoctrineCommand::SetReplacement { entries }),
        Err(R::InvalidDoctrine)
    );
    // An NPC faction has no doctrine.
    assert_eq!(
        validate_command_from(
            &state,
            Owner::Npc(nomad_types::FactionId(1)),
            client,
            &doctrine(DoctrineCommand::SetPreset {
                preset: DoctrinePreset::Fortress
            })
        ),
        Err(R::UnknownCommander)
    );
    let produced: BTreeSet<R> = seen.intersection(&doctrine_reasons()).copied().collect();
    assert_eq!(produced, doctrine_reasons());
}

#[test]
fn a_direct_order_takes_control_until_release() {
    let mut state = stage1(3);
    let kit = kit_fleet(&state, 1);
    assert_eq!(state.direct_control(kit), None);
    send(&mut state, 1, fleet_order(kit, FleetOrder::Launch, false));
    assert_eq!(
        state.direct_control(kit),
        Some(nomad_types::DirectControl { retreat: false })
    );
    assert!(state.fleet(kit).unwrap().direct_control.is_some());
    // The Deputy's order for it is dropped when it would apply, and uses no rate.
    let (mother, _) = home(&state, 1);
    submit(
        &mut state,
        1,
        Layer::Deputy,
        fleet_order(kit, FleetOrder::Dock { at: mother }, false),
    );
    settle(&mut state, commander(1));
    assert_eq!(
        last_outcome(&state, commander(1)),
        Some(CommandOutcome::Dropped(DropReason::DirectControl))
    );
    assert_eq!(
        state.ai_counters()[&(commander(1), Layer::Deputy)]
            .dropped
            .get(&DropReason::DirectControl),
        Some(&1)
    );
    // Release: doctrine applies again, and the Deputy's order goes through.
    let out = send(
        &mut state,
        1,
        doctrine(DoctrineCommand::Release { fleet: kit }),
    );
    assert_eq!(out.commands[0].outcome, Ok(()));
    assert_eq!(state.direct_control(kit), None);
    submit(
        &mut state,
        1,
        Layer::Deputy,
        fleet_order(kit, FleetOrder::Dock { at: mother }, false),
    );
    settle(&mut state, commander(1));
    assert_eq!(
        last_outcome(&state, commander(1)),
        Some(CommandOutcome::Applied)
    );
    // A Deputy order does not take control; a Strategist order does (A1 plan §5.2).
    assert_eq!(state.direct_control(kit), None);
    submit(
        &mut state,
        1,
        Layer::Strategist,
        fleet_order(
            kit,
            FleetOrder::SetRulesOfEngagement {
                rules: RulesOfEngagement::ReturnFire,
            },
            false,
        ),
    );
    settle(&mut state, commander(1));
    assert!(state.direct_control(kit).is_some());
    // The Deputy may not release it either.
    submit(
        &mut state,
        1,
        Layer::Deputy,
        doctrine(DoctrineCommand::Release { fleet: kit }),
    );
    settle(&mut state, commander(1));
    assert_eq!(
        state.ai_counters()[&(commander(1), Layer::Deputy)]
            .rejected
            .get(&R::DoctrineByDeputy),
        Some(&1)
    );
    assert!(state.direct_control(kit).is_some());
}

#[test]
fn only_the_opted_in_retreat_overrides_a_direct_order() {
    let mut state = stage1(4);
    let (_, system) = home(&state, 1);
    let spot = site_position(&state, system, 0) + Vec2::new(9_000.0, 0.0);
    let fleet = spawn_fleet(&mut state, commander(1), system, spot, &[ShipClass::Vx]);
    let patrol = FleetOrder::Patrol {
        route: vec![crate::testkit::at(system, spot + Vec2::new(2_000.0, 0.0))],
    };
    let retreat = fleet_order(fleet, FleetOrder::Retreat { to: None }, false);
    // Without the opt-in the Deputy's retreat is dropped.
    send(&mut state, 1, fleet_order(fleet, patrol.clone(), false));
    submit(&mut state, 1, Layer::Deputy, retreat.clone());
    settle(&mut state, commander(1));
    assert_eq!(
        last_outcome(&state, commander(1)),
        Some(CommandOutcome::Dropped(DropReason::DirectControl))
    );
    assert!(matches!(
        state.fleet(fleet).unwrap().order,
        Some(FleetOrder::Patrol { .. })
    ));
    // With it, it applies; other Deputy orders are still dropped.
    send(&mut state, 1, fleet_order(fleet, patrol, true));
    submit(
        &mut state,
        1,
        Layer::Deputy,
        fleet_order(fleet, FleetOrder::StationKeep { relative_to: None }, false),
    );
    settle(&mut state, commander(1));
    assert_eq!(
        last_outcome(&state, commander(1)),
        Some(CommandOutcome::Dropped(DropReason::DirectControl))
    );
    submit(&mut state, 1, Layer::Deputy, retreat);
    settle(&mut state, commander(1));
    assert_eq!(
        last_outcome(&state, commander(1)),
        Some(CommandOutcome::Applied)
    );
    assert!(matches!(
        state.fleet(fleet).unwrap().order,
        Some(FleetOrder::Retreat { .. })
    ));
}

#[test]
fn splits_inherit_direct_control_and_convoys_release_it() {
    let mut state = stage1(5);
    let kit = kit_fleet(&state, 1);
    let ships = state.fleet(kit).unwrap().ships;
    send(
        &mut state,
        1,
        fleet_order(
            kit,
            FleetOrder::SetRulesOfEngagement {
                rules: RulesOfEngagement::HoldFire,
            },
            true,
        ),
    );
    send(
        &mut state,
        1,
        Command::CreateFleet {
            ships: vec![ships[0]],
        },
    );
    let split = state.ship(ships[0]).unwrap().fleet;
    assert_ne!(split, kit);
    assert_eq!(
        state.direct_control(split),
        Some(nomad_types::DirectControl { retreat: true })
    );
    // The kit's MULEs (their own fleet, D-286) under a direct order, then as a convoy:
    // doctrine, so released (D-162).
    let convoy = crate::testkit::kit_mules(&state, 1);
    send(
        &mut state,
        1,
        fleet_order(
            convoy,
            FleetOrder::SetRulesOfEngagement {
                rules: RulesOfEngagement::HoldFire,
            },
            false,
        ),
    );
    assert!(state.direct_control(convoy).is_some());
    let (mother, _) = home(&state, 1);
    let out = send(
        &mut state,
        1,
        Command::Economy(EconomyCommand::Convoy {
            fleet: convoy,
            route: vec![mother],
        }),
    );
    assert_eq!(out.commands[0].outcome, Ok(()));
    assert_eq!(state.direct_control(convoy), None);
}

#[test]
fn the_lapse_ends_direct_control_but_not_the_deputys_orders() {
    let mut state = stage1(6);
    let connect = |state: &mut UniverseState, seq, kind| {
        step(
            state,
            &TickInputs {
                operator: vec![],
                commanders: vec![CommanderInput {
                    commander: CommanderId(1),
                    client_seq: seq,
                    kind,
                }],
            },
        );
    };
    connect(&mut state, 1, CommanderInputKind::Connected);
    let (_, system) = home(&state, 1);
    let spot = site_position(&state, system, 0) + Vec2::new(9_000.0, 0.0);
    let direct = spawn_fleet(&mut state, commander(1), system, spot, &[ShipClass::Vx]);
    let doctrine_fleet = spawn_fleet(
        &mut state,
        commander(1),
        system,
        spot + Vec2::new(0.0, 2_000.0),
        &[ShipClass::Vx],
    );
    let patrol = |p: Vec2| FleetOrder::Patrol {
        route: vec![crate::testkit::at(system, p)],
    };
    send(
        &mut state,
        1,
        fleet_order(direct, patrol(spot + Vec2::new(3_000.0, 0.0)), false),
    );
    submit(
        &mut state,
        1,
        Layer::Deputy,
        fleet_order(
            doctrine_fleet,
            patrol(spot + Vec2::new(3_000.0, 2_000.0)),
            false,
        ),
    );
    settle(&mut state, commander(1));
    connect(&mut state, 2, CommanderInputKind::Disconnected);
    let grace = Ticks::from_secs(state.rules().sieges.disconnect_grace_s).get();
    let outs = idle(&mut state, grace + 1);
    let lapsed: Vec<FleetId> = outs
        .iter()
        .flat_map(|o| o.announcements.iter())
        .filter_map(|a| match &a.kind {
            crate::announce::AnnouncementKind::OrdersLapsed { fleets, .. } => Some(fleets.clone()),
            _ => None,
        })
        .flatten()
        .collect();
    assert_eq!(lapsed, vec![direct]);
    assert_eq!(state.direct_control(direct), None);
    assert_eq!(
        state.fleet(direct).unwrap().order,
        Some(FleetOrder::StationKeep { relative_to: None })
    );
    // The Deputy's order is doctrine and stands (D-162).
    assert!(matches!(
        state.fleet(doctrine_fleet).unwrap().order,
        Some(FleetOrder::Patrol { .. })
    ));
    assert!(state.tick() > Tick::new(grace));
}
