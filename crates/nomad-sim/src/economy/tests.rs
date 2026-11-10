//! Economy tests (S4 plan §8 "Done when").

use std::collections::BTreeSet;

use nomad_types::{
    BuildItem, Command, DepositKind, EconomyCommand, FleetOrder, ModuleKind, PowerProfile,
    ShipClass, Slot,
};

use super::power::{self, Consumer};
use super::*;
use crate::command::CommandRejection as R;
use crate::command::tests::economy_reasons;
use crate::fleet::{Fleet, Motion, Place};
use crate::rules::Rules;
use crate::snapshot::{decode, encode, state_hash};
use crate::testkit::{commander, home, idle, kit_fleet, kit_mules, order, send, stage1};
use crate::validate_command;

const ONE_HOUR: u64 = 3_600 * TICKS_PER_SECOND;

fn eco(command: EconomyCommand) -> Command {
    Command::Economy(command)
}

fn events(outputs: &[crate::TickOutputs]) -> Vec<EconomyEvent> {
    outputs
        .iter()
        .flat_map(|o| o.economy_events.iter().copied())
        .collect()
}

fn check(state: &UniverseState, n: u32, command: EconomyCommand) -> Result<(), R> {
    validate_command(state, commander(n), &eco(command))
}

/// The id of the queue item an `ItemQueued` event names, from one tick's outputs.
fn queued(outputs: &crate::TickOutputs) -> SimId {
    outputs
        .economy_events
        .iter()
        .find_map(|e| match e {
            EconomyEvent::ItemQueued { item, .. } => Some(*item),
            _ => None,
        })
        .expect("the item was queued")
}

#[test]
fn flows_are_exact_over_a_day() {
    for rate in [600_000_u64, 900_000, 60_000, 333_333, 1] {
        let mut flow = Flow::default();
        let total: u64 = (0..86_400).map(|_| flow.second(rate)).sum();
        assert_eq!(total, rate * 24, "rate {rate}");
    }
    // A rate change restarts the count without losing exactness from then on.
    let mut flow = Flow::default();
    let first: u64 = (0..1_800).map(|_| flow.second(600_000)).sum();
    let second: u64 = (0..3_600).map(|_| flow.second(300_000)).sum();
    assert_eq!((first, second), (300_000, 300_000));
}

#[test]
fn the_start_station_is_the_start_kit() {
    let state = stage1(7);
    let (mother, _) = home(&state, 1);
    let modules = state.modules_at(mother);
    let kinds: Vec<ModuleKind> = modules.iter().map(|m| m.kind).collect();
    assert_eq!(kinds.len(), 10, "{kinds:?}");
    for kind in [
        ModuleKind::Core,
        ModuleKind::Solar,
        ModuleKind::Harvester,
        ModuleKind::Refinery,
        ModuleKind::Storage,
        ModuleKind::DryDock,
    ] {
        assert_eq!(kinds.iter().filter(|&&k| k == kind).count(), 1, "{kind:?}");
    }
    assert_eq!(kinds.iter().filter(|&&k| k == ModuleKind::Pylon).count(), 4);
    assert!(
        modules
            .iter()
            .all(|m| m.building_ms.is_none() && !m.disabled)
    );
    assert_eq!(state.pool(CommanderId(1)), Stock::units(6_000, 500));
    // D-282: base 5,000 A / 600 C plus one rack's 1,500 A / 400 C.
    assert_eq!(state.pool_cap(CommanderId(1)), Stock::units(6_500, 1_000));
    assert_eq!(state.dry_docks(mother), 1);
    assert_eq!(state.docking_capacity(mother), 30);
    assert_eq!(state.pylons_per_sector(mother), [true; 4]);
    assert_eq!(
        (
            state.command_cap(CommanderId(1)),
            state.command_used(CommanderId(1))
        ),
        (120, 26)
    );
    assert!(state.is_depot(mother) && state.delivers_to_pool(mother));
    assert!(
        state.economy_problems().is_empty(),
        "{:?}",
        state.economy_problems()
    );
    // The mother site has exactly its standard ore field (D-098).
    let site = state
        .galaxy()
        .site(state.holding(mother).unwrap().site)
        .unwrap();
    assert_eq!(site.deposits.len(), 1);
    assert_eq!(site.deposits[0].kind, DepositKind::OreField);
}

#[test]
fn the_start_kit_produces_600_alloy_an_hour_exactly() {
    let mut state = stage1(7);
    idle(&mut state, ONE_HOUR);
    for n in 1..=4 {
        let economy = state.economy_of(CommanderId(n));
        assert_eq!(economy.produced, Stock::units(600, 0), "commander {n}");
        assert_eq!(economy.income_last_hour, Stock::units(600, 0));
        // D-282: the 6,500 A start cap is full 50 min in if nothing is spent.
        assert_eq!(economy.pool, Stock::units(6_500, 500));
        assert_eq!(economy.lost, Stock::units(100, 0));
    }
    // Power at the start: 1,140 MW supply, 340 MW demand, all served.
    let (mother, _) = home(&state, 1);
    let power = state.power(mother).unwrap();
    assert_eq!(
        (power.supply_kw, power.demand_kw, power.allocated_kw),
        (1_140_000, 340_000, 340_000)
    );
    assert_eq!(power.load_permille(), 298);
}

#[test]
fn a_full_pool_clips_and_counts_the_overflow() {
    let mut state = stage1(7);
    let you = CommanderId(1);
    let cap = state.pool_cap(you);
    state.economy.pools.insert(
        you,
        Stock {
            alloy: cap.alloy - 100,
            crystal: 0,
        },
    );
    let out = idle(&mut state, 100);
    assert_eq!(state.pool(you).alloy, cap.alloy);
    let lost = state.economy_of(you).lost;
    assert!(lost.alloy > 0);
    assert_eq!(
        state.economy_of(you).produced.alloy,
        100 + lost.alloy,
        "everything produced is either kept or counted as lost"
    );
    assert!(
        events(&out).iter().any(
            |e| matches!(e, EconomyEvent::PoolOverflow { commander, .. } if *commander == you)
        )
    );
}

/// Places a completed holding of commander 1 on a free site with an ore field in another
/// system than its mother station, with a harvester, a refinery and a solar array.
fn remote_ore_holding(state: &mut UniverseState) -> HoldingId {
    let (_, home_system) = home(state, 1);
    let site = state
        .galaxy()
        .sites()
        .find(|(id, s)| {
            s.system != home_system
                && state.holding_at(*id).is_none()
                && s.deposits
                    .first()
                    .is_some_and(|d| d.kind == DepositKind::OreField)
                && state
                    .galaxy()
                    .system(s.system)
                    .is_some_and(|sys| crate::galaxy::is_claimable(sys.kind))
        })
        .map(|(id, _)| id)
        .expect("a free ore site elsewhere");
    let holding = state.insert_holding(site, CommanderId(1), true).unwrap();
    spawn_module(state, holding, Slot::Core, ModuleKind::Core, None);
    spawn_module(
        state,
        holding,
        Slot::Deposit(0),
        ModuleKind::Harvester,
        None,
    );
    spawn_module(state, holding, Slot::Inner(0), ModuleKind::Refinery, None);
    spawn_module(state, holding, Slot::Solar, ModuleKind::Solar, None);
    holding
}

#[test]
fn a_remote_buffer_fills_to_two_hours_and_stops() {
    let mut state = stage1(7);
    let remote = remote_ore_holding(&mut state);
    assert!(!state.delivers_to_pool(remote));
    let richness = state
        .galaxy()
        .site(state.holding(remote).unwrap().site)
        .unwrap()
        .deposits[0]
        .richness;
    let per_h = 600 * richness_permille(richness);
    assert_eq!(
        state.buffer_cap(remote),
        Stock {
            alloy: per_h * 2,
            crystal: 0
        }
    );
    state.economy.holdings.get_mut(&remote).unwrap().buffer = Stock {
        alloy: per_h * 2 - 50,
        crystal: 0,
    };
    let out = idle(&mut state, 100);
    let view = state.holding_economy(remote).unwrap();
    assert_eq!(view.buffer, view.buffer_cap);
    assert!(
        events(&out).iter().any(
            |e| matches!(e, EconomyEvent::BufferOverflow { holding, .. } if *holding == remote)
        )
    );
    // Remote production went to the buffer, not the pool.
    assert_eq!(
        state.pool(CommanderId(1)).alloy,
        6_000_000 + 100 * 600_000 / 36_000
    );
}

