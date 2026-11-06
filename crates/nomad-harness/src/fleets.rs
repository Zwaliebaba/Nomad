//! `--fleets`: the fleet demo (S3 plan §6 task 11, §7 "Performance").
//!
//! Generates the MVP stage-1 sector with its four start kits (doc 14 §12), then plays a
//! short script and continuous, seeded random orders for every commander: launches, moves,
//! patrols, jumps, retreats, docking, guarding, attacking, station-keeping, splits, merges
//! and rules of engagement, each replacing or appending. Every command goes through the
//! simulation's validator (D-077), so some are rejected; that is part of the demo.
//!
//! It reports what happened (commands by outcome, jumps with their transit times, docking,
//! completed and dropped orders), checks the ships and fleets for broken invariants every
//! simulated minute, and measures the time each tick takes. The random orders come from the
//! harness's own seeded stream, so the same seed gives the same run and the same hash.

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::time::Instant;

use nomad_sim::galaxy::{STAGE1_COMMANDERS, generate_stage1_with_rules};
use nomad_sim::snapshot::{decode, encode, state_hash};
use nomad_sim::{
    Command, CommandRejection, CommanderId, CommanderInput, CommanderInputKind, FleetEvent,
    FleetId, FleetOrder, FleetView, HoldingId, HoldingOrder, Location, Owner, Place, Queueing,
    RngStreams, Rules, RulesOfEngagement, SeasonConfig, StreamKey, SystemId, TICKS_PER_SECOND,
    TickInputs, UniverseState, Vec2, step,
};

/// Commanders of the generated Universe: 1 human and 3 AI (doc 10 §3).
const COMMANDERS: [CommanderId; STAGE1_COMMANDERS] = [
    CommanderId(1),
    CommanderId(2),
    CommanderId(3),
    CommanderId(4),
];

/// How often the consistency check runs: every simulated minute.
const CHECK_EVERY_TICKS: u64 = 600;

/// Options of a fleet demo run.
pub struct FleetRun {
    /// The season seed; the random orders are seeded from it too.
    pub seed: u64,
    /// How many ticks to run.
    pub ticks: u64,
    /// Each commander gives one random command this often.
    pub order_interval_secs: u64,
    /// Write a snapshot and read it back before this tick.
    pub restore_at: Option<u64>,
    /// The rules the Universe is created with.
    pub rules: Rules,
}

/// What the run counted.
#[derive(Default)]
struct Stats {
    applied: u64,
    rejected: BTreeMap<CommandRejection, u64>,
    departed: u64,
    arrived: u64,
    transit_ticks: Vec<u64>,
    departures: BTreeMap<FleetId, u64>,
    spool_ups: u64,
    spool_cancels: u64,
    docked: u64,
    launched: u64,
    completed: u64,
    dropped: u64,
    checks: u64,
    errors: Vec<String>,
    tick_micros: Vec<f64>,
}

impl Stats {
    fn record_event(&mut self, tick: u64, event: &FleetEvent) {
        match *event {
            FleetEvent::Launched { .. } => self.launched += 1,
            FleetEvent::Docked { .. } => self.docked += 1,
            FleetEvent::OrderCompleted { .. } => self.completed += 1,
            FleetEvent::OrderDropped { .. } => self.dropped += 1,
            FleetEvent::SpoolUpStarted { .. } => self.spool_ups += 1,
            FleetEvent::SpoolUpCancelled { .. } => self.spool_cancels += 1,
            FleetEvent::Departed { fleet, .. } => {
                self.departed += 1;
                self.departures.insert(fleet, tick);
            }
            FleetEvent::Arrived { fleet, .. } => {
                self.arrived += 1;
                if let Some(departed) = self.departures.remove(&fleet) {
                    self.transit_ticks.push(tick - departed);
                }
            }
        }
    }
}

