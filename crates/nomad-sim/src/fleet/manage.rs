//! Fleet management: create (and split) and merge (doc 15 §3.4, doc 17 §6 fleet panel;
//! S3 plan §3).
//!
//! These functions carry out commands the validator has accepted
//! ([`crate::command::validate_command`]): the ships exist, belong to the issuer and are
//! together, in one system or docked at one holding, and none is in lane transit.

use std::collections::VecDeque;

use nomad_types::Owner;

use crate::fleet::{DEFAULT_RULES_OF_ENGAGEMENT, Fleet, FleetId, Motion, Ship, ShipId, reform};
use crate::geometry::Vec2;
use crate::state::UniverseState;

/// Forms a new fleet of `ships`, taking each out of its fleet. Fleets left empty cease to
/// exist. The new fleet has no orders and the default rules of engagement; its anchor starts
/// at the ships' centroid with their mean velocity, so it does not jerk. Returns `None`, and
/// changes nothing, if a ship or its fleet does not exist.
pub(crate) fn create_fleet(
    state: &mut UniverseState,
    owner: Owner,
    ships: &[ShipId],
) -> Option<FleetId> {
    let mut ships: Vec<ShipId> = ships.to_vec();
    ships.sort_unstable();
    ships.dedup();

    let mut sources = Vec::with_capacity(ships.len());
    for &ship in &ships {
        let motion = state.entities.get::<Motion>(ship.sim_id())?;
        let fleet = state.entities.get::<Ship>(ship.sim_id())?.fleet;
        let (place, engaged) = state
            .entities
            .with::<Fleet, _>(fleet.sim_id(), |f| (f.place, f.engaged))?;
        sources.push((motion, place, engaged));
    }
    let (first, place, _) = *sources.first()?;
    let n = sources.len() as f64;
    let centroid = sources
        .iter()
        .fold(Vec2::ZERO, |sum, (m, _, _)| sum + Vec2::from(m.position));
    let velocity = sources
        .iter()
        .fold(Vec2::ZERO, |sum, (m, _, _)| sum + m.velocity);
    let anchor = Motion {
        position: (centroid * (1.0 / n)).into(),
        velocity: velocity * (1.0 / n),
        heading_rad: first.heading_rad,
    };

    for &ship in &ships {
        remove_from_fleet(state, ship);
    }
    let id = FleetId(state.ids.allocate());
    let mut fleet = Fleet {
        owner,
        place,
        members: ships.iter().map(|&s| (s, Vec2::ZERO)).collect(),
        rules_of_engagement: DEFAULT_RULES_OF_ENGAGEMENT,
        engaged: sources.iter().any(|&(_, _, engaged)| engaged),
        active: None,
        queue: VecDeque::new(),
    };
    reform(&mut fleet, state.rules.movement.formation_spacing_m);
    for &ship in &ships {
        if let Some(s) = state.entities.get_mut::<Ship>(ship.sim_id()) {
            s.fleet = id;
        }
    }
    state.entities.spawn(id.sim_id(), (fleet, anchor));
    Some(id)
}

/// Moves every ship of `fleet` into `into`, which keeps its orders and rules of engagement.
/// `fleet` ceases to exist.
pub(crate) fn merge_fleets(state: &mut UniverseState, fleet: FleetId, into: FleetId) {
    let Some(absorbed) = state.entities.get::<Fleet>(fleet.sim_id()) else {
        return;
    };
    for &ship in absorbed.members.keys() {
        if let Some(s) = state.entities.get_mut::<Ship>(ship.sim_id()) {
            s.fleet = into;
        }
    }
    let spacing = state.rules.movement.formation_spacing_m;
    if let Some(target) = state.entities.get_mut::<Fleet>(into.sim_id()) {
        target.engaged |= absorbed.engaged;
        target
            .members
            .extend(absorbed.members.keys().map(|&s| (s, Vec2::ZERO)));
        reform(target, spacing);
    }
    state.entities.despawn(fleet.sim_id());
}

/// Takes `ship` out of its fleet; the fleet ceases to exist if that was its last ship.
fn remove_from_fleet(state: &mut UniverseState, ship: ShipId) {
    let Some(old) = state.entities.get::<Ship>(ship.sim_id()).map(|s| s.fleet) else {
        return;
    };
    let spacing = state.rules.movement.formation_spacing_m;
    let empty = state
        .entities
        .get_mut::<Fleet>(old.sim_id())
        .is_some_and(|f| {
            f.members.remove(&ship);
            reform(f, spacing);
            f.members.is_empty()
        });
    if empty {
        state.entities.despawn(old.sim_id());
    }
}