#[test]
fn power_follows_each_profile_with_all_or_nothing_fixed_draws() {
    let mut state = stage1(7);
    let remote = remote_ore_holding(&mut state);
    // Take the solar array away: 120 MW from the core only.
    let solar = state.economy.holdings[&remote].slots[&Slot::Solar];
    let mut ev = Vec::new();
    assert!(destroy_module(&mut state, solar, &mut ev));
    // Four turrets ask for 160 MW of fixed power: three get it, the fourth none.
    for i in 1..=3 {
        spawn_module(&mut state, remote, Slot::Inner(i), ModuleKind::Turret, None);
    }
    spawn_module(&mut state, remote, Slot::Pylon(0), ModuleKind::Turret, None);
    let now = state.tick();
    power::allocate(&mut state, remote, now);
    let p = state.power(remote).unwrap().clone();
    assert_eq!(p.supply_kw, 120_000);
    let fixed: Vec<bool> = p
        .consumers
        .iter()
        .filter(|c| c.demand_kw == 40_000)
        .map(|c| c.allocated_kw == c.demand_kw)
        .collect();
    assert_eq!(fixed, [true, true, true, false]);
    assert_eq!(p.headroom_kw, 0);

    // Remove two turrets: 40 MW left after the fixed draw for scalable consumers.
    for slot in [Slot::Inner(1), Slot::Inner(2)] {
        let id = state.economy.holdings[&remote].slots[&slot];
        destroy_module(&mut state, id, &mut ev);
    }
    let pylon_slot_turret = state.economy.holdings[&remote].slots[&Slot::Pylon(0)];
    destroy_module(&mut state, pylon_slot_turret, &mut ev);
    state
        .economy
        .holdings
        .get_mut(&remote)
        .unwrap()
        .regen_demand_kw = [30_000, 0, 0, 0];
    let harvester = state.economy.holdings[&remote].slots[&Slot::Deposit(0)];
    let refinery = state.economy.holdings[&remote].slots[&Slot::Inner(0)];
    let share = |state: &mut UniverseState, profile| {
        state.economy.holdings.get_mut(&remote).unwrap().profile = profile;
        let now = state.tick();
        power::allocate(state, remote, now);
        let p = state.power(remote).unwrap();
        (
            p.regeneration_kw(0),
            p.share(Consumer::Module(harvester)).unwrap().0,
            p.share(Consumer::Module(refinery)).unwrap().0,
        )
    };
    // 80 MW left: regeneration 30 (capped at 25% without a pylon) + production 180.
    // Balanced: one tier of 210 MW sharing 80 MW.
    assert_eq!(
        share(&mut state, PowerProfile::Balanced),
        (11_428, 22_857, 45_714)
    );
    // Shields first: regeneration in full, production shares the rest.
    assert_eq!(
        share(&mut state, PowerProfile::ShieldsFirst),
        (30_000, 16_666, 33_333)
    );
    // Production first: production takes everything; the rounding remainder (1 kW) flows on
    // to the next tier.
    assert_eq!(
        share(&mut state, PowerProfile::ProductionFirst),
        (1, 26_666, 53_333)
    );
}

#[test]
fn a_disabled_solar_array_lowers_regeneration_and_production() {
    let mut state = stage1(7);
    let (mother, _) = home(&state, 1);
    state
        .economy
        .holdings
        .get_mut(&mother)
        .unwrap()
        .regen_demand_kw = [120_000; 4];
    let now = state.tick();
    power::allocate(&mut state, mother, now);
    let full = state.power(mother).unwrap().clone();
    assert_eq!(full.supply_kw, 1_140_000);
    assert_eq!(full.regeneration_kw(0), 120_000);
    let solar = state.economy.holdings[&mother].slots[&Slot::Solar];
    let mut ev = Vec::new();
    disable_module(&mut state, solar, now, &mut ev);
    assert_eq!(ev, [EconomyEvent::ModuleDisabled { module: solar }]);
    power::allocate(&mut state, mother, now);
    let cut = state.power(mother).unwrap().clone();
    assert_eq!(cut.supply_kw, 520_000);
    assert!(cut.regeneration_kw(0) < 120_000);
    let harvester = state.economy.holdings[&mother].slots[&Slot::Deposit(0)];
    let (allocated, demand) = cut.share(Consumer::Module(harvester)).unwrap();
    assert!(allocated < demand);
    // Restored, it supplies again.
    restore_module(&mut state, solar, &mut ev);
    power::allocate(&mut state, mother, now);
    assert_eq!(state.power(mother).unwrap().supply_kw, 1_140_000);
}

#[test]
fn abilities_claim_fixed_power_only_if_it_fits() {
    let mut state = stage1(7);
    let (mother, _) = home(&state, 1);
    let now = state.tick();
    power::allocate(&mut state, mother, now);
    let headroom = state.power(mother).unwrap().headroom_kw;
    assert_eq!(headroom, 1_140_000 - 160_000);
    let until = now.after(crate::time::Ticks::from_secs(10));
    assert!(request_ability_power(
        &mut state, mother, 200_000, now, until
    ));
    assert_eq!(state.power(mother).unwrap().headroom_kw, headroom - 200_000);
    assert!(!request_ability_power(
        &mut state, mother, 900_000, now, until
    ));
    // It ends on time.
    power::allocate(&mut state, mother, until);
    assert_eq!(state.power(mother).unwrap().headroom_kw, headroom);
}

#[test]
fn queues_pay_at_start_wait_without_blocking_and_refund_on_cancel() {
    let mut state = stage1(7);
    let (mother, _) = home(&state, 1);
    let you = CommanderId(1);
    // A WARDEN needs 600 crystal; the pool has 500. It waits; the VX behind it starts.
    let first = send(
        &mut state,
        1,
        eco(EconomyCommand::Build {
            holding: mother,
            item: BuildItem::Ship {
                class: ShipClass::Warden,
            },
        }),
    );
    let warden = queued(&first);
    let second = send(
        &mut state,
        1,
        eco(EconomyCommand::Build {
            holding: mother,
            item: BuildItem::Ship {
                class: ShipClass::Vx,
            },
        }),
    );
    let vx = queued(&second);
    let mut out = vec![first, second];
    out.extend(idle(&mut state, 10));
    let ev = events(&out);
    assert!(ev.contains(&EconomyEvent::ItemWaiting {
        holding: mother,
        item: warden,
        short: Shortage::Crystal
    }));
    assert!(ev.contains(&EconomyEvent::ItemStarted {
        holding: mother,
        item: vx
    }));
    assert!(command::waiting(&state, mother, warden));
    assert!(state.pool(you).alloy < 6_000_000 - 1_200_000 + 1_000);

    // Cancelling the waiting item refunds nothing (nothing was paid); cancelling the
    // building VX refunds half.
    let before = state.pool(you);
    let out = send(
        &mut state,
        1,
        eco(EconomyCommand::Cancel {
            holding: mother,
            item: vx,
        }),
    );
    assert!(out.economy_events.contains(&EconomyEvent::ItemCancelled {
        holding: mother,
        item: vx,
        refund: Stock::units(600, 0)
    }));
    assert!(state.pool(you).alloy >= before.alloy + 600_000);
    let out = send(
        &mut state,
        1,
        eco(EconomyCommand::Cancel {
            holding: mother,
            item: warden,
        }),
    );
    assert!(out.economy_events.contains(&EconomyEvent::ItemCancelled {
        holding: mother,
        item: warden,
        refund: Stock::ZERO
    }));
    assert!(state.holding_economy(mother).unwrap().queue.is_empty());
}

