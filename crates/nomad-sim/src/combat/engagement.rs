//! Engagement (D-114; doc 15 §3.3; S5 plan §4.4) and the attacker memory for *return fire*
//! (D-145).
//!
//! A fleet becomes engaged when it takes fire or when a hostile ship is within the
//! cruise-drop range (20 km). Each trigger moves its deadline to 30 s after the trigger's
//! tick; at the deadline the engagement ends and the fleet may cruise again. "A hostile
//! within 20 km" is a hostile ship in space in the same system, within 20 km of the fleet's
//! anchor, that its commander's picture from the last sensors pass holds (D-114, S7): an
//! undetected fleet neither drops anyone out of cruise nor keeps them engaged.

use std::collections::{BTreeMap, BTreeSet};

use nomad_types::Owner;

use super::{Attackers, ShipIndex, hostile};
use crate::fleet::{Fleet, FleetId, Motion, Place, ShipId, Shroud};
use crate::galaxy::SystemId;
use crate::geometry::Vec2;
use crate::sensors::Seen;
use crate::state::UniverseState;
use crate::time::{Tick, Ticks};

/// Owners, identities and positions of the ships in space, by system and cell of the
/// trigger range.
type OwnerGrid = BTreeMap<(SystemId, (i64, i64)), Vec<(Owner, ShipId, Vec2)>>;

/// The engagement window (D-114).
pub(crate) fn window(state: &UniverseState) -> Ticks {
    Ticks::from_secs(state.rules.combat.engagement_s)
}

/// Phase 7, step 1: the 20 km trigger, then expired deadlines and stale attacker records.
pub(crate) fn refresh(state: &mut UniverseState, now: Tick, index: &ShipIndex, seen: &Seen) {
    let range = state.rules.movement.cruise_drop_hostile_range_m;
    let until = now.after(window(state));

    // Owners present per (system, cell of the trigger range), so each fleet looks at the
    // 3 × 3 cells around its anchor.
    let mut owners = OwnerGrid::new();
    for (&system, ships) in &index.systems {
        for entry in &ships.entries {
            owners
                .entry((system, super::cell(entry.position, range)))
                .or_default()
                .push((entry.owner, entry.id, entry.position));
        }
    }

    for id in state.entities.ids_with::<Fleet>() {
        let Some((place, owner, deadline)) = state
            .entities
            .with::<Fleet, _>(id, |f| (f.place, f.owner, f.engaged_until))
        else {
            continue;
        };
        let mut deadline = deadline.filter(|&d| d > now);
        if let Place::InSystem(system) = place
            && let Some(anchor) = state.entities.get::<Motion>(id)
        {
            let at = Vec2::from(anchor.position);
            let (cx, cy) = super::cell(at, range);
            let threatened = (cx - 1..=cx + 1)
                .flat_map(|x| (cy - 1..=cy + 1).map(move |y| (x, y)))
                .filter_map(|c| owners.get(&(system, c)))
                .flatten()
                .any(|&(other, ship, p)| {
                    hostile(owner, other)
                        && (p - at).length() <= range
                        && seen.ship(owner, system, ship)
                });
            if threatened {
                deadline = Some(deadline.map_or(until, |d| d.max(until)));
            }
        }
        if let Some(f) = state.entities.get_mut::<Fleet>(id) {
            f.engaged_until = deadline;
        }
    }

    let window = window(state);
    for id in state.entities.ids_with::<Attackers>() {
        let empty = state.entities.get_mut::<Attackers>(id).is_some_and(|a| {
            a.last_hit.retain(|_, &mut t| t.after(window) > now);
            a.last_hit.is_empty()
        });
        if empty {
            state.entities.remove::<Attackers>(id);
        }
    }
}

/// A ship of `fleet` was hit by `shooter` at tick `now`: the fleet is engaged for another
/// window ("taking fire", D-114), and `shooter` becomes a target for return fire (D-145).
pub(crate) fn taking_fire(state: &mut UniverseState, now: Tick, fleet: FleetId, shooter: ShipId) {
    if !engaged_by_fire(state, now, fleet) {
        return;
    }
    let id = fleet.sim_id();
    if let Some(attackers) = state.entities.get_mut::<Attackers>(id) {
        attackers.last_hit.insert(shooter, now);
    } else {
        state.entities.insert(
            id,
            Attackers {
                last_hit: BTreeMap::from([(shooter, now)]),
            },
        );
    }
}

/// A ship of `fleet` was hit at tick `now`: the fleet is engaged for another window ("taking
/// fire", D-114). Returns whether the fleet exists. A hit from a turret or a FLAK SCREEN
/// (S5b) does only this, since there is no ship to return fire at.
pub(crate) fn engaged_by_fire(state: &mut UniverseState, now: Tick, fleet: FleetId) -> bool {
    let until = now.after(window(state));
    let Some(f) = state.entities.get_mut::<Fleet>(fleet.sim_id()) else {
        return false;
    };
    f.engaged_until = Some(f.engaged_until.map_or(until, |d| d.max(until)));
    true
}

/// Ships that hit one of `fleet`'s ships within the engagement window (D-145).
pub(crate) fn recent_attackers(state: &UniverseState, fleet: FleetId) -> BTreeSet<ShipId> {
    state
        .entities
        .with::<Attackers, _>(fleet.sim_id(), |a| a.last_hit.keys().copied().collect())
        .unwrap_or_default()
}

/// Whether a ship hostile to `owner`, which `owner` detects, is in space in `system` within
/// `range` of `point`. For Attack-move, which pauses to fight what it meets (S5 plan §4.2).
/// It runs in phase 5, with the pictures in the state.
pub(crate) fn hostile_ship_within(
    state: &UniverseState,
    system: SystemId,
    owner: Owner,
    point: Vec2,
    range: f64,
) -> bool {
    let Some(picture) = crate::sensors::picture_of(state, owner, system) else {
        return false;
    };
    state.entities.ids_with::<Fleet>().into_iter().any(|id| {
        state
            .entities
            .with::<Fleet, _>(id, |f| {
                f.place == Place::InSystem(system)
                    && hostile(owner, f.owner)
                    && f.members.keys().any(|s| {
                        // A shrouded ship cannot be fought (D-055), so it does not stop one.
                        picture.has_ship(*s)
                            && !state.entities.has::<Shroud>(s.sim_id())
                            && state
                                .entities
                                .get::<Motion>(s.sim_id())
                                .is_some_and(|m| (Vec2::from(m.position) - point).length() <= range)
                    })
            })
            .unwrap_or(false)
    })
}
