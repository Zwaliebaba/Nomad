//! Threat assessment, alert and the alert actions of each holding (doc 13 §4 items 1–2;
//! D-016, D-147, D-148, D-206; A1 plan §5.3, §5.4).
//!
//! - **Alert** (every 0.5 s per holding, on its slot): a holding is on alert while a hostile
//!   ship in the commander's visibility set is inside its defended zone (the doctrine's
//!   radius around the site), or the inbox shows hostile fire on the holding, its modules or
//!   dome, or on an own ship inside the zone. The alert ends 60 s after the last such event.
//!   Fire on an asset outside every zone puts no holding on alert. Hostile is every other
//!   owner (no alliances in the MVP, D-040).
//! - **Alert start:** the doctrine's alert power profile, remembering the profile before;
//!   [`DeputyEvent::AlertStarted`]. The garrison launches from its own fleet slots
//!   (`fleets`).
//! - **On alert:** SHLD SURGE on the weakest closed sector below the doctrine threshold, and
//!   FLAK SCREEN once the doctrine's count of light hostiles is within 3 km of the dome
//!   centre, each when the validator accepts it now, so when power and the cooldown allow.
//! - **Alert end:** the profile before the alert again; [`DeputyEvent::AlertEnded`].
//!
//! *Proposed* readings:
//! - a sensed hostile's event tick is the sensors pass that saw it (D-173), so the latency
//!   counts from the pass; fire counts from the tick of the hit;
//! - fire on an own ship counts for a holding when the ship's fleet is inside the zone when
//!   the Deputy reads the entry;
//! - the profile changes only at alert start and end (A1 plan §5.2): at the start only if it
//!   differs from the alert profile, at the end back to the remembered one, if it differs now;
//! - SHLD SURGE takes the closed sector with the lowest share of its maximum (then the lowest
//!   index), counting its latency from the last fire on the holding; FLAK SCREEN counts every
//!   sensed light hostile, shrouded or not, and its latency from the sensors pass;
//! - an ability is not issued again while the last one is unanswered.

use std::collections::BTreeMap;

use nomad_types::{
    Command, EconomyCommand, HoldingId, HoldingOrder, PositionMetres, PowerProfile, StationAbility,
    SystemId, Tick, Ticks,
};
use serde::{Deserialize, Serialize};

use super::{DeputyEvent, Stagger};
use crate::inbox::{InboxEntry, InboxEvent};
use crate::view::{FleetPlace, OwnHolding, View};
use crate::{Issue, Layer, Output};

/// Alerts and the fire not yet weighed, per own holding.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct ThreatMemory {
    /// The latest fire on each holding or in its zone, read from the inbox and not yet
    /// weighed by the holding's evaluation.
    fire: BTreeMap<HoldingId, Tick>,
    /// The holdings on alert.
    alerts: BTreeMap<HoldingId, Alert>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Alert {
    /// The event that started it.
    since: Tick,
    /// The latest event; the alert ends a window after it.
    last: Tick,
    /// The profile to restore at the end, if the Deputy changed it.
    restore: Option<PowerProfile>,
    /// Ability orders issued and not yet answered.
    waiting: Vec<Command>,
}

impl ThreatMemory {
    /// The tick of the event that started `holding`'s alert, if it is on alert.
    pub(super) fn alert(&self, holding: HoldingId) -> Option<Tick> {
        self.alerts.get(&holding).map(|a| a.since)
    }
}

/// Squared distance between two points, m² (no square root: exact comparisons, D-049).
pub(super) fn distance2(a: PositionMetres, b: PositionMetres) -> f64 {
    let (dx, dy) = (a.x - b.x, a.y - b.y);
    dx * dx + dy * dy
}

/// Whether `p` is within `radius_m` of `centre`.
pub(super) fn within(p: PositionMetres, centre: PositionMetres, radius_m: f64) -> bool {
    distance2(p, centre) <= radius_m * radius_m
}

/// Reads fire on own holdings and zones, and the answers to ability orders.
pub(super) fn read(view: &impl View, memory: &mut ThreatMemory, fresh: &[&InboxEntry]) {
    let me = view.observer();
    let mut holdings: Option<Vec<OwnHolding>> = None;
    for entry in fresh {
        let hit = match &entry.event {
            InboxEvent::SectorDamaged { holding, by, .. }
            | InboxEvent::ModuleDamaged { holding, by, .. } => {
                if *by == me {
                    Vec::new()
                } else {
                    vec![*holding]
                }
            }
            InboxEvent::ShipDamaged { fleet, by, .. } if *by != me => {
                let Some(FleetPlace::InSpace(at)) = view.own_fleet(*fleet).map(|f| f.place) else {
                    continue;
                };
                let holdings = holdings.get_or_insert_with(|| view.own_holdings());
                zones_containing(view, holdings, at.system, at.position)
            }
            InboxEvent::Outcome {
                layer: Layer::Deputy,
                command,
                ..
            } => {
                for alert in memory.alerts.values_mut() {
                    alert.waiting.retain(|c| c != command);
                }
                continue;
            }
            _ => continue,
        };
        for holding in hit {
            let at = memory.fire.entry(holding).or_insert(entry.tick);
            *at = (*at).max(entry.tick);
        }
    }
}

/// The own holdings in `system` whose defended zone contains `p`.
fn zones_containing(
    view: &impl View,
    holdings: &[OwnHolding],
    system: SystemId,
    p: PositionMetres,
) -> Vec<HoldingId> {
    holdings
        .iter()
        .filter(|h| h.system == system)
        .filter(|h| {
            view.holding_doctrine(h.id)
                .is_some_and(|d| within(p, h.centre, f64::from(d.zone_radius_m)))
        })
        .map(|h| h.id)
        .collect()
}