#[test]
fn a_dry_dock_adds_a_parallel_ship_lane_and_ships_dock_when_done() {
    let mut state = stage1(7);
    let (mother, _) = home(&state, 1);
    let dock = state.economy.holdings[&mother].slots[&Slot::Dock];
    let a = queued(&send(
        &mut state,
        1,
        eco(EconomyCommand::Build {
            holding: mother,
            item: BuildItem::Ship {
                class: ShipClass::Vx,
            },
        }),
    ));
    let b = queued(&send(
        &mut state,
        1,
        eco(EconomyCommand::Build {
            holding: mother,
            item: BuildItem::Ship {
                class: ShipClass::Mule,
            },
        }),
    ));
    idle(&mut state, 10);
    let queue = state.holding_economy(mother).unwrap().queue;
    let lanes: Vec<Option<BuildLane>> = queue.iter().map(|q| q.building.map(|b| b.0)).collect();
    assert_eq!(lanes, [Some(BuildLane::Main), Some(BuildLane::Dock(dock))]);
    // The building dock draws 150 MW.
    idle(&mut state, 10);
    assert_eq!(
        state.power(mother).unwrap().share(Consumer::Module(dock)),
        Some((150_000, 150_000))
    );
    let docked_before = state.docked_ships(mother);
    let out = idle(&mut state, 300 * TICKS_PER_SECOND);
    let ev = events(&out);
    assert!(ev.contains(&EconomyEvent::ItemCompleted {
        holding: mother,
        item: a
    }));
    assert!(ev.contains(&EconomyEvent::ItemCompleted {
        holding: mother,
        item: b
    }));
    assert_eq!(
        ev.iter()
            .filter(|e| matches!(e, EconomyEvent::ShipBuilt { .. }))
            .count(),
        2
    );
    assert_eq!(state.docked_ships(mother), docked_before + 2);
    assert_eq!(state.command_used(CommanderId(1)), 26 + 6 + 4);
    assert!(
        state.fleet_problems().is_empty(),
        "{:?}",
        state.fleet_problems()
    );
}

#[test]
fn rally_fleets_take_new_ships() {
    let mut state = stage1(7);
    let (mother, _) = home(&state, 1);
    let kit = kit_fleet(&state, 1);
    send(
        &mut state,
        1,
        eco(EconomyCommand::SetRally {
            holding: mother,
            fleet: Some(kit),
        }),
    );
    send(
        &mut state,
        1,
        eco(EconomyCommand::Build {
            holding: mother,
            item: BuildItem::Ship {
                class: ShipClass::Vx,
            },
        }),
    );
    let out = idle(&mut state, 190 * TICKS_PER_SECOND);
    let built = events(&out).into_iter().find_map(|e| match e {
        EconomyEvent::ShipBuilt { fleet, .. } => Some(fleet),
        _ => None,
    });
    assert_eq!(built, Some(kit));
    assert_eq!(state.fleet(kit).unwrap().ships.len(), 4);
    assert!(state.fleet_problems().is_empty());
}

#[test]
fn the_command_cap_blocks_starts_not_existing_ships() {
    let mut state = stage1(7);
    let (mother, _) = home(&state, 1);
    state.rules.economy.command_cap.base = 30;
    assert_eq!(state.command_cap(CommanderId(1)), 30);
    let first = send(
        &mut state,
        1,
        eco(EconomyCommand::Build {
            holding: mother,
            item: BuildItem::Ship {
                class: ShipClass::Vx,
            },
        }),
    );
    let vx = queued(&first);
    let mut out = vec![first];
    out.extend(idle(&mut state, 20));
    assert!(events(&out).contains(&EconomyEvent::ItemWaiting {
        holding: mother,
        item: vx,
        short: Shortage::CommandCap
    }));
    // A MULE fits (26 + 4 = 30), and nothing was taken from the existing fleet.
    send(
        &mut state,
        1,
        eco(EconomyCommand::Build {
            holding: mother,
            item: BuildItem::Ship {
                class: ShipClass::Mule,
            },
        }),
    );
    idle(&mut state, 20);
    assert_eq!(state.command_used(CommanderId(1)), 30);
    // Lowering the cap below what exists keeps the ships.
    state.rules.economy.command_cap.base = 10;
    idle(&mut state, 20);
    assert_eq!(state.command_used(CommanderId(1)), 30);
}

#[test]
fn modules_build_in_their_slot_and_count_when_complete() {
    let mut state = stage1(7);
    let (mother, _) = home(&state, 1);
    let item = queued(&send(
        &mut state,
        1,
        eco(EconomyCommand::Build {
            holding: mother,
            item: BuildItem::Module {
                kind: ModuleKind::Turret,
                slot: Slot::Inner(2),
            },
        }),
    ));
    idle(&mut state, 10);
    let turret = state.economy.holdings[&mother].slots[&Slot::Inner(2)];
    let view = state.module(turret).unwrap();
    assert_eq!((view.kind, view.hp), (ModuleKind::Turret, 30_000));
    assert!(view.building_ms.is_some());
    // Under construction it draws nothing.
    assert_eq!(
        state.power(mother).unwrap().share(Consumer::Module(turret)),
        None
    );
    let out = idle(&mut state, 1_200 * TICKS_PER_SECOND);
    assert!(events(&out).contains(&EconomyEvent::ItemCompleted {
        holding: mother,
        item
    }));
    assert_eq!(state.module(turret).unwrap().building_ms, None);
    assert!(state.power(mother).unwrap().powered(turret));
    // 500 C at the start less the turret's 50 C (D-309).
    assert_eq!(state.pool(CommanderId(1)).crystal, 450_000);
    assert!(state.economy_problems().is_empty());
    // Destroying it frees the slot.
    let mut ev = Vec::new();
    assert!(destroy_module(&mut state, turret, &mut ev));
    assert!(
        !state.economy.holdings[&mother]
            .slots
            .contains_key(&Slot::Inner(2))
    );
    assert!(state.economy_problems().is_empty());
    // A core cannot be destroyed here (capture is S6's).
    let core = state.economy.holdings[&mother].slots[&Slot::Core];
    assert!(!destroy_module(&mut state, core, &mut ev));
}

