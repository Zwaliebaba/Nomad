//! `--season --ai`: the α pacing run with real AI (D-218, D-280; A2 plan §5.4, §9).
//!
//! The four stage-1 commanders are Normal AI commanders on their seeded schedules (D-080,
//! D-204): their Strategists play while online, their Deputies always (A1, A2; A3's raiders
//! are not built yet). `--solo` keeps commander 1 alone: the other three are pinned offline
//! with `SetPresence`, the "unopposed" run of D-083 item 3. The season starts on a Monday at
//! 00:00 UTC, so weekends fall on days 5–6, 12–13 and so on, and lasts the default 8 weeks
//! (D-106) however many days the run simulates.
//!
//! It reports D-070's measures as D-218 §5.4 and D-283 define them, computed by
//! [`crate::pacing`] exactly as `--economy` computes them for S4's scripted builder, so the
//! two runs on the same seed compare line by line (D-280). The first expansion counts from
//! the commander's first online minute (Q219). Alongside, per commander: its schedule, its
//! holdings by day, D-083 item 2 (the least share of a day's income spent, or the days at
//! the cap), the Strategist's statistics, its evaluation time on real states (D-083 item 7,
//! D-203) and the per-layer command counters (A1 plan §4.8). `--csv PATH` writes the hourly
//! samples in `--economy`'s format; `--trace-hours N` prints each commander's plans, pool
//! and queues every N hours.
//!
//! *Proposed* readings:
//! - a remote holding is built out (D-283) when every pylon and turret of its blueprint is
//!   complete, skipping a slot that holds another module, as the Strategist skips it: the
//!   Strategist builds a holding's producers before its defence (D-215; A2 plan §4.5), so
//!   that ends its blueprint bar the one forward dry dock, storage (which Logistics and the
//!   kit gate place) and producers worth nothing under D-217;
//! - a day's spending is its production less what the pool gained and what was lost.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;

use crate::pacing;
use nomad_sim::galaxy::{DepositKind, STAGE1_COMMANDERS, generate_stage1_roster};
use nomad_sim::snapshot::state_hash;
use nomad_sim::{
    BuildItem, CommanderId, CommanderSetup, Difficulty, EconomyEvent, HoldingId, ModuleKind,
    OperatorInput, Owner, Rules, SeasonConfig, SiteKind, TICKS_PER_SECOND, Tick, TickInputs, Ticks,
    UniverseState, step,
};

const COMMANDERS: [CommanderId; STAGE1_COMMANDERS] = [
    CommanderId(1),
    CommanderId(2),
    CommanderId(3),
    CommanderId(4),
];
/// 2026-11-09, a Monday, 00:00 UTC.
const MONDAY: i64 = 1_794_182_400_000;
const HOUR: u64 = 3_600 * TICKS_PER_SECOND;
const DAY: u64 = 24 * HOUR;

/// What `--season --ai` runs.
pub struct PacingRun {
    /// The season seed.
    pub seed: u64,
    /// Simulated days.
    pub days: u64,
    /// Commander 1 alone.
    pub solo: bool,
    /// The rules (D-104).
    pub rules: Rules,
    /// Print each commander's plans, pool and queues every this many hours.
    pub trace_hours: Option<u64>,
    /// Where to write the hourly samples as CSV, if anywhere.
    pub csv: Option<PathBuf>,
}

