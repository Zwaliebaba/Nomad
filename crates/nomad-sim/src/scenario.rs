//! Scenario fixtures for the harness and integration tests: fleets placed directly in space.
//!
//! **Not a game rule.** In a season, ships come from the start kit and later from shipyards
//! (S4). Measurement runs such as the time-to-kill scenarios (D-058; S5 plan §7 task 7) need
//! fleets of a chosen composition at a chosen place, so this module puts them there without
//! the economy. Nothing in the simulation calls it.

use std::collections::{BTreeMap, VecDeque};

use nomad_types::Owner;

use crate::fleet::{
    DEFAULT_RULES_OF_ENGAGEMENT, Fleet, FleetId, Motion, Place, Ship, ShipClass, ShipId,
    formation_offsets, slot_position,
};
use crate::galaxy::SystemId;
use crate::geometry::Vec2;
use crate::state::UniverseState;

/// Places a new fleet of `classes` for `owner` at rest in space in `system`, its anchor at
/// `centre`, in the default formation, facing `heading_rad` (counter-clockwise from east).
/// The fleet has no orders and the default rules of engagement.
pub fn place_fleet(
    state: &mut UniverseState,
    owner: Owner,
    system: SystemId,
    centre: Vec2,
    heading_rad: f64,
    classes: &[ShipClass],
) -> FleetId {
    let id = FleetId(state.ids.allocate());
    let anchor = Motion {
        position: centre.into(),
        velocity: Vec2::ZERO,
        heading_rad,
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