#[test]
fn a_core_kit_is_built_loaded_deployed_and_completes_as_a_holding() {
    let mut state = stage1(7);
    let (mother, home_system) = home(&state, 1);
    let you = CommanderId(1);
    // D-189: the kit counts once towards the next kit's price at every stage, queued,
    // stored, carried, deployed and complete.
    let counts_once = |state: &UniverseState, stage: &str| {
        assert_eq!(state.expansion_count(you), 1, "{stage}");
        assert_eq!(
            state.economy_of(you).core_kit_price,
            Stock::units(7_500, 625),
            "{stage}"
        );
    };
    assert_eq!(state.expansion_count(you), 0);
    assert_eq!(state.core_kit_price(you), Stock::units(6_000, 500));
    let item = queued(&send(
        &mut state,
        1,
        eco(EconomyCommand::Build {
            holding: mother,
            item: BuildItem::CoreKit,
        }),
    ));
    counts_once(&state, "queued");
    let out = idle(&mut state, 900 * TICKS_PER_SECOND + 10);
    assert!(events(&out).contains(&EconomyEvent::ItemCompleted {
        holding: mother,
        item
    }));
    assert_eq!(state.holding_economy(mother).unwrap().kits, 1);
    assert_eq!(state.pool(you).crystal, 0);
    counts_once(&state, "stored");

    let kit = kit_mules(&state, 1);
    let out = send(
        &mut state,
        1,
        eco(EconomyCommand::LoadCoreKit { fleet: kit }),
    );
    assert!(out.economy_events.contains(&EconomyEvent::KitLoaded {
        fleet: kit,
        holding: mother
    }));
    let fleet = state.entities.get::<Fleet>(kit.sim_id()).unwrap();
    let carrier = logistics::kit_carrier(&state, &fleet).expect("a MULE carries the kit");
    assert!(state.cargo(carrier).core_kit);
    counts_once(&state, "carried");

    // Fly next to a free site of the home system (placed there for the test).
    let site = state
        .galaxy()
        .system(home_system)
        .unwrap()
        .sites
        .iter()
        .copied()
        .find(|&s| state.holding_at(s).is_none())
        .unwrap();
    let target = state.galaxy().site(site).unwrap().position;
    send(&mut state, 1, order(kit, FleetOrder::Launch));
    teleport(&mut state, kit, home_system, target);
    let deploy = EconomyCommand::DeployCoreKit { fleet: kit, site };
    let out = send(&mut state, 1, eco(deploy.clone()));
    let outpost = out
        .economy_events
        .iter()
        .find_map(|e| match e {
            EconomyEvent::OutpostDeployed { holding, .. } => Some(*holding),
            _ => None,
        })
        .expect("deployed");
    assert!(!state.holding(outpost).unwrap().core_complete);
    assert!(!state.cargo(carrier).core_kit);
    counts_once(&state, "outpost");
    let core = state.economy.holdings[&outpost].slots[&Slot::Core];
    // 2 h to build, and D-224's start at 10% of its hull, less at most the one economy
    // second its slot (by `SimId`, doc 11 §4.1) may have had since the deployment.
    let building = state.module(core).unwrap().building_ms.unwrap();
    assert!((7_199_000..=7_200_000).contains(&building), "{building}");
    let hp = state.module(core).unwrap().hp;
    assert!((30_000..30_100).contains(&hp), "{hp}");
    // Outposts cannot build and do not raise the cap.
    assert_eq!(
        check(
            &state,
            1,
            EconomyCommand::Build {
                holding: outpost,
                item: BuildItem::CoreKit
            }
        ),
        Err(R::Outpost)
    );
    assert_eq!(state.command_cap(you), 120);
    // Skip most of the 2 h deployment.
    state
        .entities
        .get_mut::<Construction>(core)
        .unwrap()
        .remaining_ms = 2_000;
    let out = idle(&mut state, 30);
    assert!(events(&out).contains(&EconomyEvent::HoldingCompleted { holding: outpost }));
    assert!(state.holding(outpost).unwrap().core_complete);
    assert_eq!(state.command_cap(you), 130);
    assert_eq!(state.docking_capacity(outpost), 8);
    counts_once(&state, "holding");
}

#[test]
fn an_outposts_hull_grows_from_a_tenth_to_full_over_its_deployment() {
    // D-224: linear from 10% to 100% over the 2 h deployment; damage taken stays taken.
    let total = 7_200_000;
    assert_eq!(deploy_hull(300_000, 10, total, 0), 30_000);
    assert_eq!(deploy_hull(300_000, 10, total, total / 2), 165_000);
    assert_eq!(deploy_hull(300_000, 10, total, total), 300_000);
    assert_eq!(deploy_hull(300_000, 10, total, 2 * total), 300_000);
    assert_eq!(deploy_hull(300_000, 100, total, 0), 300_000);
    // The growth of every second sums to the full rise, with no drift.
    let rise: u32 = (0..7_200_u64)
        .map(|s| {
            deploy_hull(300_000, 10, total, (s + 1) * 1_000)
                - deploy_hull(300_000, 10, total, s * 1_000)
        })
        .sum();
    assert_eq!(rise, 270_000);

    let mut state = stage1(7);
    let (_, system) = home(&state, 1);
    let site = state
        .galaxy()
        .system(system)
        .unwrap()
        .sites
        .iter()
        .copied()
        .find(|&s| state.holding_at(s).is_none())
        .unwrap();
    let outpost = state.insert_holding(site, CommanderId(1), false).unwrap();
    let core = spawn_outpost_core(&mut state, outpost);
    let hp = |state: &UniverseState| state.module(core).unwrap().hp;
    assert_eq!(hp(&state), 30_000);
    idle(&mut state, 10 * TICKS_PER_SECOND);
    assert_eq!(hp(&state), deploy_hull(300_000, 10, total, 10_000));
    assert_eq!(hp(&state), 30_375);
    let now = state.tick();
    damage_module(&mut state, core, 1_000, now);
    idle(&mut state, 10 * TICKS_PER_SECOND);
    assert_eq!(hp(&state), deploy_hull(300_000, 10, total, 20_000) - 1_000);
}

#[test]
fn the_core_kit_price_grows_by_k_per_further_holding_rounded_up() {
    let base = Rules::embedded().economy.core_kit;
    // D-189 at the starting k = 0.25: 6,000 A + 500 C × (1 + 0.25 n).
    let k = Rules::embedded().economy.core_kit_growth_permille;
    assert_eq!(k, 250);
    for (n, alloy, crystal) in [
        (0, 6_000, 500),
        (1, 7_500, 625),
        (2, 9_000, 750),
        (4, 12_000, 1_000),
        (15, 28_500, 2_375),
    ] {
        assert_eq!(
            rules::core_kit_price(base, k, n),
            Stock::units(alloy, crystal),
            "n {n}"
        );
    }
    // Each resource is rounded up to whole units: 500 C × 1.333 = 666.5 → 667;
    // 6,000 A × 1.001 = 6,006 exactly, 500 C × 1.001 = 500.5 → 501.
    assert_eq!(
        rules::core_kit_price(base, 333, 1),
        Stock::units(7_998, 667)
    );
    assert_eq!(rules::core_kit_price(base, 1, 1), Stock::units(6_006, 501));
    let odd = rules::Cost {
        alloy: 1,
        crystal: 3,
        build_s: 1,
    };
    assert_eq!(rules::core_kit_price(odd, 250, 1), Stock::units(2, 4));
    assert_eq!(rules::core_kit_price(odd, 250, 4), Stock::units(2, 6));
    // Out-of-range numbers saturate instead of overflowing.
    let huge = rules::Cost {
        alloy: u64::MAX,
        crystal: u64::MAX,
        build_s: 1,
    };
    assert_eq!(
        rules::core_kit_price(huge, u32::MAX, u64::MAX),
        Stock {
            alloy: u64::MAX,
            crystal: u64::MAX
        }
    );
}

/// Runs one tick in which commander `n` sends every command in `commands`, in order.
fn send_all(state: &mut UniverseState, n: u32, commands: Vec<Command>) -> crate::TickOutputs {
    let commanders = commands
        .into_iter()
        .zip(1..)
        .map(|(command, seq)| crate::input::CommanderInput {
            commander: CommanderId(n),
            client_seq: seq,
            kind: crate::input::CommanderInputKind::Command(command),
        })
        .collect();
    crate::step(
        state,
        &crate::input::TickInputs {
            operator: vec![],
            commanders,
        },
    )
}

/// The prices of `holding`'s queue, in order.
fn queue_prices(state: &UniverseState, holding: HoldingId) -> Vec<Stock> {
    state
        .holding_economy(holding)
        .unwrap()
        .queue
        .iter()
        .map(|q| q.price)
        .collect()
}