/// Runs the fleet demo and writes its report to `out`.
pub fn run(out: &mut impl Write, args: FleetRun) -> Result<(), String> {
    let mut state = generate_stage1_with_rules(
        SeasonConfig {
            seed: args.seed,
            season_start_unix_ms: 0,
        },
        COMMANDERS,
        args.rules,
    )
    .map_err(|e| e.to_string())?;
    let mut orders = RngStreams::new(args.seed ^ 0x6f72_6465_7273); // "orders"
    let interval = (args.order_interval_secs * TICKS_PER_SECOND).max(1);
    let mut stats = Stats::default();
    let started = Instant::now();

    for t in 0..args.ticks {
        if Some(t) == args.restore_at {
            let bytes = encode(&state).map_err(|e| e.to_string())?;
            state = decode(&bytes).map_err(|e| e.to_string())?;
        }
        let mut commanders = Vec::new();
        for (n, &commander) in COMMANDERS.iter().enumerate() {
            let owner = Owner::Commander(commander);
            let script = scripted(&state, t, owner);
            // Commanders take turns, spread over the interval.
            let random = (t >= 100 && (t + n as u64 * interval / 4).is_multiple_of(interval))
                .then(|| random_command(&state, &mut orders, owner))
                .flatten();
            for (seq, command) in (0..).zip(script.into_iter().chain(random)) {
                commanders.push(CommanderInput {
                    commander,
                    client_seq: t * 16 + seq,
                    kind: CommanderInputKind::Command(command),
                });
            }
        }
        let tick_started = Instant::now();
        let output = step(
            &mut state,
            &TickInputs {
                operator: vec![],
                commanders,
            },
        );
        stats
            .tick_micros
            .push(tick_started.elapsed().as_secs_f64() * 1e6);
        for result in &output.commands {
            match result.outcome {
                Ok(()) => stats.applied += 1,
                Err(reason) => *stats.rejected.entry(reason).or_default() += 1,
            }
        }
        for event in &output.fleet_events {
            stats.record_event(output.tick.get(), event);
        }
        if (t + 1) % CHECK_EVERY_TICKS == 0 || t + 1 == args.ticks {
            stats.checks += 1;
            for problem in state.fleet_problems() {
                stats
                    .errors
                    .push(format!("tick {}: {problem}", output.tick.get()));
            }
        }
    }
    let elapsed = started.elapsed().as_secs_f64();
    let hash = state_hash(&state).map_err(|e| e.to_string())?;
    report(
        out, args.seed, &state, &stats, args.ticks, interval, elapsed, hash,
    )
    .map_err(|e| e.to_string())?;
    if stats.errors.is_empty() {
        Ok(())
    } else {
        Err(format!("{} consistency errors", stats.errors.len()))
    }
}

/// The scripted opening: every commander launches at once; a minute later the MULEs split
/// off and dock again, and the warships of commander 1 head for the hub.
fn scripted(state: &UniverseState, tick: u64, owner: Owner) -> Vec<Command> {
    let Some((holding, system)) = mother_station(state, owner) else {
        return Vec::new();
    };
    match tick {
        0 => vec![Command::Holding {
            holding,
            order: HoldingOrder::LaunchReady,
        }],
        600 => {
            let Some(&fleet) = own_fleets(state, owner).first() else {
                return Vec::new();
            };
            let mules: Vec<_> = state
                .fleet(fleet)
                .map(|f| f.ships)
                .unwrap_or_default()
                .into_iter()
                .filter(|&s| {
                    state
                        .ship(s)
                        .is_some_and(|v| v.class == nomad_sim::ShipClass::Mule)
                })
                .collect();
            vec![Command::CreateFleet { ships: mules }]
        }
        601 => {
            let fleets = own_fleets(state, owner);
            let mut commands = Vec::new();
            if let Some(&mules) = fleets.last() {
                commands.push(Command::Fleet {
                    fleet: mules,
                    order: FleetOrder::Dock { at: holding },
                    queue: Queueing::Replace,
                });
            }
            if owner == Owner::Commander(COMMANDERS[0])
                && let (Some(&warships), Some(hub)) = (fleets.first(), hub(state))
                && hub != system
            {
                commands.push(Command::Fleet {
                    fleet: warships,
                    order: FleetOrder::Jump { system: hub },
                    queue: Queueing::Replace,
                });
            }
            commands
        }
        _ => Vec::new(),
    }
}

fn mother_station(state: &UniverseState, owner: Owner) -> Option<(HoldingId, SystemId)> {
    let (id, holding) = state
        .holdings()
        .find(|(_, h)| Owner::Commander(h.owner) == owner && h.mother_station)?;
    Some((id, state.galaxy().site(holding.site)?.system))
}

fn own_fleets(state: &UniverseState, owner: Owner) -> Vec<FleetId> {
    state
        .fleet_ids()
        .into_iter()
        .filter(|&f| state.fleet(f).is_some_and(|v| v.owner == owner))
        .collect()
}

fn hub(state: &UniverseState) -> Option<SystemId> {
    state
        .galaxy()
        .systems()
        .find(|(_, s)| s.kind == nomad_sim::SystemKind::Hub)
        .map(|(id, _)| id)
}

/// A random point 3–15 km from a random site of `system`, or near a gate if it has none.
fn random_point(state: &UniverseState, rng: &mut RngStreams, system: SystemId) -> Location {
    let key = StreamKey::global(1);
    let Some(sys) = state.galaxy().system(system) else {
        return Location {
            system,
            position: nomad_sim::PositionMetres::ORIGIN,
        };
    };
    let anchors: Vec<Vec2> = if sys.sites.is_empty() {
        sys.gates
            .iter()
            .filter_map(|&g| state.galaxy().gate(g).map(|g| g.position.into()))
            .collect()
    } else {
        sys.sites
            .iter()
            .filter_map(|&s| state.galaxy().site(s).map(|s| s.position.into()))
            .collect()
    };
    let base = anchors
        .get(rng.index(key, anchors.len()))
        .copied()
        .unwrap_or(Vec2::ZERO);
    let radius = 3_000.0 + rng.unit_f64(key) * 12_000.0;
    let angle = rng.unit_f64(key) * std::f64::consts::TAU;
    Location {
        system,
        position: (base + Vec2::from_angle(angle) * radius).into(),
    }
}

