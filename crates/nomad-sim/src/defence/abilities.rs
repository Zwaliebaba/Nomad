//! The station abilities and STOP: validation, execution and FLAK SCREEN's bursts (doc 15
//! §5.3; D-128, D-147, D-148; S5 plan §4.8).
//!
//! - **SHLD SURGE** restores 20% of the chosen sector's maximum at once, up to the maximum.
//!   It needs its 200 MW free for 10 s (S4's ability power, D-128), and it is refused on an
//!   open sector (doc 15 §5.3). 90 s cooldown.
//! - **FLAK SCREEN** for 10 s fires one burst a second at each hostile Light or Medium ship
//!   within 2 km of the dome centre, at most 8 a second, the nearest first, each with the
//!   WARDEN's flak values (D-148). It needs 90 MW while it fires. 60 s cooldown.
//! - **STOP** sets or clears the holding's turret hold.
//!
//! Cooldowns run from the moment an ability starts. Every order needs a completed holding
//! (an outpost has no dome or turrets). Bursts leave the dome centre as flak shells, aimed
//! with linear lead, at the base spread (the screen builds no lock), and pass outward
//! through the holding's own dome.

use nomad_types::{HoldingOrder, Owner, StationAbility};

use super::dome;
use crate::combat::{Gun, ShipIndex, Shooter, Shot, hostile, launch};
use crate::command::CommandRejection;
use crate::economy::rules::KW_PER_MW;
use crate::fleet::ShipId;
use crate::geometry::Vec2;
use crate::holding::HoldingId;
use crate::rules::combat::SizeClass;
use crate::state::UniverseState;
use crate::time::{TICK_MILLIS, Tick, Ticks};

/// The tick an order sent now is carried out on: commands apply in phase 1 of the next tick.
fn order_tick(state: &UniverseState) -> Tick {
    state.tick.next()
}

/// Checks a station order (S5b) to `holding`, which the issuer owns (D-077).
pub(crate) fn validate(
    state: &UniverseState,
    holding: HoldingId,
    order: HoldingOrder,
) -> Result<(), CommandRejection> {
    use CommandRejection as R;
    if !state
        .holdings
        .get(&holding)
        .is_some_and(|h| h.core_complete)
    {
        return Err(R::Outpost);
    }
    let now = order_tick(state);
    let d = state.defence.holdings.get(&holding);
    let rules = &state.rules.defence;
    match order {
        HoldingOrder::LaunchReady | HoldingOrder::TurretsHold { .. } => Ok(()),
        HoldingOrder::Ability {
            ability: StationAbility::ShieldSurge { sector },
        } => {
            if usize::from(sector) >= 4 {
                return Err(R::NoSuchSector);
            }
            if d.is_some_and(|d| d.dome_up && d.sectors[usize::from(sector)].open) {
                return Err(R::SectorOpen);
            }
            if d.is_some_and(|d| now < d.surge_ready_at) {
                return Err(R::AbilityCooldown);
            }
            let draw = rules.shield_surge.draw_mw.saturating_mul(KW_PER_MW);
            if crate::economy::ability_power_available(state, holding, draw, now) {
                Ok(())
            } else {
                Err(R::NoPower)
            }
        }
        HoldingOrder::Ability {
            ability: StationAbility::FlakScreen,
        } => {
            if d.is_some_and(|d| now < d.flak_screen.ready_at) {
                return Err(R::AbilityCooldown);
            }
            let draw = rules.flak_screen.draw_mw.saturating_mul(KW_PER_MW);
            if crate::economy::ability_power_available(state, holding, draw, now) {
                Ok(())
            } else {
                Err(R::NoPower)
            }
        }
    }
}