#[test]
fn kits_queued_at_once_each_pay_more_and_keep_their_price() {
    let mut state = stage1(7);
    let (mother, home_system) = home(&state, 1);
    let you = CommanderId(1);
    let kit = eco(EconomyCommand::Build {
        holding: mother,
        item: BuildItem::CoreKit,
    });
    // Three kits in one tick: each counts the kits queued before it (D-189).
    let out = send_all(&mut state, 1, vec![kit.clone(), kit.clone(), kit]);
    assert!(out.commands.iter().all(|r| r.outcome.is_ok()));
    let ids: Vec<SimId> = out
        .economy_events
        .iter()
        .filter_map(|e| match e {
            EconomyEvent::ItemQueued { item, .. } => Some(*item),
            _ => None,
        })
        .collect();
    assert_eq!(ids.len(), 3);
    assert_eq!(
        queue_prices(&state, mother),
        [
            Stock::units(6_000, 500),
            Stock::units(7_500, 625),
            Stock::units(9_000, 750)
        ]
    );
    assert_eq!(state.expansion_count(you), 3);
    assert_eq!(state.core_kit_price(you), Stock::units(10_500, 875));

    // The first kit takes the whole start stock.
    idle(&mut state, 20);
    assert!(!command::waiting(&state, mother, ids[0]));
    assert_eq!(state.pool(you).crystal, 0);

    // n falls (the third kit is cancelled) and rises (an outpost appears); the second kit
    // keeps the price it was queued at.
    send(
        &mut state,
        1,
        eco(EconomyCommand::Cancel {
            holding: mother,
            item: ids[2],
        }),
    );
    let site = state
        .galaxy()
        .system(home_system)
        .unwrap()
        .sites
        .iter()
        .copied()
        .find(|&s| state.holding_at(s).is_none())
        .unwrap();
    state.insert_holding(site, you, false).unwrap();
    assert_eq!(state.expansion_count(you), 3);
    assert_eq!(
        queue_prices(&state, mother),
        [Stock::units(6_000, 500), Stock::units(7_500, 625)]
    );

    // It starts once the pool holds its price, and pays exactly that.
    state.economy.pools.insert(you, Stock::units(8_000, 1_000));
    let out = idle(&mut state, 900 * TICKS_PER_SECOND + 10);
    assert!(events(&out).contains(&EconomyEvent::ItemStarted {
        holding: mother,
        item: ids[1]
    }));
    assert_eq!(state.pool(you).crystal, 375_000);
    // Cancelled while building, it refunds half of that price (D-068).
    let out = send(
        &mut state,
        1,
        eco(EconomyCommand::Cancel {
            holding: mother,
            item: ids[1],
        }),
    );
    assert!(out.economy_events.contains(&EconomyEvent::ItemCancelled {
        holding: mother,
        item: ids[1],
        refund: Stock {
            alloy: 3_750_000,
            crystal: 312_500
        }
    }));
}

/// Puts `fleet` at rest at `at` in `system` (tests only).
fn teleport(
    state: &mut UniverseState,
    fleet: FleetId,
    system: crate::galaxy::SystemId,
    at: crate::PositionMetres,
) {
    let members: Vec<_> = state
        .entities
        .get::<Fleet>(fleet.sim_id())
        .unwrap()
        .members
        .keys()
        .copied()
        .collect();
    if let Some(f) = state.entities.get_mut::<Fleet>(fleet.sim_id()) {
        f.place = Place::InSystem(system);
        f.active = None;
        f.queue.clear();
    }
    for id in members.iter().map(|s| s.sim_id()).chain([fleet.sim_id()]) {
        if let Some(m) = state.entities.get_mut::<Motion>(id) {
            m.position = at;
            m.velocity = crate::geometry::Vec2::ZERO;
        }
    }
}

#[test]
fn convoys_empty_buffers_into_the_pool() {
    let mut state = stage1(7);
    let you = CommanderId(1);
    let (mother, home_system) = home(&state, 1);
    let remote = remote_ore_holding(&mut state);
    state.economy.holdings.get_mut(&remote).unwrap().buffer = Stock::units(900, 0);
    let kit = kit_mules(&state, 1);
    // Stand at the remote stop, in space: the first convoy second loads the buffer. (Given
    // while docked at the mother station, a stop of the route, the route would start
    // there, A1 plan §5.8.)
    let remote_system = state
        .galaxy()
        .site(state.holding(remote).unwrap().site)
        .unwrap()
        .system;
    let stop = state.launch_point(remote).unwrap();
    teleport(&mut state, kit, remote_system, stop);
    send(
        &mut state,
        1,
        eco(EconomyCommand::Convoy {
            fleet: kit,
            route: vec![remote, mother],
        }),
    );
    assert!(state.economy.convoys.contains_key(&kit));
    let out = idle(&mut state, 10);
    let loaded = events(&out).into_iter().find_map(|e| match e {
        EconomyEvent::ConvoyLoaded { amount, .. } => Some(amount),
        _ => None,
    });
    assert!(loaded.is_some_and(|a| a.alloy >= 900_000), "{loaded:?}");
    let carried: u64 = state.economy.cargo.values().map(|c| c.stock.alloy).sum();
    assert!(carried >= 900_000);
    // Now heading for the mother station: stand there instead, with room in the pool for
    // the cargo under D-282's 6,500 A start cap.
    state.economy.pools.insert(you, Stock::ZERO);
    let pool = state.pool(you).alloy;
    let depot = state.launch_point(mother).unwrap();
    teleport(&mut state, kit, home_system, depot);
    let out = idle(&mut state, 10);
    assert!(
        events(&out)
            .iter()
            .any(|e| matches!(e, EconomyEvent::ConvoyUnloaded { .. }))
    );
    assert!(state.pool(you).alloy >= pool + carried);
    assert!(state.economy.cargo.values().all(|c| c.stock.is_zero()));
    // Any other order ends the route.
    let out = send(
        &mut state,
        1,
        order(kit, FleetOrder::StationKeep { relative_to: None }),
    );
    assert!(
        out.economy_events
            .contains(&EconomyEvent::ConvoyEnded { fleet: kit })
    );
    assert!(state.economy.convoys.is_empty());
}

#[test]
fn convoys_travel_by_ordinary_orders() {
    let mut state = stage1(7);
    let (mother, _) = home(&state, 1);
    let remote = remote_ore_holding(&mut state);
    let kit = kit_mules(&state, 1);
    send(
        &mut state,
        1,
        eco(EconomyCommand::Convoy {
            fleet: kit,
            route: vec![remote, mother],
        }),
    );
    // Docked at the mother station, heading for the remote stop: launch, then jump.
    idle(&mut state, 30);
    let fleet = state.entities.get::<Fleet>(kit.sim_id()).unwrap();
    assert!(!matches!(fleet.place, Place::Docked(_)));
    assert!(fleet.active.is_some(), "the convoy gave the fleet an order");
}

#[test]
fn repair_is_free_after_the_grace_period_and_faster_with_the_ability() {
    let mut state = stage1(7);
    let (mother, _) = home(&state, 1);
    let refinery = state.economy.holdings[&mother].slots[&Slot::Inner(0)];
    let now = state.tick();
    assert_eq!(
        damage_module(&mut state, refinery, 10_000, now),
        Some(30_000)
    );
    // Free repair: 1% (400) a minute once 60 s have passed since the damage.
    idle(&mut state, 120 * TICKS_PER_SECOND);
    let hp = state.module(refinery).unwrap().hp;
    assert!((30_400..=30_800).contains(&hp), "{hp}");
    // REPAIR: 80 hull a second for 8 alloy a second.
    let pool = state.pool(CommanderId(1)).alloy;
    send(
        &mut state,
        1,
        eco(EconomyCommand::SetRepair {
            holding: mother,
            on: true,
        }),
    );
    idle(&mut state, 10 * TICKS_PER_SECOND);
    let after = state.module(refinery).unwrap().hp;
    assert!(after >= hp + 800, "{hp} → {after}");
    assert!(state.pool(CommanderId(1)).alloy < pool + 2_000);
    // It stops by itself when nothing is damaged.
    let out = idle(&mut state, 200 * TICKS_PER_SECOND);
    assert!(events(&out).contains(&EconomyEvent::RepairDone { holding: mother }));
    assert_eq!(state.module(refinery).unwrap().hp, 40_000);
    assert!(!state.holding_economy(mother).unwrap().repairing);
}

#[test]
fn module_positions_follow_the_slot_template() {
    let state = stage1(7);
    let (mother, _) = home(&state, 1);
    let site = state
        .galaxy()
        .site(state.holding(mother).unwrap().site)
        .unwrap();
    let dome = f64::from(state.rules.economy.slots.dome_radius_voxels)
        * state.rules.mother_station.voxel_m;
    for module in state.modules_at(mother) {
        let (system, at, radius) = state.module_position(module.id).unwrap();
        assert_eq!(system, site.system);
        assert!(radius > 0.0);
        let d = crate::geometry::distance_m(at, site.position);
        match module.slot {
            Slot::Core | Slot::Pylon(_) | Slot::Inner(_) | Slot::Dock => {
                assert!(d < dome, "{:?} at {d}", module.slot);
            }
            Slot::Solar | Slot::Deposit(_) => assert!(d > dome, "{:?} at {d}", module.slot),
        }
    }
}

