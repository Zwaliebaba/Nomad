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
    generate_stage1(SeasonConfig::new(seed, 0), COMMANDERS).expect("stage 1 generates")
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
        retreat: false,
    }
}

/// An appended order to `fleet`.
pub(crate) fn then(fleet: FleetId, order: FleetOrder) -> Command {
    Command::Fleet {
        fleet,
        order,
        queue: Queueing::Append,
        retreat: false,
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

/// Commander `n`'s start-kit garrison: the 3 VX (D-286).
pub(crate) fn kit_fleet(state: &UniverseState, n: u32) -> FleetId {
    kit_of(state, n, ShipClass::Vx)
}

/// Commander `n`'s start-kit reserve: the 2 MULEs (D-286).
pub(crate) fn kit_mules(state: &UniverseState, n: u32) -> FleetId {
    kit_of(state, n, ShipClass::Mule)
}

fn kit_of(state: &UniverseState, n: u32, class: ShipClass) -> FleetId {
    state
        .fleet_ids()
        .into_iter()
        .find(|&f| {
            state.fleet(f).is_some_and(|v| {
                v.owner == commander(n)
                    && v.ships
                        .iter()
                        .all(|&s| state.ship(s).is_some_and(|s| s.class == class))
            })
        })
        .expect("every commander has its start-kit fleets")
}

/// Keeps commander `n`'s Deputy out of a mechanics test, through ordinary doctrine (A1 plan
/// §5.1; a test fixture, not a game rule): every holding of theirs gets a doctrine that
/// neither switches its power profile nor uses an ability, and every fleet of theirs docked
/// at one of them becomes that holding's reserve, which the Deputy never launches (D-207).
/// Fleets in space with no known home are left alone by the Deputy anyway.
pub(crate) fn passive(state: &mut UniverseState, n: u32) {
    let me = CommanderId(n);
    let preset = *state.rules.ai.preset(state.doctrine_preset(me));
    let holdings: Vec<(HoldingId, nomad_types::PowerProfile)> = state
        .holdings()
        .filter(|(_, h)| h.owner == me)
        .map(|(id, _)| {
            let profile = state
                .holding_economy(id)
                .map(|e| e.profile)
                .unwrap_or_default();
            (id, profile)
        })
        .collect();
    let docked: Vec<(FleetId, HoldingId)> = state
        .fleet_ids()
        .into_iter()
        .filter_map(|f| {
            let view = state.fleet(f)?;
            match view.place {
                Place::Docked(at) if view.owner == commander(n) => Some((f, at)),
                _ => None,
            }
        })
        .collect();
    let doctrine = state.doctrine.commanders.entry(me).or_default();
    for (holding, profile) in holdings {
        doctrine.holdings.insert(
            holding,
            nomad_types::HoldingDoctrine {
                alert_profile: profile,
                surge_below_permille: 0,
                flak_light_hostiles: None,
                ..preset.holding
            },
        );
    }
    for (fleet, holding) in docked {
        doctrine.fleets.insert(
            fleet,
            nomad_types::FleetDoctrine {
                role: Some(nomad_types::FleetRole::Reserve { holding }),
                ..preset.fleet()
            },
        );
    }
}

/// [`passive`] for every stage-1 commander.
pub(crate) fn passive_all(state: &mut UniverseState) {
    for n in 1..=4 {
        passive(state, n);
    }
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
/// facing east, and runs a sensors pass so it is seen at once ([`spawn_unseen`] does not).
pub(crate) fn spawn_fleet(
    state: &mut UniverseState,
    owner: Owner,
    system: SystemId,
    centre: Vec2,
    classes: &[ShipClass],
) -> FleetId {
    let fleet = spawn_unseen(state, owner, system, centre, classes);
    sense(state);
    fleet
}

/// Runs a sensors pass now, as on a pass tick (S7): a fixture for state changed outside the
/// step.
pub(crate) fn sense(state: &mut UniverseState) {
    crate::sensors::refresh(state);
}

/// As [`spawn_fleet`], without the sensors pass: the fleet is detected from the next pass.
pub(crate) fn spawn_unseen(
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