/// Weighs the due holdings: alert start and end, the profile and the abilities.
pub(super) fn act(view: &impl View, memory: &mut ThreatMemory, stagger: Stagger, out: &mut Output) {
    let now = view.now();
    let params = view.deputy_params();
    let window = Ticks::from_secs(params.alert_window_s);
    let holdings = view.own_holdings();
    // Holdings lost or given up are no longer the Deputy's.
    memory
        .alerts
        .retain(|id, _| holdings.iter().any(|h| h.id == *id));
    memory
        .fire
        .retain(|id, _| holdings.iter().any(|h| h.id == *id));
    for holding in &holdings {
        if !stagger.due(holding.id.sim_id().get()) {
            continue;
        }
        let Some(doctrine) = view.holding_doctrine(holding.id) else {
            continue;
        };
        let zone_m = f64::from(doctrine.zone_radius_m);
        let mut last = memory.fire.remove(&holding.id);
        let sensed = if view.anything_sensed_in(holding.system) {
            view.sensed_fleets_in(holding.system)
        } else {
            Vec::new()
        };
        let in_zone = sensed
            .iter()
            .flat_map(|f| &f.ships)
            .any(|s| within(s.position, holding.centre, zone_m));
        if in_zone {
            let seen = view.picture_tick();
            last = Some(last.map_or(seen, |t| t.max(seen)));
        }
        let issue = |out: &mut Output, trigger: Tick, command: Command| {
            out.commands.push(Issue { trigger, command });
        };
        let profile = |p| {
            Command::Economy(EconomyCommand::SetPowerProfile {
                holding: holding.id,
                profile: p,
            })
        };
        let alert = match memory.alerts.get_mut(&holding.id) {
            None => {
                let Some(t) = last.filter(|&t| now < t.after(window)) else {
                    continue;
                };
                out.events.push(DeputyEvent::AlertStarted {
                    holding: holding.id,
                });
                let current = view.power_profile(holding.id);
                let restore = current.filter(|&p| p != doctrine.alert_profile);
                if restore.is_some() {
                    issue(out, t, profile(doctrine.alert_profile));
                }
                memory.alerts.entry(holding.id).or_insert(Alert {
                    since: t,
                    last: t,
                    restore,
                    waiting: Vec::new(),
                })
            }
            Some(alert) => {
                if let Some(t) = last {
                    alert.last = alert.last.max(t);
                }
                if now >= alert.last.after(window) {
                    let restore = alert.restore;
                    memory.alerts.remove(&holding.id);
                    out.events.push(DeputyEvent::AlertEnded {
                        holding: holding.id,
                    });
                    if let Some(p) = restore
                        && view.power_profile(holding.id) != Some(p)
                    {
                        issue(out, now, profile(p));
                    }
                    continue;
                }
                alert
            }
        };
        if alert.waiting.is_empty() {
            abilities(view, holding, &doctrine, &sensed, alert, out);
        }
    }
}

/// SHLD SURGE and FLAK SCREEN on their triggers, one at a time (D-147, D-148).
fn abilities(
    view: &impl View,
    holding: &OwnHolding,
    doctrine: &nomad_types::HoldingDoctrine,
    sensed: &[crate::view::SensedFleet],
    alert: &mut Alert,
    out: &mut Output,
) {
    let now = view.now();
    let mut issue = |trigger: Tick, command: Command| {
        out.commands.push(Issue {
            trigger,
            command: command.clone(),
        });
        alert.waiting.push(command);
    };
    {
        // SHLD SURGE on the weakest closed sector below the threshold (D-147).
        if let Some(defence) = view.defence(holding.id).filter(|d| d.dome) {
            let below = |s: &crate::view::Sector| {
                !s.open
                    && s.max_milli > 0
                    && u128::from(s.strength_milli) * 1_000
                        < u128::from(doctrine.surge_below_permille) * u128::from(s.max_milli)
            };
            let weakest = defence
                .sectors
                .iter()
                .enumerate()
                .filter(|(_, s)| below(s))
                .min_by(|(i, a), (j, b)| {
                    // a.strength / a.max against b.strength / b.max, exactly.
                    (u128::from(a.strength_milli) * u128::from(b.max_milli))
                        .cmp(&(u128::from(b.strength_milli) * u128::from(a.max_milli)))
                        .then(i.cmp(j))
                })
                .and_then(|(i, _)| u8::try_from(i).ok());
            if let Some(sector) = weakest {
                let command = Command::Holding {
                    holding: holding.id,
                    order: HoldingOrder::Ability {
                        ability: StationAbility::ShieldSurge { sector },
                    },
                };
                if view.accepts(&command) {
                    issue(alert.last.min(now), command);
                    return;
                }
            }
        }
        // FLAK SCREEN on the count of light hostiles near the dome (D-148, doc 13 §9).
        if let Some(count) = doctrine.flak_light_hostiles {
            let radius = f64::from(view.deputy_params().flak_trigger_radius_m);
            let light = sensed
                .iter()
                .flat_map(|f| &f.ships)
                .filter(|s| view.class_light(s.class) && within(s.position, holding.centre, radius))
                .count();
            if light >= usize::try_from(count).unwrap_or(usize::MAX) {
                let command = Command::Holding {
                    holding: holding.id,
                    order: HoldingOrder::Ability {
                        ability: StationAbility::FlakScreen,
                    },
                };
                if view.accepts(&command) {
                    issue(view.picture_tick(), command);
                }
            }
        }
    }
}