#[test]
fn snapshot_and_restore_mid_production_and_mid_build_change_nothing() {
    let mut reference = stage1(11);
    let (mother, _) = home(&reference, 1);
    let mut restored = None;
    for t in 0..6_000_u64 {
        let inputs = match t {
            5 => crate::TickInputs {
                operator: vec![],
                commanders: vec![crate::CommanderInput {
                    commander: CommanderId(1),
                    client_seq: 1,
                    kind: crate::CommanderInputKind::Command(eco(EconomyCommand::Build {
                        holding: mother,
                        item: BuildItem::Module {
                            kind: ModuleKind::Turret,
                            slot: Slot::Inner(3),
                        },
                    })),
                }],
            },
            _ => crate::TickInputs::default(),
        };
        crate::step(&mut reference, &inputs);
        if let Some(copy) = restored.as_mut() {
            crate::step(copy, &inputs);
        }
        if t == 3_001 {
            let bytes = encode(&reference).unwrap();
            let copy = decode(&bytes).unwrap();
            assert_eq!(encode(&copy).unwrap(), bytes, "re-encoding is identical");
            restored = Some(copy);
        }
    }
    let restored = restored.unwrap();
    assert_eq!(
        state_hash(&restored).unwrap(),
        state_hash(&reference).unwrap()
    );
    assert!(reference.economy_problems().is_empty());
}

/// Collects the rejection reasons the scenarios below produce.
#[derive(Default)]
struct Seen(BTreeSet<R>);

impl Seen {
    fn expect(&mut self, state: &UniverseState, command: EconomyCommand, reason: R) {
        let shown = format!("{command:?}");
        assert_eq!(check(state, 1, command), Err(reason), "{shown}");
        self.0.insert(reason);
    }
}

fn build(holding: HoldingId, item: BuildItem) -> EconomyCommand {
    EconomyCommand::Build { holding, item }
}

fn module(kind: ModuleKind, slot: Slot) -> BuildItem {
    BuildItem::Module { kind, slot }
}

#[test]
fn every_economy_rejection_reason_is_produced() {
    let mut seen = Seen::default();
    module_and_queue_reasons(&mut seen);
    fleet_and_claim_reasons(&mut seen);
    let economy = economy_reasons();
    seen.0.retain(|r| economy.contains(r));
    assert_eq!(seen.0, economy);
}

/// Building and the queue.
fn module_and_queue_reasons(seen: &mut Seen) {
    let mut state = stage1(7);
    let (mother, _) = home(&state, 1);
    let (theirs, _) = home(&state, 2);

    seen.expect(
        &state,
        build(mother, module(ModuleKind::Core, Slot::Core)),
        R::NotBuildable,
    );
    seen.expect(
        &state,
        build(
            mother,
            BuildItem::Ship {
                class: ShipClass::HeavyCruiser,
            },
        ),
        R::NotBuildable,
    );
    seen.expect(
        &state,
        build(mother, module(ModuleKind::Turret, Slot::Inner(9))),
        R::NoSuchSlot,
    );
    seen.expect(
        &state,
        build(mother, module(ModuleKind::Turret, Slot::Deposit(1))),
        R::NoSuchSlot,
    );
    seen.expect(
        &state,
        build(mother, module(ModuleKind::Turret, Slot::Solar)),
        R::WrongSlot,
    );
    seen.expect(
        &state,
        build(mother, module(ModuleKind::Extractor, Slot::Deposit(0))),
        R::WrongSlot,
    );
    seen.expect(
        &state,
        build(mother, module(ModuleKind::Refinery, Slot::Inner(0))),
        R::SlotTaken,
    );
    seen.expect(
        &state,
        EconomyCommand::Cancel {
            holding: mother,
            item: SimId::from_raw(9_999_999),
        },
        R::UnknownItem,
    );
    seen.expect(&state, build(theirs, BuildItem::CoreKit), R::NotOwner);

    // A full queue.
    let max = state.rules.economy.queue.max_items;
    for _ in 0..max {
        send(
            &mut state,
            1,
            eco(build(
                mother,
                BuildItem::Ship {
                    class: ShipClass::Vx,
                },
            )),
        );
    }
    seen.expect(&state, build(mother, BuildItem::CoreKit), R::QueueFull);
    // A queued module reserves its slot.
    let mut fresh = stage1(7);
    send(
        &mut fresh,
        1,
        eco(build(mother, module(ModuleKind::Turret, Slot::Inner(2)))),
    );
    seen.expect(
        &fresh,
        build(mother, module(ModuleKind::Storage, Slot::Inner(2))),
        R::SlotTaken,
    );
}

/// Fleets, core kits and claiming; then convoys on the same Universe.
fn fleet_and_claim_reasons(seen: &mut Seen) {
    let state = stage1(7);
    let (mother, home_system) = home(&state, 1);
    let (theirs, _) = home(&state, 2);
    // The start kit's MULEs and VX are fleets of their own (D-286).
    let kit = kit_mules(&state, 1);
    // Fleets: no MULE, no kit, a full hold, in space instead of docked.
    let mut s = stage1(7);
    let vx_fleet = kit_fleet(&s, 1);
    seen.expect(
        &s,
        EconomyCommand::LoadCoreKit { fleet: vx_fleet },
        R::NoMule,
    );
    seen.expect(
        &s,
        EconomyCommand::Convoy {
            fleet: vx_fleet,
            route: vec![mother],
        },
        R::NoMule,
    );
    seen.expect(&s, EconomyCommand::LoadCoreKit { fleet: kit }, R::NoKit);
    seen.expect(
        &s,
        EconomyCommand::DeployCoreKit {
            fleet: kit,
            site: s.holding(mother).unwrap().site,
        },
        R::NoKit,
    );
    for ship in s.fleet(kit).unwrap().ships.clone() {
        s.economy.cargo.insert(
            ship,
            Cargo {
                stock: Stock::units(1, 0),
                core_kit: false,
            },
        );
    }
    s.economy.holdings.get_mut(&mother).unwrap().kits = 1;
    seen.expect(&s, EconomyCommand::LoadCoreKit { fleet: kit }, R::CargoFull);
    claim_reasons(seen, &mut s, kit, theirs, home_system);
    convoy_reasons(seen, &mut s, mother, kit);
}

/// Deploying a core kit.
fn claim_reasons(
    seen: &mut Seen,
    s: &mut UniverseState,
    kit: FleetId,
    theirs: HoldingId,
    home_system: crate::galaxy::SystemId,
) {
    // Deploying: unknown site, a raider haven, a taken site, too far away.
    let carrier = s.fleet(kit).unwrap().ships[0];
    s.economy.cargo.insert(
        carrier,
        Cargo {
            stock: Stock::ZERO,
            core_kit: true,
        },
    );
    seen.expect(
        s,
        EconomyCommand::DeployCoreKit {
            fleet: kit,
            site: SiteId(SimId::from_raw(9_999_999)),
        },
        R::UnknownSite,
    );
    let taken = s.holding(theirs).unwrap().site;
    seen.expect(
        s,
        EconomyCommand::DeployCoreKit {
            fleet: kit,
            site: taken,
        },
        R::SiteTaken,
    );
    let free = s
        .galaxy()
        .system(home_system)
        .unwrap()
        .sites
        .iter()
        .copied()
        .find(|&site| s.holding_at(site).is_none())
        .unwrap();
    seen.expect(
        s,
        EconomyCommand::DeployCoreKit {
            fleet: kit,
            site: free,
        },
        R::TooFar,
    );
    // Raider havens have no sites (D-099 item 6), so a claimable flag decides: mark the
    // home system unclaimable for the check.
    let home_kind = s.galaxy.systems[&home_system].kind;
    s.galaxy.systems.get_mut(&home_system).unwrap().kind = crate::galaxy::SystemKind::RaiderHaven;
    seen.expect(
        s,
        EconomyCommand::DeployCoreKit {
            fleet: kit,
            site: free,
        },
        R::NotClaimable,
    );
    s.galaxy.systems.get_mut(&home_system).unwrap().kind = home_kind;
}

