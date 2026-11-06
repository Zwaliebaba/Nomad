//! Helpers for unit tests: a generated stage-1 Universe, commands, and fleets placed
//! directly in space (a test fixture, not a game rule).

use std::collections::{BTreeMap, VecDeque};

use nomad_types::{Command, FleetOrder, Location, Owner, Queueing};

use crate::fleet::{
    DEFAULT_RULES_OF_ENGAGEMENT, Fleet, FleetId, Motion, Place, Ship, ShipClass, ShipId,
    formation_offsets, slot_position,
};
use crate::galaxy::{STAGE1_COMMANDERS, SystemId, generate_stage1};
use crate::geometry::Vec2;
use crate::holding::HoldingId;
use crate::id::CommanderId;
use crate::input::{CommanderInput, CommanderInputKind, TickInputs};
use crate::state::{SeasonConfig, UniverseState};
use crate::step::{TickOutputs, step};

/// The four stage-1 commanders.
pub(crate) const COMMANDERS: [CommanderId; STAGE1_COMMANDERS] = [
    CommanderId(1),
    CommanderId(2),
    CommanderId(3),
    CommanderId(4),
];

/// A generated stage-1 Universe with the start kits.
pub(crate) fn stage1(seed: u64) -> UniverseState {
    generate_stage1(
        SeasonConfig {
            seed,
            season_start_unix_ms: 0,
        },
        COMMANDERS,
    )
    .expect("stage 1 generates")
}

/// Commander `n`'s owner identity.
pub(crate) const fn commander(n: u32) -> Owner {
    Owner::Commander(CommanderId(n))
}

/// Runs one tick in which commander `n` sends `command`.
pub(crate) fn send(state: &mut UniverseState, n: u32, command: Command) -> TickOutputs {
    step(
        state,
        &TickInputs {
            operator: vec![],
            commanders: vec![CommanderInput {
                commander: CommanderId(n),
                client_seq: state.tick().get(),
                kind: CommanderInputKind::Command(command),
            }],
        },
    )
}

/// Runs `ticks` ticks without inputs and returns what they produced.
pub(crate) fn idle(state: &mut UniverseState, ticks: u64) -> Vec<TickOutputs> {
    (0..ticks)
        .map(|_| step(state, &TickInputs::default()))
        .collect()
}

/// A replacing order to `fleet`.
pub(crate) fn order(fleet: FleetId, order: FleetOrder) -> Command {
    Command::Fleet {
        fleet,
        order,
        queue: Queueing::Replace,
    }
}

/// An appended order to `fleet`.
pub(crate) fn then(fleet: FleetId, order: FleetOrder) -> Command {
    Command::Fleet {
        fleet,
        order,
        queue: Queueing::Append,
    }
}

/// A point in `system`.
pub(crate) fn at(system: SystemId, point: Vec2) -> Location {
    Location {
        system,
        position: point.into(),
    }
}

/// Commander `n`'s mother station and its system.
pub(crate) fn home(state: &UniverseState, n: u32) -> (HoldingId, SystemId) {
    let (id, holding) = state
        .holdings()
        .find(|(_, h)| h.owner == CommanderId(n) && h.mother_station)
        .expect("every commander has a mother station");
    let system = state
        .galaxy()
        .site(holding.site)
        .expect("the site exists")
        .system;
    (id, system)
}

/// Commander `n`'s start-kit fleet.
pub(crate) fn kit_fleet(state: &UniverseState, n: u32) -> FleetId {
    state
        .fleet_ids()
        .into_iter()
        .find(|&f| state.fleet(f).is_some_and(|v| v.owner == commander(n)))
        .expect("every commander has a start-kit fleet")
}

/// The position of site `index` of `system`.
pub(crate) fn site_position(state: &UniverseState, system: SystemId, index: usize) -> Vec2 {
    let site = state.galaxy().system(system).expect("system exists").sites[index];
    state
        .galaxy()
        .site(site)
        .expect("site exists")
        .position
        .into()
}

/// Places a new fleet of `classes` for `owner` at rest in space at `centre`, in formation,
/// facing east.
pub(crate) fn spawn_fleet(
    state: &mut UniverseState,
    owner: Owner,
    system: SystemId,
    centre: Vec2,
    classes: &[ShipClass],
) -> FleetId {
    let id = FleetId(state.ids.allocate());
    let anchor = Motion {
        position: centre.into(),
        velocity: Vec2::ZERO,
        heading_rad: 0.0,
    };
    let offsets = formation_offsets(classes.len(), state.rules.movement.formation_spacing_m);
    let mut members = BTreeMap::new();
    for (&class, offset) in classes.iter().zip(offsets) {
        let ship = ShipId(state.ids.allocate());
        members.insert(ship, offset);
        state.entities.spawn(
            ship.sim_id(),
            (
                Ship {
                    class,
                    owner,
                    fleet: id,
                },
                Motion {
                    position: slot_position(&anchor, offset).into(),
                    ..anchor
                },
            ),
        );
    }
    state.entities.spawn(
        id.sim_id(),
        (
            Fleet {
                owner,
                place: Place::InSystem(system),
                members,
                rules_of_engagement: DEFAULT_RULES_OF_ENGAGEMENT,
                engaged_until: None,
                active: None,
                queue: VecDeque::new(),
            },
            anchor,
        ),
    );
    id
}

/// The positions of `fleet`'s ships.
pub(crate) fn ship_positions(state: &UniverseState, fleet: FleetId) -> Vec<Vec2> {
    state
        .fleet(fleet)
        .expect("fleet exists")
        .ships
        .iter()
        .map(|&s| state.ship(s).expect("ship exists").position.into())
        .collect()
}
