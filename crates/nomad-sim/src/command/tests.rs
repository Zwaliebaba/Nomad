//! Every rejection reason of the validator, and orders validated where the queue ends
//! (S3 plan §4.5, §7 "Validator tests: every rejection reason is covered").

use std::collections::BTreeSet;

use nomad_types::{Command, FleetOrder, HoldingOrder, Location, SimId};

use super::{CommandRejection as R, validate_command};
use crate::fleet::{Fleet, FleetId, Place, ShipClass, ShipId};
use crate::galaxy::{LaneId, SystemId};
use crate::geometry::Vec2;
use crate::holding::HoldingId;
use crate::state::UniverseState;
use crate::testkit::{
    at, commander, home, kit_fleet, order, send, site_position, spawn_fleet, spawn_unseen, stage1,
    then,
};
use crate::time::Tick;

/// The economy's reasons (S4), produced by `economy::tests`; the test below covers the rest.
pub(crate) fn economy_reasons() -> BTreeSet<R> {
    [
        R::Outpost,
        R::NotBuildable,
        R::NoSuchSlot,
        R::WrongSlot,
        R::SlotTaken,
        R::QueueFull,
        R::UnknownItem,
        R::NoMule,
        R::CargoFull,
        R::NoKit,
        R::UnknownSite,
        R::NotClaimable,
        R::SiteTaken,
        R::TooFar,
        R::RouteTooLong,
        R::DuplicateStop,
        R::NotDepot,
    ]
    .into_iter()
    .collect()
}

/// The station orders' reasons (S5b), produced by `defence::tests`.
pub(crate) fn defence_reasons() -> BTreeSet<R> {
    [
        R::NoSuchSector,
        R::AbilityCooldown,
        R::SectorOpen,
        R::NoPower,
    ]
    .into_iter()
    .collect()
}

/// The commander-settings reasons (S6a), produced by `commander::tests`.
pub(crate) fn commander_reasons() -> BTreeSet<R> {
    [R::UnknownCommander, R::InvalidWindow, R::WindowLocked]
        .into_iter()
        .collect()
}

/// Every reason, by an exhaustive match: a new reason does not compile until it is listed,
/// and the tests fail until a scenario produces it.
pub(crate) fn all_reasons() -> BTreeSet<R> {
    let every = [
        R::UnknownFleet,
        R::UnknownShip,
        R::UnknownHolding,
        R::UnknownTarget,
        R::UnknownSystem,
        R::NotOwner,
        R::HoldingNotOwned,
        R::InvalidPosition,
        R::WrongSystem,
        R::Unreachable,
        R::AlreadyThere,
        R::Docked,
        R::NotDocked,
        R::InTransit,
        R::NoCapacity,
        R::NoDryDock,
        R::NoHolding,
        R::NoWeapons,
        R::FriendlyTarget,
        R::SelfTarget,
        R::EmptyRoute,
        R::EmptyFleet,
        R::DuplicateShip,
        R::NotTogether,
        R::NothingToLaunch,
        R::Outpost,
        R::NotBuildable,
        R::NoSuchSlot,
        R::WrongSlot,
        R::SlotTaken,
        R::QueueFull,
        R::UnknownItem,
        R::NoMule,
        R::CargoFull,
        R::NoKit,
        R::UnknownSite,
        R::NotClaimable,
        R::SiteTaken,
        R::TooFar,
        R::RouteTooLong,
        R::DuplicateStop,
        R::NotDepot,
        R::NoSuchSector,
        R::AbilityCooldown,
        R::SectorOpen,
        R::NoPower,
        // S6a.
        R::UnknownCommander,
        R::InvalidWindow,
        R::WindowLocked,
        // S7.
        R::NotVisible,
    ];
    assert!(every.into_iter().all(listed));
    every.into_iter().collect()
}