/// Where a new order of a fleet starts.
enum Start {
    /// Docked at this holding now: only a launch makes sense.
    DockedNow(HoldingId),
    /// Docked once the queue has run: only a queued launch makes sense.
    DockedLater,
    /// In space in this system.
    InSystem(SystemId),
}

/// Where a new order starts. A replacing order starts here, or where the lane ends, since
/// transit cannot be interrupted (D-042); an appended one where the queue ends.
fn order_start(state: &UniverseState, view: &FleetView, queue: Queueing) -> Option<Start> {
    let here = match view.place {
        Place::Docked(holding) => return Some(Start::DockedNow(holding)),
        Place::InTransit { to_gate, .. } => state.galaxy().gate(to_gate)?.system,
        Place::InSystem(system) => system,
    };
    if queue == Queueing::Replace {
        return Some(Start::InSystem(here));
    }
    let (mut system, mut docked) = (here, false);
    for order in view.order.iter().chain(&view.queued) {
        match order {
            FleetOrder::Jump { system: to } => (system, docked) = (*to, false),
            FleetOrder::Dock { .. } | FleetOrder::Retreat { .. } => docked = true,
            FleetOrder::Launch => docked = false,
            _ => {}
        }
    }
    Some(if docked {
        Start::DockedLater
    } else {
        Start::InSystem(system)
    })
}

/// One random command for one of `owner`'s fleets.
fn random_command(state: &UniverseState, rng: &mut RngStreams, owner: Owner) -> Option<Command> {
    let key = StreamKey::global(1);
    let fleets = own_fleets(state, owner);
    let fleet = *fleets.get(rng.index(key, fleets.len()))?;
    let view = state.fleet(fleet)?;
    let queue = if rng.below(key, 10) < 3 {
        Queueing::Append
    } else {
        Queueing::Replace
    };
    let give = |order| {
        Some(Command::Fleet {
            fleet,
            order,
            queue,
        })
    };
    let system = match order_start(state, &view, queue)? {
        Start::DockedNow(holding) if rng.below(key, 4) == 0 => {
            return Some(Command::Holding {
                holding,
                order: HoldingOrder::LaunchReady,
            });
        }
        Start::DockedNow(_) | Start::DockedLater => return give(FleetOrder::Launch),
        Start::InSystem(system) => system,
    };
    let others_here = |mine: bool| -> Vec<FleetId> {
        state
            .fleet_ids()
            .into_iter()
            .filter(|&f| f != fleet)
            .filter(|&f| {
                state.fleet(f).is_some_and(|v| {
                    (v.owner == owner) == mine && v.place == Place::InSystem(system)
                })
            })
            .collect()
    };
    match rng.below(key, 100) {
        0..25 => give(FleetOrder::Move {
            to: random_point(state, rng, system),
        }),
        25..35 => {
            let route = (0..3).map(|_| random_point(state, rng, system)).collect();
            give(FleetOrder::Patrol { route })
        }
        35..55 => {
            let systems: Vec<SystemId> = state
                .galaxy()
                .systems()
                .map(|(id, _)| id)
                .filter(|&id| id != system)
                .collect();
            let to = *systems.get(rng.index(key, systems.len()))?;
            give(FleetOrder::Jump { system: to })
        }
        55..63 => give(FleetOrder::Retreat { to: None }),
        63..68 => match mother_station(state, owner)? {
            (holding, at) if at == system => give(FleetOrder::Dock { at: holding }),
            _ => give(FleetOrder::Retreat { to: None }),
        },
        68..73 => {
            let friends = others_here(true);
            let object = friends.get(rng.index(key, friends.len()))?;
            give(FleetOrder::Guard {
                object: object.sim_id(),
            })
        }
        73..78 => {
            let hostiles = others_here(false);
            let target = hostiles.get(rng.index(key, hostiles.len()))?;
            give(FleetOrder::Attack {
                target: target.sim_id(),
            })
        }
        78..81 => give(FleetOrder::StationKeep { relative_to: None }),
        81..88 => {
            let ships = view.ships;
            (ships.len() >= 2).then(|| Command::CreateFleet {
                ships: ships[..ships.len() / 2].to_vec(),
            })
        }
        88..96 => {
            let friends = others_here(true);
            let into = *friends.get(rng.index(key, friends.len()))?;
            Some(Command::MergeFleets { fleet, into })
        }
        _ => give(FleetOrder::SetRulesOfEngagement {
            rules: [
                RulesOfEngagement::WeaponsFree,
                RulesOfEngagement::ReturnFire,
                RulesOfEngagement::HoldFire,
            ][rng.index(key, 3)],
        }),
    }
}

