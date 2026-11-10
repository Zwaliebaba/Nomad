//! The sign-off before the commander goes offline (D-077, D-162, D-214; A2 plan §4.8).
//!
//! At the last evaluation that starts at least one interval before the session ends, the
//! Strategist writes its Deputy's doctrine and fills the queues for the time away:
//! - **doctrine:** Balanced (D-082), every fleet given a role (uncommitted fleets as
//!   garrisons of the holding they are docked at, D-209; MULEs as reserves, D-207; an
//!   escort as its outpost's garrison, A1 plan §5.6), and the replacement list set to the
//!   garrisons (D-212);
//! - **release:** every fleet under direct control returns to doctrine (RK-10), so nothing
//!   lapses after the grace (D-162), except a kit's fleet on its way to the site, which stays
//!   under direct control so that its trip continues offline and deploys (D-291);
//! - **queues:** the next core kit if an expansion waits for one, then blueprint steps and
//!   garrison ships worth the pool and the income expected until the next session, within
//!   the 24-item limit (D-025, D-068: waiting items start as the money arrives). *Proposed:*
//!   waiting items start cheapest first, so while a kit waits only producers (and storage)
//!   are queued with it, and no ships, which would otherwise keep the pool below its price;
//!   due defence (D-309) is queued with it too, since it goes before the kit;
//! - **convoys:** routes set ([`super::build`]).
//!
//! The command rate drains at most a few commands a second, and commands still waiting
//! after 10 s expire (D-201), so each sign-off evaluation sends at most `sign_off_commands`,
//! most important first. *Proposed:* every evaluation from that one to the session's end
//! signs off, each sending what the state still lacks, so what did not fit is sent next.
//!
//! *Proposed* readings:
//! - with no next session known, the expected income is 12 h's;
//! - a fleet in space gets the reserve role of the holding its plan started from, or of the
//!   commander's first complete holding.

use std::collections::BTreeSet;

use nomad_types::{
    BuildItem, Command, DoctrineCommand, DoctrinePreset, FleetDoctrine, FleetId, FleetRole,
    HoldingId, ModuleKind, ReplacementEntry, ShipClass, Stock, Tick,
};

use super::build;
use super::expand::Stage;
use super::model::{Features, TICKS_PER_HOUR};
use super::view::{StrategicView, StrategistParams};
use super::{StrategistMemory, Turn};
use crate::Output;
use crate::view::FleetPlace;

/// Whether `now` is in the sign-off: no evaluation starting at least one interval before
/// the session ends comes after it.
pub(crate) fn due(view: &impl StrategicView, params: &StrategistParams<'_>, now: Tick) -> bool {
    view.session().ends.is_some_and(|ends| {
        now.get()
            .saturating_add(params.interval_ticks.saturating_mul(2))
            > ends.get()
    })
}