/// Carries out a validated station order.
pub(crate) fn execute(state: &mut UniverseState, holding: HoldingId, order: HoldingOrder) {
    let now = order_tick(state);
    match order {
        HoldingOrder::LaunchReady => {}
        HoldingOrder::TurretsHold { hold } => {
            state
                .defence
                .holdings
                .entry(holding)
                .or_default()
                .turrets_hold = hold;
        }
        HoldingOrder::Ability {
            ability: StationAbility::ShieldSurge { sector },
        } => surge(state, holding, sector, now),
        HoldingOrder::Ability {
            ability: StationAbility::FlakScreen,
        } => {
            let rules = state.rules.defence.flak_screen.clone();
            let until = now.after(Ticks::from_secs(rules.duration_s));
            let draw = rules.draw_mw.saturating_mul(KW_PER_MW);
            if crate::economy::request_ability_power(state, holding, draw, now, until) {
                let d = state.defence.holdings.entry(holding).or_default();
                d.flak_screen.firing = Some((now, until));
                d.flak_screen.ready_at = now.after(Ticks::from_secs(rules.cooldown_s));
            }
        }
    }
}

/// SHLD SURGE on `sector` of `holding` at `now`.
fn surge(state: &mut UniverseState, holding: HoldingId, sector: u8, now: Tick) {
    let rules = state.rules.defence.shield_surge;
    let until = now.after(Ticks::from_secs(rules.draw_s));
    let draw = rules.draw_mw.saturating_mul(KW_PER_MW);
    if !crate::economy::request_ability_power(state, holding, draw, now, until) {
        return;
    }
    dome::refresh(state, holding);
    let Some(d) = state.defence.holdings.get_mut(&holding) else {
        return;
    };
    d.surge_ready_at = now.after(Ticks::from_secs(rules.cooldown_s));
    if let Some(s) = d.sectors.get_mut(usize::from(sector)) {
        let max = s.max;
        let restore =
            u64::try_from(u128::from(max) * u128::from(rules.restore_percent) / 100).unwrap_or(max);
        s.strength = s.strength.saturating_add(restore).min(max);
    }
}

/// FLAK SCREEN's volleys this tick (D-148), holding by holding.
pub(crate) fn flak_screens(state: &mut UniverseState, now: Tick, index: &ShipIndex) {
    let firing: Vec<(HoldingId, Tick)> = state
        .defence
        .holdings
        .iter_mut()
        .filter_map(|(&holding, d)| {
            let (started, until) = d.flak_screen.firing?;
            if now >= until {
                d.flak_screen.firing = None;
                return None;
            }
            Some((holding, started))
        })
        .collect();
    if firing.is_empty() {
        return;
    }
    let rules = state.rules.defence.flak_screen.clone();
    let interval = (rules.interval_ms / TICK_MILLIS).max(1);
    for (holding, started) in firing {
        if !now.since(started).get().is_multiple_of(interval) {
            continue;
        }
        let Some(h) = state.holdings.get(&holding) else {
            continue;
        };
        let owner = Owner::Commander(h.owner);
        let Some(site) = state.galaxy.site(h.site) else {
            continue;
        };
        let (system, centre) = (site.system, Vec2::from(site.position));
        let Some(ships) = index.systems.get(&system) else {
            continue;
        };
        // (distance, ship, position, velocity) of each target.
        let mut targets: Vec<(f64, ShipId, Vec2, Vec2)> = ships
            .near(centre, rules.reach_m)
            .map(|j| &ships.entries[j])
            .filter(|t| hostile(owner, t.owner) && !t.shrouded)
            .filter(|t| {
                matches!(
                    state.rules.combat.class(t.class).size,
                    SizeClass::Light | SizeClass::Medium
                )
            })
            .map(|t| ((t.position - centre).length(), t.id, t.position, t.velocity))
            .filter(|&(distance, ..)| distance <= rules.reach_m)
            .collect();
        targets.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        targets.truncate(usize::try_from(rules.max_bursts).unwrap_or(usize::MAX));
        for (_, _, at, velocity) in targets {
            launch(
                state,
                system,
                &rules.weapon,
                Shot {
                    owner,
                    shooter: Shooter::Holding(holding),
                    gun: Gun::FlakScreen,
                    from: centre,
                    at,
                    velocity,
                    lock: 0.0,
                },
            );
        }
    }
}