/// Convoy routes.
fn convoy_reasons(seen: &mut Seen, s: &mut UniverseState, mother: HoldingId, kit: FleetId) {
    // Convoy routes: too long, a stop twice, not ending at a depot, an outpost stop.
    let remote = remote_ore_holding(s);
    let long = vec![remote; s.rules.economy.logistics.max_route_stops + 1];
    seen.expect(
        s,
        EconomyCommand::Convoy {
            fleet: kit,
            route: long,
        },
        R::RouteTooLong,
    );
    seen.expect(
        s,
        EconomyCommand::Convoy {
            fleet: kit,
            route: vec![remote, remote, mother],
        },
        R::DuplicateStop,
    );
    seen.expect(
        s,
        EconomyCommand::Convoy {
            fleet: kit,
            route: vec![mother, remote],
        },
        R::NotDepot,
    );
    s.holdings.get_mut(&remote).unwrap().core_complete = false;
    seen.expect(
        s,
        EconomyCommand::Convoy {
            fleet: kit,
            route: vec![remote, mother],
        },
        R::Outpost,
    );
    seen.expect(s, build(remote, BuildItem::CoreKit), R::Outpost);
}

#[test]
fn destroyed_carriers_lose_their_cargo_and_end_their_routes() {
    let mut state = stage1(7);
    let (mother, _) = home(&state, 1);
    let remote = remote_ore_holding(&mut state);
    let kit = kit_mules(&state, 1);
    send(
        &mut state,
        1,
        eco(EconomyCommand::Convoy {
            fleet: kit,
            route: vec![remote, mother],
        }),
    );
    send(
        &mut state,
        1,
        eco(EconomyCommand::SetRally {
            holding: mother,
            fleet: Some(kit),
        }),
    );
    let ships = state.fleet(kit).unwrap().ships.clone();
    let mule = ships
        .iter()
        .copied()
        .find(|s| state.ship(*s).is_some_and(|v| v.class == ShipClass::Mule))
        .unwrap();
    state.economy.cargo.insert(
        mule,
        Cargo {
            stock: Stock::units(500, 20),
            core_kit: true,
        },
    );
    // Remove the whole fleet the way combat does when it destroys every ship (S5a).
    for ship in &ships {
        state.entities.despawn(ship.sim_id());
    }
    state.entities.despawn(kit.sim_id());
    let out = idle(&mut state, 1);
    let ev = events(&out);
    assert!(ev.contains(&EconomyEvent::CargoLost {
        ship: mule,
        amount: Stock::units(500, 20),
        core_kit: true
    }));
    assert!(ev.contains(&EconomyEvent::ConvoyEnded { fleet: kit }));
    assert!(state.economy.cargo.is_empty());
    assert!(state.economy.convoys.is_empty());
    assert_eq!(state.economy.holdings[&mother].rally, None);
    assert!(
        state.economy_problems().is_empty(),
        "{:?}",
        state.economy_problems()
    );
}

// ------------------------------------------------------------ D-217: diminishing yield

/// Places a completed holding of commander 1 with only its core on a free claimable site
/// outside its home system: a holding that is not producing.
fn bare_remote_holding(state: &mut UniverseState) -> HoldingId {
    let (_, home_system) = home(state, 1);
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
    let holding = state.insert_holding(site, CommanderId(1), true).unwrap();
    spawn_module(state, holding, Slot::Core, ModuleKind::Core, None);
    holding
}

/// A remote ore holding's nominal alloy per hour, milli-units.
fn nominal_alloy_per_h(state: &UniverseState, holding: HoldingId) -> u64 {
    let site = state.holding(holding).unwrap().site;
    let richness = state.galaxy().site(site).unwrap().deposits[0].richness;
    600 * richness_permille(richness)
}

#[test]
fn yield_factor_is_min_of_one_and_f_over_n() {
    use super::rules::{scaled, yield_factor, yield_permille};
    // F = 2 (D-217): up to two producing holdings yield in full.
    for n in 0..=2 {
        assert_eq!(yield_factor(2_000, n), (1, 1), "n {n}");
        assert_eq!(yield_permille(2_000, n), 1_000);
    }
    assert_eq!(yield_factor(2_000, 3), (2_000, 3_000));
    assert_eq!(yield_permille(2_000, 3), 666);
    assert_eq!(yield_permille(2_000, 4), 500);
    assert_eq!(yield_permille(2_000, 15), 133);
    // F need not be whole.
    assert_eq!(yield_factor(2_500, 2), (1, 1));
    assert_eq!(yield_permille(2_500, 3), 833);
    // Exact integer scaling, rounded down.
    assert_eq!(scaled(600_000, (2_000, 3_000)), 400_000);
    assert_eq!(scaled(1, (2_000, 3_000)), 0);
    assert_eq!(scaled(u64::MAX, (1, 1)), u64::MAX);
    assert_eq!(scaled(u64::MAX, (2_000, 4_000)), u64::MAX / 2);
}

/// Seed 7's stage-1 Universe with D-217's F set to `f_permille`, so these tests do not depend
/// on the embedded value.
fn stage1_with_f(f_permille: u32) -> UniverseState {
    let mut rules = Rules::embedded();
    rules.economy.diminishing_yield_permille = f_permille;
    crate::galaxy::generate_stage1_with_rules(stage1(7).season(), crate::testkit::COMMANDERS, rules)
        .unwrap()
}

/// Places a completed holding of commander 1 with an extractor on a crystal vein and no
/// harvester, on a free claimable site outside its home system: a holding that produces
/// crystal only. Returns it and its nominal crystal per hour, milli-units.
fn remote_crystal_holding(state: &mut UniverseState) -> (HoldingId, u64) {
    let (_, home_system) = home(state, 1);
    let (site, vein) = state
        .galaxy()
        .sites()
        .filter(|(id, s)| {
            s.system != home_system
                && state.holding_at(*id).is_none()
                && state
                    .galaxy()
                    .system(s.system)
                    .is_some_and(|sys| crate::galaxy::is_claimable(sys.kind))
        })
        .find_map(|(id, s)| {
            s.deposits
                .iter()
                .position(|d| d.kind == DepositKind::CrystalVein)
                .map(|i| (id, i))
        })
        .expect("a free crystal site elsewhere");
    let richness = state.galaxy().site(site).unwrap().deposits[vein].richness;
    let holding = state.insert_holding(site, CommanderId(1), true).unwrap();
    spawn_module(state, holding, Slot::Core, ModuleKind::Core, None);
    spawn_module(
        state,
        holding,
        Slot::Deposit(u8::try_from(vein).unwrap()),
        ModuleKind::Extractor,
        None,
    );
    spawn_module(state, holding, Slot::Solar, ModuleKind::Solar, None);
    let per_h =
        state.rules().economy.production.extractor_crystal_per_h * richness_permille(richness);
    (holding, per_h)
}