/// One sign-off evaluation.
#[expect(
    clippy::too_many_lines,
    reason = "the evaluation's order of work reads best in one place"
)]
pub(crate) fn run(
    view: &impl StrategicView,
    f: &Features,
    memory: &mut StrategistMemory,
    out: &mut Output,
) {
    let params = view.strategist();
    let limit = params.rules.sign_off_commands;
    let ends = view.session().ends;
    if memory.signed_off_for != ends {
        memory.signed_off_for = ends;
        memory.stats.sign_offs += 1;
    }
    let mut turn = Turn {
        f,
        free: f.economy.pool.minus(f.committed()),
        out,
        busy: BTreeSet::new(),
        command_planned: f.command_queued(),
        kit_held: Stock::ZERO,
    };
    let preset = view.preset_values(DoctrinePreset::Balanced);
    // Doctrine: Balanced.
    if view.doctrine_preset() != Some(DoctrinePreset::Balanced) {
        turn.send(Command::Doctrine(DoctrineCommand::SetPreset {
            preset: DoctrinePreset::Balanced,
        }));
    }
    // Every fleet released, with a role.
    let escorts: Vec<(FleetId, HoldingId)> = memory
        .plans
        .iter()
        .filter_map(|p| p.expansion.as_ref())
        .filter(|e| e.stage == Stage::Return)
        .filter_map(|e| {
            let escort = e.escort?;
            let site = f.site(e.site)?;
            let holder = site.holder.filter(|h| h.owner == f.me)?;
            Some((escort, holder.holding))
        })
        .collect();
    let origins: Vec<(FleetId, HoldingId)> = memory
        .plans
        .iter()
        .filter_map(|p| p.expansion.as_ref())
        .flat_map(|e| e.fleets().into_iter().map(move |fl| (fl, e.holding)))
        .collect();
    // Claim trips under way stay as they are: their orders continue past the grace and the
    // kit deploys on arrival (D-291).
    let trips: Vec<FleetId> = memory
        .plans
        .iter()
        .filter_map(|p| p.expansion.as_ref())
        .filter(|e| e.stage == Stage::Travel && e.carrying(view))
        .filter_map(|e| e.carrier)
        .collect();
    let fallback = f.complete().next().map(|h| h.id);
    let mut garrisons: Vec<(FleetId, Vec<(ShipClass, u32)>)> = Vec::new();
    for fleet in &f.fleets {
        if turn.out.commands.len() >= limit {
            break;
        }
        if trips.contains(&fleet.id) {
            continue;
        }
        if view.direct_control(fleet.id).is_some() {
            turn.send(Command::Doctrine(DoctrineCommand::Release {
                fleet: fleet.id,
            }));
        }
        if f.convoys.contains(&fleet.id) {
            continue;
        }
        let mules = fleet.ships.iter().all(|&(_, c)| c == ShipClass::Mule);
        let role = if let Some(&(_, outpost)) = escorts.iter().find(|(e, _)| *e == fleet.id) {
            Some(FleetRole::Garrison { holding: outpost })
        } else {
            match fleet.place {
                FleetPlace::Docked(h) if mules => Some(FleetRole::Reserve { holding: h }),
                FleetPlace::Docked(h) => Some(FleetRole::Garrison { holding: h }),
                FleetPlace::InSpace(_) | FleetPlace::Travelling => origins
                    .iter()
                    .find(|(fl, _)| *fl == fleet.id)
                    .map(|&(_, h)| h)
                    .or(fallback)
                    .map(|holding| FleetRole::Reserve { holding }),
            }
        };
        let Some(role) = role else {
            continue;
        };
        if let FleetRole::Garrison { .. } = role {
            let mut counts: Vec<(ShipClass, u32)> = Vec::new();
            for &(_, class) in &fleet.ships {
                if class == ShipClass::Mule {
                    continue;
                }
                match counts.iter_mut().find(|(c, _)| *c == class) {
                    Some(entry) => entry.1 += 1,
                    None => counts.push((class, 1)),
                }
            }
            counts.sort_by_key(|&(c, _)| c);
            garrisons.push((fleet.id, counts));
        }
        let current = view.fleet_doctrine(fleet.id).and_then(|d| d.role);
        if current.as_ref() != Some(&role) {
            turn.send(Command::Doctrine(DoctrineCommand::SetFleet {
                fleet: fleet.id,
                doctrine: Some(FleetDoctrine {
                    role: Some(role),
                    ..preset.fleet()
                }),
            }));
        }
    }
    // The replacement list: the garrisons, as they are (D-208, D-212).
    let entries: Vec<ReplacementEntry> = garrisons
        .iter()
        .flat_map(|(fleet, counts)| {
            counts.iter().map(move |&(class, count)| ReplacementEntry {
                fleet: *fleet,
                class,
                count,
                role: None,
            })
        })
        .take(32)
        .collect();
    if turn.out.commands.len() < limit && entries != view.replacement_list() {
        turn.send(Command::Doctrine(DoctrineCommand::SetReplacement {
            entries,
        }));
    }
    // Queues worth the expected income until the next session.
    let hours = view
        .session()
        .next_starts
        .map_or(12, |next| next.since(f.now).get().div_ceil(TICKS_PER_HOUR));
    let income = f.economy.income_last_hour;
    let mut budget = turn.free.plus(Stock {
        alloy: income.alloy.saturating_mul(hours),
        crystal: income.crystal.saturating_mul(hours),
    });
    fill_queues(view, &mut turn, memory, &params, &mut budget, limit);
    // Convoys (D-216): routes set.
    if turn.out.commands.len() < limit {
        let mut plan = super::Plan::new(super::Goal::Logistics, f.now);
        let _ = build::advance_logistics(view, &mut turn, &mut plan);
    }
}