/// The exhaustive match behind [`all_reasons`]: a new reason does not compile until it is
/// listed here and there.
const fn listed(reason: R) -> bool {
    match reason {
        R::UnknownFleet
        | R::UnknownShip
        | R::UnknownHolding
        | R::UnknownTarget
        | R::UnknownSystem
        | R::NotOwner
        | R::HoldingNotOwned
        | R::InvalidPosition
        | R::WrongSystem
        | R::Unreachable
        | R::AlreadyThere
        | R::Docked
        | R::NotDocked
        | R::InTransit
        | R::NoCapacity
        | R::NoDryDock
        | R::NoHolding
        | R::NoWeapons
        | R::FriendlyTarget
        | R::SelfTarget
        | R::EmptyRoute
        | R::EmptyFleet
        | R::DuplicateShip
        | R::NotTogether
        | R::NothingToLaunch
        | R::Outpost
        | R::NotBuildable
        | R::NoSuchSlot
        | R::WrongSlot
        | R::SlotTaken
        | R::QueueFull
        | R::UnknownItem
        | R::NoMule
        | R::CargoFull
        | R::NoKit
        | R::UnknownSite
        | R::NotClaimable
        | R::SiteTaken
        | R::TooFar
        | R::RouteTooLong
        | R::DuplicateStop
        | R::NotDepot
        | R::NoSuchSector
        | R::AbilityCooldown
        | R::SectorOpen
        | R::NoPower
        | R::UnknownCommander
        | R::InvalidWindow
        | R::WindowLocked
        | R::NotVisible => true,
    }
}

fn check(state: &UniverseState, n: u32, command: &Command) -> Result<(), R> {
    validate_command(state, commander(n), command)
}

fn nowhere() -> SimId {
    SimId::from_raw(9_999_999)
}

/// A Universe with commander 1's start kit docked, a fleet of commander 1 in space in its
/// home system, an unarmed one, a hostile fleet beside them and a WARDEN.
struct Scene {
    state: UniverseState,
    home: SystemId,
    mother: HoldingId,
    kit: FleetId,
    fleet: FleetId,
    mules: FleetId,
    hostile: FleetId,
    warden: FleetId,
    spot: Vec2,
}