/// One trace line per commander: plans, pool, kit price and queues.
fn trace(
    out: &mut impl Write,
    state: &UniverseState,
    players: &[CommanderId],
    now: u64,
) -> Result<(), String> {
    for &c in players {
        let e = state.economy_of(c);
        let goals = state
            .ai_memory()
            .commander(c)
            .map(|m| m.strategist.describe())
            .unwrap_or_default();
        let queues: Vec<String> = e
            .holdings
            .iter()
            .filter(|h| !h.queue.is_empty())
            .map(|h| {
                let items: Vec<String> = h
                    .queue
                    .iter()
                    .map(|q| {
                        format!(
                            "{:?}{}",
                            q.item,
                            if q.building.is_some() { "*" } else { "" }
                        )
                    })
                    .collect();
                format!("{:?}: {}", h.holding.sim_id(), items.join(", "))
            })
            .collect();
        let fleets: Vec<String> = state
            .fleet_ids()
            .into_iter()
            .filter_map(|f| state.fleet(f))
            .filter(|f| f.owner == Owner::Commander(c))
            .map(|f| {
                let kit = f.ships.iter().any(|&s| state.cargo(s).core_kit);
                format!(
                    "{}@{:?}{}{}{}",
                    f.id.sim_id().get(),
                    f.place,
                    if kit { " kit" } else { "" },
                    if f.order.is_some() { " busy" } else { "" },
                    if f.ships.len() > 1 {
                        format!(" x{}", f.ships.len())
                    } else {
                        String::new()
                    }
                )
            })
            .collect();
        let candidates: Vec<String> = state
            .expansion_candidates(c, 4)
            .into_iter()
            .map(|(site, score)| {
                let (system, kinds) = state.galaxy().site(site).map_or_else(
                    || (0, String::new()),
                    |s| {
                        let kinds: String = s
                            .deposits
                            .iter()
                            .map(|d| match d.kind {
                                DepositKind::OreField => 'O',
                                DepositKind::CrystalVein => 'V',
                            })
                            .collect();
                        (s.system.0.get(), kinds)
                    },
                );
                format!("{}/{system}{kinds}:{score}", site.0.get())
            })
            .collect();
        writeln!(
            out,
            "  [{:.1} h] C{} online {} | plans {goals} | candidates {} | pool {:.0} A {:.0} C / cap {:.0} A {:.0} C | kit {:.0} A {:.0} C | command {}/{} | queues {} | fleets {}",
            hours(now),
            c.0,
            state.commander(c).is_some_and(|r| r.presence.is_online()),
            candidates.join(" "),
            units(e.pool.alloy),
            units(e.pool.crystal),
            units(e.cap.alloy),
            units(e.cap.crystal),
            units(e.core_kit_price.alloy),
            units(e.core_kit_price.crystal),
            e.command_used,
            e.command_cap,
            queues.join(" | "),
            fleets.join(", ")
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// What the run records of one commander.
#[derive(Default)]
struct Record {
    first_online: Option<u64>,
    first_kit: Option<u64>,
    completions: Vec<u64>,
    completed_at: BTreeMap<HoldingId, u64>,
    built_out: BTreeMap<HoldingId, u64>,
    samples: Vec<pacing::Sample>,
    /// The produced, lost and pool totals at the start of the day.
    day_start: Option<(nomad_sim::Stock, nomad_sim::Stock, nomad_sim::Stock)>,
    least_spent: Option<f64>,
    days_at_cap: u32,
}

fn hours(ticks: u64) -> f64 {
    ticks as f64 / HOUR as f64
}

fn units(milli: u64) -> f64 {
    milli as f64 / 1_000.0
}

/// Whether a holding's blueprint's pylons and turrets are complete (D-283; the reading in
/// the module documentation).
fn built_out(state: &UniverseState, holding: HoldingId) -> bool {
    let (Some(h), Some(view)) = (state.holding(holding), state.holding_economy(holding)) else {
        return false;
    };
    let deposits = state
        .galaxy()
        .site(h.site)
        .map(|s| s.deposits.clone())
        .unwrap_or_default();
    let ore = deposits
        .iter()
        .filter(|d| d.kind == DepositKind::OreField)
        .count();
    let crystal = deposits.len() - ore;
    let kind = if h.mother_station {
        SiteKind::Mother
    } else {
        SiteKind::from_deposits(ore, crystal)
    };
    let ai = &state.rules().ai;
    let blueprint = ai
        .strategist
        .blueprint(kind, ai.difficulty(Difficulty::Normal).blueprints);
    blueprint
        .steps
        .iter()
        .filter(|s| matches!(s.kind, ModuleKind::Pylon | ModuleKind::Turret))
        .all(|s| {
            match view
                .slots
                .iter()
                .find(|(slot, _)| *slot == s.slot)
                .map(|(_, m)| m)
            {
                // The holding has no such slot: nothing to build.
                None => true,
                Some(None) => false,
                Some(Some(m)) => m.kind != s.kind || m.building_ms.is_none(),
            }
        })
}

/// Runs `--season --ai` and prints the report.
#[expect(
    clippy::too_many_lines,
    reason = "the run loop and its report read best in one place"
)]
pub fn run(out: &mut impl Write, cfg: PacingRun) -> Result<(), String> {
    let roster = COMMANDERS.map(|id| CommanderSetup::scheduled_ai(id, Difficulty::Normal));
    // The season is the default 8 weeks (D-106) whatever the run's length, so a short run
    // sees the AI play the season's start, not its end (the payback horizon, D-213).
    let mut season = SeasonConfig::new(cfg.seed, MONDAY);
    season.final_tick = season
        .final_tick
        .max(Tick::ZERO.after(Ticks::from_days(cfg.days)));
    let mut state =
        generate_stage1_roster(season, roster, cfg.rules).map_err(|e| format!("{e:?}"))?;
    let start_cap = state.pool_cap(COMMANDERS[0]);
    let players: Vec<CommanderId> = if cfg.solo {
        vec![COMMANDERS[0]]
    } else {
        COMMANDERS.to_vec()
    };
    let mut records: BTreeMap<CommanderId, Record> =
        players.iter().map(|&c| (c, Record::default())).collect();
    let mut csv = match &cfg.csv {
        Some(path) => {
            let mut file =
                std::io::BufWriter::new(std::fs::File::create(path).map_err(|e| e.to_string())?);
            writeln!(file, "{}", pacing::CSV_HEADER).map_err(|e| e.to_string())?;
            Some(file)
        }
        None => None,
    };
    let ticks = Ticks::from_days(cfg.days).get();
    let mut evaluation_us: Vec<u128> = Vec::new();
    let started = Instant::now();
    for t in 0..ticks {
        let inputs = if t == 0 && cfg.solo {
            TickInputs {
                operator: COMMANDERS[1..]
                    .iter()
                    .map(|&commander| OperatorInput::SetPresence {
                        commander,
                        online: false,
                    })
                    .collect(),
                commanders: vec![],
            }
        } else {
            TickInputs::default()
        };
        // Time a Strategist evaluation on the real state every simulated hour (D-203).
        if t % HOUR == HOUR / 2 {
            for &c in &players {
                if state.commander(c).is_some_and(|r| r.presence.is_online()) {
                    let at = Instant::now();
                    let _ = state.trial_strategist(c);
                    evaluation_us.push(at.elapsed().as_micros());
                }
            }
        }
        let outputs = step(&mut state, &inputs);
        let now = outputs.tick.get();
        for event in &outputs.economy_events {
            match *event {
                EconomyEvent::ItemStarted { holding, item } => {
                    let Some(owner) = state.holding(holding).map(|h| h.owner) else {
                        continue;
                    };
                    let kit = state.holding_economy(holding).is_some_and(|v| {
                        v.queue
                            .iter()
                            .any(|q| q.id == item && q.item == BuildItem::CoreKit)
                    });
                    if kit
                        && let Some(r) = records.get_mut(&owner)
                        && r.first_kit.is_none()
                    {
                        r.first_kit = Some(now);
                    }
                }
                EconomyEvent::HoldingCompleted { holding } => {
                    if let Some(owner) = state.holding(holding).map(|h| h.owner)
                        && let Some(r) = records.get_mut(&owner)
                    {
                        r.completions.push(now);
                        r.completed_at.entry(holding).or_insert(now);
                    }
                }
                _ => {}
            }
        }
        if now.is_multiple_of(60 * TICKS_PER_SECOND) {
            for (&c, r) in &mut records {
                if r.first_online.is_none()
                    && state.commander(c).is_some_and(|x| x.presence.is_online())
                {
                    r.first_online = Some(now);
                }
                let pending: Vec<HoldingId> = r
                    .completed_at
                    .keys()
                    .filter(|h| !r.built_out.contains_key(h))
                    .copied()
                    .collect();
                for h in pending {
                    if state.holding(h).is_some_and(|x| x.owner == c) && built_out(&state, h) {
                        r.built_out.insert(h, now);
                    }
                }
            }
        }
        if now.is_multiple_of(HOUR) {
            for (&c, r) in &mut records {
                let e = state.economy_of(c);
                let complete = e
                    .holdings
                    .iter()
                    .filter(|h| state.holding(h.holding).is_some_and(|x| x.core_complete))
                    .count();
                let sample = pacing::Sample {
                    hour: now / HOUR,
                    produced: e.produced,
                    cap: e.cap,
                    command_cap: e.command_cap,
                    holdings: u32::try_from(complete).unwrap_or(u32::MAX),
                };
                if let Some(file) = csv.as_mut() {
                    pacing::write_csv_row(file, &sample, c, &e)?;
                }
                r.samples.push(sample);
            }
        }
        if let Some(every) = cfg.trace_hours
            && now.is_multiple_of(HOUR * every.max(1))
        {
            trace(out, &state, &players, now)?;
        }
        if now.is_multiple_of(DAY) {
            for (&c, r) in &mut records {
                let e = state.economy_of(c);
                if let Some((produced, lost, pool)) = r.day_start {
                    let made = e.produced.minus(produced).alloy;
                    let gained = e.pool.alloy.saturating_sub(pool.alloy);
                    let wasted = e.lost.minus(lost).alloy;
                    if made > 0 {
                        let spent = made.saturating_sub(gained + wasted) as f64 / made as f64;
                        if e.pool.alloy >= e.cap.alloy {
                            r.days_at_cap += 1;
                        } else {
                            r.least_spent =
                                Some(r.least_spent.map_or(spent, |l: f64| l.min(spent)));
                        }
                    }
                }
                r.day_start = Some((e.produced, e.lost, e.pool));
            }
        }
    }
    let elapsed = started.elapsed().as_secs_f64();

    let run_h = ticks / HOUR;
    let per_command = pacing::garrison_per_command(&state.rules().economy);
    let mid = pacing::Window::days(21, 35, run_h);
    let late = pacing::Window::days(42, 56, run_h);
    writeln!(
        out,
        "season --ai | seed {} | {} | {} days ({ticks} ticks) | Normal AI on seeded schedules",
        cfg.seed,
        if cfg.solo {
            "solo: commander 1 alone"
        } else {
            "4 AI commanders"
        },
        cfg.days
    )
    .map_err(|e| e.to_string())?;
    let mut m = pacing::Measures::default();
    for (&c, r) in &records {
        let e = state.economy_of(c);
        let stats = state.strategist_stats(c).unwrap_or_default();
        let schedule = state
            .schedule(c)
            .map(|s| {
                s.weekday
                    .iter()
                    .map(|x| {
                        format!(
                            "{:02}:{:02}+{}m",
                            x.start_min / 60,
                            x.start_min % 60,
                            x.length_min
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default();
        let completions: Vec<f64> = r.completions.iter().map(|&t| hours(t)).collect();
        let build_outs: Vec<Option<f64>> = r
            .completed_at
            .iter()
            .filter(|(h, _)| state.holding(**h).is_some_and(|x| !x.mother_station))
            .map(|(h, &done)| r.built_out.get(h).map(|&built| hours(built - done)))
            .collect();
        let first_kit = match (r.first_kit, r.first_online) {
            (Some(k), Some(o)) => Some(hours(k.saturating_sub(o))),
            _ => None,
        };
        m.add(
            &pacing::CommanderRun {
                samples: &r.samples,
                start_cap,
                completions: &completions,
                first_kit,
                build_outs: &build_outs,
            },
            per_command,
            mid,
            late,
        );
        let by_day: Vec<String> = (1..=cfg.days)
            .filter_map(|d| {
                r.samples
                    .iter()
                    .find(|s| s.hour == d * 24)
                    .map(|s| s.holdings.to_string())
            })
            .collect();
        writeln!(
            out,
            "  C{}: schedule {schedule} | first online {} | first kit {} | holdings by day [{}] | income {:.0} A/h {:.0} C/h | D-281 yield {}‰ over {} alloy, {}‰ over {} crystal producers | pool {:.0}/{:.0} A | command {}/{} | lost {:.0} A | least day spent {} ({} days at cap) | Strategist: {} evaluations, {} plans, {} abandoned, {} deployed, {} sign-offs",
            c.0,
            r.first_online
                .map_or("never".to_owned(), |t| format!("{:.2} h", hours(t))),
            r.first_kit
                .map_or("never".to_owned(), |t| format!("{:.2} h", hours(t))),
            by_day.join(", "),
            units(e.income_last_hour.alloy),
            units(e.income_last_hour.crystal),
            e.yield_permille.alloy,
            e.producing_holdings.alloy,
            e.yield_permille.crystal,
            e.producing_holdings.crystal,
            units(e.pool.alloy),
            units(e.cap.alloy),
            e.command_used,
            e.command_cap,
            units(e.lost.alloy),
            r.least_spent
                .map_or("n/a".to_owned(), |s| format!("{:.0}%", s * 100.0)),
            r.days_at_cap,
            stats.evaluations,
            stats.plans_started,
            stats.plans_abandoned,
            stats.deployed,
            stats.sign_offs,
        )
        .map_err(|e| e.to_string())?;
    }
    m.write(out, mid, late)?;
    evaluation_us.sort_unstable();
    let eval_mean = evaluation_us.iter().sum::<u128>() as f64 / evaluation_us.len().max(1) as f64;
    let eval_p99 = evaluation_us
        .get(evaluation_us.len() * 99 / 100)
        .copied()
        .unwrap_or(0);
    let mut counters = String::new();
    for (&(owner, layer), c) in state.ai_counters() {
        if let Owner::Commander(id) = owner {
            let _ = write!(
                counters,
                " C{}/{layer:?} {}/{}/{}/{}",
                id.0,
                c.issued,
                c.applied,
                c.rejected_total(),
                c.dropped_total()
            );
        }
    }
    writeln!(
        out,
        "Strategist evaluation (D-083 item 7, <= 5 ms mean in release): mean {eval_mean:.0} µs, p99 {eval_p99} µs over {} | AI commands issued/applied/rejected/dropped:{counters}",
        evaluation_us.len()
    )
    .map_err(|e| e.to_string())?;
    let hash = state_hash(&state).map_err(|e| e.to_string())?;
    writeln!(
        out,
        "problems {} | elapsed {elapsed:.1} s | hash {hash:016x}",
        state.economy_problems().len() + state.fleet_problems().len()
    )
    .map_err(|e| e.to_string())?;
    if let Some(mut file) = csv {
        file.flush().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "long simulation: a day of a stage-1 Universe with AI; CI runs fast tests only (D-190), run with --ignored"]
    fn a_day_with_real_ai_claims_and_reports() {
        let mut text = Vec::new();
        run(
            &mut text,
            PacingRun {
                seed: 1,
                days: 1,
                solo: false,
                rules: Rules::embedded(),
                trace_hours: None,
                csv: None,
            },
        )
        .unwrap();
        let text = String::from_utf8(text).unwrap();
        assert!(text.contains("D-070 measures"), "{text}");
        assert!(text.contains("problems 0"), "{text}");
    }
}