/// Queues the next kit, then build-out steps and garrison ships, while `budget` covers them.
#[expect(
    clippy::too_many_lines,
    reason = "the kit, the steps and the ships in one budget"
)]
fn fill_queues(
    view: &impl StrategicView,
    turn: &mut Turn<'_>,
    memory: &mut StrategistMemory,
    params: &StrategistParams<'_>,
    budget: &mut Stock,
    limit: usize,
) {
    let f = turn.f;
    let queue_max = f.numbers.queue_max;
    // The kit an expansion waits for.
    let expanding = memory
        .plans
        .iter()
        .filter_map(|p| p.expansion.as_ref())
        .find(|e| e.stage == Stage::Prepare);
    if let Some(e) = expanding
        && let Some(h) = f.holding(e.holding)
        && h.kits == 0
        && !h.queue.iter().any(|q| q.item == BuildItem::CoreKit)
        && !e.carrying(view)
        && f.economy.cap.covers(f.economy.core_kit_price)
        && h.queue.len() < queue_max
        && view.build_check(e.holding, BuildItem::CoreKit) == crate::view::BuildCheck::Ok
    {
        turn.build(e.holding, BuildItem::CoreKit);
        *budget = budget.minus(f.economy.core_kit_price);
    }
    // Waiting items start in the order the pool affords them, cheapest first (D-068), so
    // while a kit waits only the producers that feed it are queued with it, and no ships.
    let kit_waiting = expanding.is_some()
        || f.holdings.iter().any(|h| {
            h.queue
                .iter()
                .any(|q| q.item == BuildItem::CoreKit && !q.started)
        });
    // Build-out steps, as if each one queued made room for the next.
    let mut g = f.clone();
    let expand_score = super::expand::score(view, f, None, params);
    let mut sent = 0;
    while turn.out.commands.len() < limit && sent < 64 {
        sent += 1;
        let steps = build::steps(
            &g,
            &mut memory.forward_dock,
            &memory.struck,
            params,
            expand_score,
        );
        let Some(step) = steps
            .iter()
            .filter(|s| {
                !kit_waiting
                    || s.due
                    || matches!(
                        s.kind,
                        ModuleKind::Harvester
                            | ModuleKind::Extractor
                            | ModuleKind::Refinery
                            | ModuleKind::Solar
                            | ModuleKind::Storage
                    )
            })
            .find(|s| {
                g.holding(s.holding)
                    .is_some_and(|h| h.queue.len() < queue_max)
            })
            .copied()
        else {
            break;
        };
        if !budget.covers(step.price) {
            break;
        }
        let item = BuildItem::Module {
            kind: step.kind,
            slot: step.slot,
        };
        if view.build_check(step.holding, item) != crate::view::BuildCheck::Ok {
            break;
        }
        turn.build(step.holding, item);
        *budget = budget.minus(step.price);
        if let Some(h) = g.holdings.iter_mut().find(|h| h.id == step.holding) {
            // Queued, and so no longer blocking: the holding may take its next step.
            h.modules.push(super::view::ModuleStatus {
                slot: step.slot,
                kind: step.kind,
                building: true,
                disabled: false,
            });
        }
    }
    // Garrison ships with what is left, cycling through the composition.
    let rules = params.rules;
    let mut room = f
        .economy
        .command_cap
        .saturating_sub(f.economy.command_used + turn.command_planned);
    let classes: Vec<ShipClass> = rules
        .composition_permille
        .keys()
        .copied()
        .filter(|&c| c != ShipClass::Warden)
        .collect();
    let holding = f
        .complete()
        .filter(|h| h.queue.len() < queue_max)
        .map(|h| h.id)
        .next();
    let Some(holding) = holding.filter(|_| !kit_waiting) else {
        return;
    };
    let mut i = 0;
    while turn.out.commands.len() < limit && !classes.is_empty() && i < 64 {
        let class = classes[i % classes.len()];
        i += 1;
        let Some((price, command)) = f.numbers.ship(class) else {
            break;
        };
        if command > room || !budget.covers(price) {
            break;
        }
        if view.build_check(holding, BuildItem::Ship { class }) != crate::view::BuildCheck::Ok {
            break;
        }
        turn.build(holding, BuildItem::Ship { class });
        *budget = budget.minus(price);
        room -= command;
        turn.command_planned += command;
    }
}