#[test]
fn diminishing_yield_scales_remote_producers_but_never_the_mother_station() {
    let mut state = stage1_with_f(2_000);
    let you = CommanderId(1);
    let (mother, _) = home(&state, 1);
    let mut remotes: Vec<HoldingId> = (0..2).map(|_| remote_ore_holding(&mut state)).collect();
    // A holding without producers is not counted (D-217's n).
    let bare = bare_remote_holding(&mut state);
    let economy = state.economy_of(you);
    assert_eq!(
        (
            economy.producing_holdings.alloy,
            economy.yield_permille.alloy
        ),
        (2, 1_000)
    );
    idle(&mut state, ONE_HOUR);
    for &remote in &remotes {
        let view = state.holding_economy(remote).unwrap();
        assert!(view.producing.alloy);
        assert!(!view.producing.crystal);
        assert_eq!(view.yield_permille.alloy, 1_000);
        assert_eq!(
            view.buffer.alloy,
            nominal_alloy_per_h(&state, remote),
            "two producing holdings are within F = 2: full yield"
        );
    }

    // A third producer: n = 3, and every remote producer yields exactly 2/3.
    remotes.push(remote_ore_holding(&mut state));
    for &remote in &remotes {
        state.economy.holdings.get_mut(&remote).unwrap().buffer = Stock::ZERO;
    }
    // Room in the pool for the hour's income (D-282's 6,500 A start cap).
    state.economy.pools.insert(you, Stock::ZERO);
    let pool = state.pool(you);
    idle(&mut state, ONE_HOUR);
    for &remote in &remotes {
        let view = state.holding_economy(remote).unwrap();
        let nominal = nominal_alloy_per_h(&state, remote);
        assert_eq!(view.buffer.alloy, nominal * 2 / 3, "{remote:?}");
        assert_eq!(view.yield_permille.alloy, 666);
        // The buffer holds 2 h of the scaled production (D-064).
        assert_eq!(view.buffer_cap.alloy, nominal * 2 / 3 * 2);
    }
    // The mother station is never scaled: 600 A an hour into the pool.
    assert_eq!(state.pool(you).alloy - pool.alloy, 600_000);
    let mother_view = state.holding_economy(mother).unwrap();
    assert_eq!(
        mother_view.yield_permille,
        PerResource {
            alloy: 1_000,
            crystal: 1_000
        }
    );
    assert!(mother_view.producing.alloy);
    let bare_view = state.holding_economy(bare).unwrap();
    assert_eq!(bare_view.producing, PerResource::default());
    assert_eq!(bare_view.buffer, Stock::ZERO);
    let economy = state.economy_of(you);
    assert_eq!(
        (economy.producing_holdings, economy.yield_permille),
        (
            PerResource {
                alloy: 3,
                crystal: 0
            },
            PerResource {
                alloy: 666,
                crystal: 1_000
            }
        )
    );
    // Other commanders are not affected.
    assert_eq!(state.economy_of(CommanderId(2)).yield_permille.alloy, 1_000);
}

#[test]
fn a_holding_that_does_not_produce_a_resource_never_lowers_its_output() {
    // D-281: each resource has its own count, so ore-only holdings never lower crystal.
    let mut state = stage1_with_f(2_000);
    let you = CommanderId(1);
    let (crystal, per_h) = remote_crystal_holding(&mut state);
    idle(&mut state, ONE_HOUR);
    assert_eq!(
        state.holding_economy(crystal).unwrap().buffer.crystal,
        per_h
    );

    // Four ore-only holdings: twice F for alloy, while crystal still has one producer.
    let ores: Vec<HoldingId> = (0..4).map(|_| remote_ore_holding(&mut state)).collect();
    let economy = state.economy_of(you);
    assert_eq!(
        economy.producing_holdings,
        PerResource {
            alloy: 4,
            crystal: 1
        }
    );
    assert_eq!(
        economy.yield_permille,
        PerResource {
            alloy: 500,
            crystal: 1_000
        }
    );
    state.economy.holdings.get_mut(&crystal).unwrap().buffer = Stock::ZERO;
    idle(&mut state, ONE_HOUR);
    let view = state.holding_economy(crystal).unwrap();
    assert_eq!(
        view.buffer.crystal, per_h,
        "ore-only holdings never lower crystal"
    );
    assert_eq!(view.buffer.alloy, 0);
    assert_eq!(
        view.producing,
        PerResource {
            alloy: false,
            crystal: true
        }
    );
    assert_eq!(view.yield_permille.crystal, 1_000);
    assert_eq!(view.buffer_cap.crystal, per_h * 2);
    for &ore in &ores {
        let view = state.holding_economy(ore).unwrap();
        assert_eq!(view.buffer.alloy, nominal_alloy_per_h(&state, ore) / 2);
        assert_eq!(view.buffer.crystal, 0);
    }

    // And crystal producers beyond F share the crystal factor, leaving alloy alone.
    let (second, second_per_h) = remote_crystal_holding(&mut state);
    let (third, third_per_h) = remote_crystal_holding(&mut state);
    let economy = state.economy_of(you);
    assert_eq!(
        economy.producing_holdings,
        PerResource {
            alloy: 4,
            crystal: 3
        }
    );
    assert_eq!(economy.yield_permille.crystal, 666);
    for &h in [crystal, second, third].iter().chain(&ores) {
        state.economy.holdings.get_mut(&h).unwrap().buffer = Stock::ZERO;
    }
    idle(&mut state, ONE_HOUR);
    for (h, nominal) in [
        (crystal, per_h),
        (second, second_per_h),
        (third, third_per_h),
    ] {
        assert_eq!(
            state.holding_economy(h).unwrap().buffer.crystal,
            nominal * 2 / 3
        );
    }
    for &ore in &ores {
        assert_eq!(
            state.holding_economy(ore).unwrap().buffer.alloy,
            nominal_alloy_per_h(&state, ore) / 2,
            "crystal producers never lower alloy"
        );
    }
}

#[test]
fn diminishing_yield_follows_the_rules_value() {
    let mut rules = Rules::embedded();
    rules.economy.diminishing_yield_permille = 1_000;
    let mut state = crate::galaxy::generate_stage1_with_rules(
        stage1(7).season(),
        crate::testkit::COMMANDERS,
        rules,
    )
    .unwrap();
    let remotes: Vec<HoldingId> = (0..2).map(|_| remote_ore_holding(&mut state)).collect();
    assert_eq!(state.economy_of(CommanderId(1)).yield_permille.alloy, 500);
    idle(&mut state, ONE_HOUR);
    for &remote in &remotes {
        assert_eq!(
            state.holding_economy(remote).unwrap().buffer.alloy,
            nominal_alloy_per_h(&state, remote) / 2
        );
    }
}

/// D-285: a gate is contested only by hostiles the convoy's owner detects. An armed fleet at
/// a gate beyond every sensor of the convoy's owner does not contest it, so a convoy sets out
/// into the ambush; once the owner detects it, the same gate is contested.
#[test]
fn only_a_detected_armed_hostile_contests_a_gate() {
    use crate::geometry::Vec2;
    use crate::testkit::{site_position, spawn_fleet};
    let mut state = stage1(7);
    let (mother, home_system) = home(&state, 1);
    let mother_at = site_position(&state, home_system, 0);
    let _ = mother;
    // The home system's gate farthest from the mother station: beyond its core's 40 km
    // reach for a light signature (24 km).
    let distance = |g: &crate::galaxy::Gate| (Vec2::from(g.position) - mother_at).length();
    let (gate, gate_at) = state
        .galaxy()
        .gates()
        .filter(|(_, g)| g.system == home_system)
        .max_by(|a, b| distance(a.1).total_cmp(&distance(b.1)))
        .map(|(id, g)| (id, Vec2::from(g.position)))
        .expect("the home system has a gate");
    assert!(
        (gate_at - mother_at).length() > 30_000.0,
        "far enough to hide"
    );
    let ambush = spawn_fleet(
        &mut state,
        commander(2),
        home_system,
        gate_at + Vec2::new(1_000.0, 0.0),
        &[ShipClass::Vx; 2],
    );
    assert!(
        !state.view(commander(1)).is_sensed(ambush.sim_id()),
        "commander 1 does not detect the ambush"
    );
    assert!(!logistics::contested(&state, gate, commander(1)));
    // Commander 2 is not its own hostile, and commander 3 sees nothing there either.
    assert!(!logistics::contested(&state, gate, commander(3)));
    // A commander-1 picket next to it detects it: now the gate is contested.
    spawn_fleet(
        &mut state,
        commander(1),
        home_system,
        gate_at + Vec2::new(-1_000.0, 0.0),
        &[ShipClass::Vx],
    );
    assert!(state.view(commander(1)).is_sensed(ambush.sim_id()));
    assert!(logistics::contested(&state, gate, commander(1)));
}