#[expect(clippy::too_many_arguments, reason = "a report of one run's figures")]
fn report(
    out: &mut impl Write,
    seed: u64,
    state: &UniverseState,
    stats: &Stats,
    ticks: u64,
    interval: u64,
    elapsed: f64,
    hash: u64,
) -> io::Result<()> {
    let hours = ticks as f64 / TICKS_PER_SECOND as f64 / 3_600.0;
    writeln!(
        out,
        "fleets | seed {seed} | {} start kits | {hours:.1} h ({ticks} ticks) | a random order \
         per commander every {} s",
        COMMANDERS.len(),
        interval / TICKS_PER_SECOND,
    )?;
    let rejected: u64 = stats.rejected.values().sum();
    let reasons: Vec<String> = stats
        .rejected
        .iter()
        .map(|(reason, n)| format!("{reason:?} {n}"))
        .collect();
    writeln!(
        out,
        "commands: {} applied, {rejected} rejected ({})",
        stats.applied,
        reasons.join(", ")
    )?;
    let minutes: Vec<f64> = stats
        .transit_ticks
        .iter()
        .map(|&t| t as f64 / TICKS_PER_SECOND as f64 / 60.0)
        .collect();
    let transits = if minutes.is_empty() {
        "-".to_owned()
    } else {
        let (lo, hi) = minutes
            .iter()
            .fold((f64::INFINITY, 0.0_f64), |(lo, hi), &m| {
                (lo.min(m), hi.max(m))
            });
        let mean = minutes.iter().sum::<f64>() / minutes.len() as f64;
        format!("min {lo:.0} / mean {mean:.1} / max {hi:.0} min")
    };
    writeln!(
        out,
        "jumps: {} spool-ups ({} cancelled), {} departed, {} arrived; transit {transits}",
        stats.spool_ups, stats.spool_cancels, stats.departed, stats.arrived,
    )?;
    let in_space = state
        .fleet_ids()
        .into_iter()
        .filter(|&f| matches!(state.fleet(f).map(|v| v.place), Some(Place::InSystem(_))))
        .count();
    writeln!(
        out,
        "fleets: {} launches, {} dockings, {} orders completed, {} dropped | at the end {} \
         fleets ({in_space} in space), {} ships",
        stats.launched,
        stats.docked,
        stats.completed,
        stats.dropped,
        state.fleet_ids().len(),
        state.ship_ids().len(),
    )?;
    writeln!(
        out,
        "consistency: {} checks, {} errors",
        stats.checks,
        stats.errors.len()
    )?;
    for error in stats.errors.iter().take(10) {
        writeln!(out, "  {error}")?;
    }
    let mut micros = stats.tick_micros.clone();
    micros.sort_by(f64::total_cmp);
    // The tick time at `per_mille` / 1,000 of the sorted list, rounded down.
    let percentile = |per_mille: usize| {
        let index = micros.len().saturating_sub(1) * per_mille / 1_000;
        micros.get(index).copied().unwrap_or(0.0)
    };
    let mean_us = micros.iter().sum::<f64>() / micros.len().max(1) as f64;
    let speed = if elapsed > 0.0 {
        hours * 3_600.0 / elapsed
    } else {
        f64::INFINITY
    };
    writeln!(
        out,
        "tick time: mean {mean_us:.0} µs, p99 {:.0} µs, max {:.0} µs | elapsed {elapsed:.1} s | \
         speed {speed:.0}x real time | hash {hash:016x}",
        percentile(990),
        percentile(1_000),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_run_reports_is_consistent_and_repeats() {
        let run_once = |restore_at| {
            let mut out = Vec::new();
            run(
                &mut out,
                FleetRun {
                    seed: 7,
                    ticks: 3_000,
                    order_interval_secs: 20,
                    restore_at,
                    rules: Rules::embedded(),
                },
            )
            .unwrap();
            String::from_utf8(out).unwrap()
        };
        let text = run_once(None);
        assert!(text.contains("4 start kits"), "{text}");
        assert!(text.contains("consistency: 5 checks, 0 errors"), "{text}");
        assert!(text.contains("spool-ups"), "{text}");
        let hash = |t: &str| t.split("hash ").nth(1).map(str::to_owned);
        assert_eq!(hash(&text), hash(&run_once(None)), "same seed, same run");
        assert_eq!(
            hash(&text),
            hash(&run_once(Some(1_500))),
            "restore changes nothing"
        );
    }
}