fn scene() -> Scene {
    let mut state = stage1(5);
    let (mother, home_system) = home(&state, 1);
    let kit = kit_fleet(&state, 1);
    let spot = site_position(&state, home_system, 1) + Vec2::new(3_000.0, 0.0);
    let fleet = spawn_fleet(
        &mut state,
        commander(1),
        home_system,
        spot,
        &[ShipClass::Vx],
    );
    let mules = spawn_fleet(
        &mut state,
        commander(1),
        home_system,
        spot + Vec2::new(0.0, 1_000.0),
        &[ShipClass::Mule],
    );
    let hostile = spawn_fleet(
        &mut state,
        commander(2),
        home_system,
        spot + Vec2::new(2_000.0, 0.0),
        &[ShipClass::Vx],
    );
    let warden = spawn_fleet(
        &mut state,
        commander(1),
        home_system,
        spot + Vec2::new(0.0, -1_000.0),
        &[ShipClass::Warden],
    );
    Scene {
        state,
        home: home_system,
        mother,
        kit,
        fleet,
        mules,
        hostile,
        warden,
        spot,
    }
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "one scenario per rejection reason, in one table"
)]
fn every_rejection_reason_is_produced() {
    let mut s = scene();
    let other_system = *s.state.galaxy().neighbours(s.home).first().unwrap();
    let (rival_mother, _) = home(&s.state, 2);
    let core = Vec2::from(
        s.state
            .galaxy()
            .site(s.state.holding(s.mother).unwrap().site)
            .unwrap()
            .position,
    );
    let ship = s.state.fleet(s.fleet).unwrap().ships[0];
    let kit_ship = s.state.fleet(s.kit).unwrap().ships[0];
    let here = |p: Vec2| at(s.home, p);
    let move_to = |to: Location| FleetOrder::Move { to };

    // A fleet too big for any holding: 26 VX, with 5 of 30 places taken.
    let armada = spawn_fleet(
        &mut s.state,
        commander(1),
        s.home,
        s.spot + Vec2::new(-4_000.0, 0.0),
        &[ShipClass::Vx; 26],
    );
    // A fleet "in transit", set up directly.
    let travelling = spawn_fleet(
        &mut s.state,
        commander(1),
        s.home,
        s.spot + Vec2::new(-2_000.0, 0.0),
        &[ShipClass::Vx],
    );
    let to_gate = s.state.galaxy().system(s.home).unwrap().gates[0];
    if let Some(f) = s.state.entities.get_mut::<Fleet>(travelling.sim_id()) {
        f.place = Place::InTransit {
            lane: LaneId(nowhere()),
            to_gate,
            arrive: Tick::new(1_000),
        };
    }
    let travelling_ship = s.state.fleet(travelling).unwrap().ships[0];
    // Hostiles that arrived after the last sensors pass (S7): one here, beside the fleet, and
    // one in the neighbouring system, which must not be told apart from it (D-123).
    let unseen = spawn_unseen(
        &mut s.state,
        commander(2),
        s.home,
        s.spot + Vec2::new(500.0, 0.0),
        &[ShipClass::Vx],
    );
    let unseen_ship = s.state.fleet(unseen).unwrap().ships[0];
    let elsewhere = spawn_unseen(
        &mut s.state,
        commander(2),
        other_system,
        Vec2::ZERO,
        &[ShipClass::Vx],
    );

    let fleet_cmd = |fleet, o| order(fleet, o);
    let cases: Vec<(u32, Command, R)> = vec![
        (
            1,
            fleet_cmd(FleetId(nowhere()), FleetOrder::Launch),
            R::UnknownFleet,
        ),
        (
            1,
            Command::CreateFleet {
                ships: vec![ShipId(nowhere())],
            },
            R::UnknownShip,
        ),
        (
            1,
            fleet_cmd(
                s.fleet,
                FleetOrder::Dock {
                    at: HoldingId(nowhere()),
                },
            ),
            R::UnknownHolding,
        ),
        (
            1,
            Command::Holding {
                holding: HoldingId(nowhere()),
                order: HoldingOrder::LaunchReady,
            },
            R::UnknownHolding,
        ),
        (
            1,
            fleet_cmd(s.fleet, FleetOrder::Attack { target: nowhere() }),
            R::UnknownTarget,
        ),
        // Docked fleets cannot be targeted.
        (
            1,
            fleet_cmd(
                s.fleet,
                FleetOrder::Guard {
                    object: s.kit.sim_id(),
                },
            ),
            R::UnknownTarget,
        ),
        (
            1,
            fleet_cmd(
                s.fleet,
                FleetOrder::Jump {
                    system: SystemId(nowhere()),
                },
            ),
            R::UnknownSystem,
        ),
        (
            1,
            fleet_cmd(
                s.fleet,
                move_to(Location {
                    system: SystemId(nowhere()),
                    position: s.spot.into(),
                }),
            ),
            R::UnknownSystem,
        ),
        (
            2,
            fleet_cmd(s.fleet, FleetOrder::StationKeep { relative_to: None }),
            R::NotOwner,
        ),
        (2, Command::CreateFleet { ships: vec![ship] }, R::NotOwner),
        (
            1,
            Command::Holding {
                holding: rival_mother,
                order: HoldingOrder::LaunchReady,
            },
            R::NotOwner,
        ),
        (
            2,
            Command::MergeFleets {
                fleet: s.hostile,
                into: s.fleet,
            },
            R::NotOwner,
        ),
        (
            1,
            fleet_cmd(
                s.fleet,
                FleetOrder::Retreat {
                    to: Some(rival_mother),
                },
            ),
            R::HoldingNotOwned,
        ),
        (
            1,
            fleet_cmd(s.fleet, move_to(here(Vec2::new(f64::NAN, 0.0)))),
            R::InvalidPosition,
        ),
        (
            1,
            fleet_cmd(s.fleet, move_to(here(Vec2::new(1e9, 0.0)))),
            R::InvalidPosition,
        ),
        (
            1,
            fleet_cmd(s.fleet, move_to(at(other_system, Vec2::ZERO))),
            R::WrongSystem,
        ),
        (
            1,
            fleet_cmd(s.fleet, FleetOrder::Dock { at: rival_mother }),
            R::WrongSystem,
        ),
        // Inside the core spindle's footprint.
        (1, fleet_cmd(s.fleet, move_to(here(core))), R::Unreachable),
        (
            1,
            fleet_cmd(s.fleet, FleetOrder::Jump { system: s.home }),
            R::AlreadyThere,
        ),
        (1, fleet_cmd(s.kit, move_to(here(s.spot))), R::Docked),
        (
            1,
            fleet_cmd(s.kit, FleetOrder::Retreat { to: None }),
            R::Docked,
        ),
        (1, fleet_cmd(s.fleet, FleetOrder::Launch), R::NotDocked),
        (
            1,
            Command::CreateFleet {
                ships: vec![travelling_ship],
            },
            R::InTransit,
        ),
        (
            1,
            Command::MergeFleets {
                fleet: s.fleet,
                into: travelling,
            },
            R::InTransit,
        ),
        (
            1,
            fleet_cmd(armada, FleetOrder::Dock { at: s.mother }),
            R::NoCapacity,
        ),
        (
            1,
            fleet_cmd(armada, FleetOrder::Retreat { to: None }),
            R::NoHolding,
        ),
        (
            1,
            fleet_cmd(
                s.mules,
                FleetOrder::Attack {
                    target: s.hostile.sim_id(),
                },
            ),
            R::NoWeapons,
        ),
        (
            1,
            fleet_cmd(s.mules, FleetOrder::AttackMove { to: here(s.spot) }),
            R::NoWeapons,
        ),
        (
            1,
            fleet_cmd(
                s.fleet,
                FleetOrder::Attack {
                    target: s.mules.sim_id(),
                },
            ),
            R::FriendlyTarget,
        ),
        (
            1,
            fleet_cmd(
                s.fleet,
                FleetOrder::Guard {
                    object: s.fleet.sim_id(),
                },
            ),
            R::SelfTarget,
        ),
        (
            1,
            fleet_cmd(
                s.fleet,
                FleetOrder::StationKeep {
                    relative_to: Some(ship.sim_id()),
                },
            ),
            R::SelfTarget,
        ),
        (
            1,
            Command::MergeFleets {
                fleet: s.fleet,
                into: s.fleet,
            },
            R::SelfTarget,
        ),
        (
            1,
            fleet_cmd(s.fleet, FleetOrder::Patrol { route: vec![] }),
            R::EmptyRoute,
        ),
        (1, Command::CreateFleet { ships: vec![] }, R::EmptyFleet),
        (
            1,
            Command::CreateFleet {
                ships: vec![ship, ship],
            },
            R::DuplicateShip,
        ),
        (
            1,
            Command::CreateFleet {
                ships: vec![ship, kit_ship],
            },
            R::NotTogether,
        ),
        (
            1,
            Command::MergeFleets {
                fleet: s.kit,
                into: s.mules,
            },
            R::NotTogether,
        ),
        // S7: what the issuer does not detect cannot be named, wherever it is.
        (
            1,
            fleet_cmd(
                s.fleet,
                FleetOrder::Attack {
                    target: unseen.sim_id(),
                },
            ),
            R::NotVisible,
        ),
        (
            1,
            fleet_cmd(
                s.fleet,
                FleetOrder::Guard {
                    object: unseen_ship.sim_id(),
                },
            ),
            R::NotVisible,
        ),
        (
            1,
            fleet_cmd(
                s.fleet,
                FleetOrder::Attack {
                    target: elsewhere.sim_id(),
                },
            ),
            R::NotVisible,
        ),
    ];

    let mut seen = BTreeSet::new();
    for (n, command, expected) in &cases {
        assert_eq!(check(&s.state, *n, command), Err(*expected), "{command:?}");
        seen.insert(*expected);
    }

    // Frigates dock only where there is a completed dry dock (D-102).
    let dock_module = s.state.economy.holdings[&s.mother].slots[&nomad_types::Slot::Dock];
    s.state.entities.insert(
        dock_module,
        crate::economy::Construction { remaining_ms: 1 },
    );
    let dock = fleet_cmd(s.warden, FleetOrder::Dock { at: s.mother });
    assert_eq!(check(&s.state, 1, &dock), Err(R::NoDryDock));
    seen.insert(R::NoDryDock);
    s.state
        .entities
        .remove::<crate::economy::Construction>(dock_module);
    assert_eq!(check(&s.state, 1, &dock), Ok(()));

    // Nothing left to launch once the start kit is out.
    let launch = Command::Holding {
        holding: s.mother,
        order: HoldingOrder::LaunchReady,
    };
    assert_eq!(check(&s.state, 1, &launch), Ok(()));
    send(&mut s.state, 1, launch.clone());
    assert_eq!(check(&s.state, 1, &launch), Err(R::NothingToLaunch));
    seen.insert(R::NothingToLaunch);

    let fleet_reasons: BTreeSet<R> = all_reasons()
        .difference(
            &economy_reasons()
                .union(&defence_reasons())
                .copied()
                .collect(),
        )
        .copied()
        .collect::<BTreeSet<R>>()
        .difference(&commander_reasons())
        .copied()
        .collect();
    assert_eq!(seen, fleet_reasons);
}

#[test]
fn appended_orders_are_checked_where_the_queue_ends() {
    let mut s = scene();
    let other_system = *s.state.galaxy().neighbours(s.home).first().unwrap();
    let gate = s
        .state
        .galaxy()
        .gate(s.state.galaxy().system(other_system).unwrap().gates[0]);
    let there = at(other_system, gate.unwrap().position.into());

    // After a jump, a move in the destination system is valid and one at home is not.
    let jump = then(
        s.fleet,
        FleetOrder::Jump {
            system: other_system,
        },
    );
    assert_eq!(send(&mut s.state, 1, jump).commands[0].outcome, Ok(()));
    assert_eq!(
        check(&s.state, 1, &then(s.fleet, FleetOrder::Move { to: there })),
        Ok(())
    );
    assert_eq!(
        check(
            &s.state,
            1,
            &then(
                s.fleet,
                FleetOrder::Move {
                    to: at(s.home, s.spot)
                }
            )
        ),
        Err(R::WrongSystem)
    );
    // Replacing starts from where the fleet is: home.
    assert_eq!(
        check(&s.state, 1, &order(s.fleet, FleetOrder::Move { to: there })),
        Err(R::WrongSystem)
    );

    // After a dock, only a launch (or rules of engagement) may follow.
    let dock = then(s.mules, FleetOrder::Dock { at: s.mother });
    assert_eq!(send(&mut s.state, 1, dock).commands[0].outcome, Ok(()));
    assert_eq!(
        check(
            &s.state,
            1,
            &then(
                s.mules,
                FleetOrder::Move {
                    to: at(s.home, s.spot)
                }
            )
        ),
        Err(R::Docked)
    );
    assert_eq!(
        check(&s.state, 1, &then(s.mules, FleetOrder::Launch)),
        Ok(())
    );
    assert_eq!(
        check(
            &s.state,
            1,
            &then(
                s.mules,
                FleetOrder::SetRulesOfEngagement {
                    rules: nomad_types::RulesOfEngagement::HoldFire,
                },
            ),
        ),
        Ok(())
    );
    // Rules of engagement apply at once, whatever the queue.
    send(
        &mut s.state,
        1,
        then(
            s.mules,
            FleetOrder::SetRulesOfEngagement {
                rules: nomad_types::RulesOfEngagement::HoldFire,
            },
        ),
    );
    let mules = s.state.fleet(s.mules).unwrap();
    assert_eq!(
        mules.rules_of_engagement,
        nomad_types::RulesOfEngagement::HoldFire
    );
    assert!(mules.queued.is_empty());
}
