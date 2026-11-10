//! Headless fast-forward harness for `nomad-sim` (Design D-037, doc 11 §7).
//!
//! Runs a season with no graphics and no pacing, then reports the final state hash and the
//! speed relative to real time. This is the seed of the pacing tool and of server catch-up.
//!
//! With `--galaxy` it instead generates the MVP stage-1 sector for the seed and prints a
//! summary of it (Design doc 10 §4.4).
//!
//! With `--fleets` it runs the fleet demo instead (S3 plan §6 task 11): the four stage-1
//! start kits under scripted and seeded random orders, by default for 6 simulated hours,
//! reporting commands, jumps, docking, a consistency check and the tick time.
//!
//! With `--combat skirmish|battle|stress` it runs the time-to-kill scenarios of D-058 (S5
//! plan §7 task 7): two fleets attack each other, and it reports the time to kill against
//! the target and the tick time. `--combat triangle` checks the counter triangle at equal
//! value (D-052, D-284). `--combat breach|core` runs the station scenarios (S5 plan
//! §7 task 13): a mid-size fleet breaches a full mother-station sector, or kills the core
//! through an open sector, measured as D-147 says.
//!
//! With `--windows` it runs the exposure-window scenario (S6 plan §8 "Windows"): the four
//! stage-1 commanders on scripted AI presence for 30 simulated days by default, reporting
//! each window move, choice and lapse of orders, and checking D-155 on every tick.
//!
//! With `--season` it runs a scripted season to its result (S8 plan §7 task 6): scripted
//! claims by the four stage-1 commanders, 14 days by default, `--early-win` to drive an early
//! win, printing the control and standings history and the result, checked against a
//! brute-force recount of the final holdings.
//!
//! With `--season --ai` it runs the α pacing run with real AI instead (D-218; A2 plan §5.4):
//! four Normal AI commanders on their seeded schedules, 7 days by default, reporting D-070's
//! measures as D-218 and D-283 define them, with the same code as `--economy`; `--solo` keeps
//! one commander alone (D-083 item 3). Run `--economy` on the same seed for S4's scripted
//! builder alongside (D-280).
//!
//! With `--siege` it runs the siege scenarios (S6 plan §8): breach, Reinforced, siege, Held
//! and Fallen, capture, the emergency jump of a mother station, protections and peripheral
//! structures, each end to end from scripted commands, with its timeline and checks.
//!
//! With `--fog` it runs the fog-of-war check (S7 plan §6 tasks 8 and 9): scripted fleets of
//! two commanders and an NPC group for a simulated day by default, every observer's picture
//! compared with brute-force ground truth after every sensors pass, the sensors pass's cost
//! at stage 1 and in a stage-2 stress layout, and two fleets finding each other.
//!
//! With `--deputy` it runs the Deputy's scenarios (A1 plan §9): a raid on a holding, SHLD
//! SURGE and FLAK SCREEN, convoy evacuation and a contested gate, rebuilding within the
//! budget, direct control, and a catch-up, each with its checks. `--deputy bench` measures
//! the Deputy's cost per tick and the catch-up speed at stage 1 (D-203), for 6 simulated
//! hours unless `--hours` or `--ticks` says otherwise; build with `--release` for it.
//!
//! With `--raids` it runs NPC raids on a stage-1 Universe (A3 plan §8 task 5; D-225): the
//! four commanders' Deputies, scripted claims, the haven's launches and the raider director,
//! 7 days by default with a catch-up segment, reporting the launch intervals, raids per
//! commander and per day, the 6 h spacing, what raids destroyed or disabled, raider damage
//! to domes, catch-up suppression and raid losses.
//!
//! `--rules PATH` replaces the rules embedded in the build with another rules file, for
//! tuning runs (D-104). The Universe keeps the rules it was created with.
//!
//! ```text
//! nomad-harness [--seed N] [--ticks N] [--diagnostics N] [--mean-interval-secs N] [--restore-at N]
//!               [--rules PATH]
//! nomad-harness --galaxy [--seed N] [--rules PATH]
//! nomad-harness --fleets [--seed N] [--hours N | --ticks N] [--order-interval-secs N]
//!               [--restore-at N] [--rules PATH]
//! nomad-harness --combat skirmish|battle|triangle|stress|breach|core [--seed N] [--rules PATH]
//! nomad-harness --economy [--seed N] [--days N] [--csv PATH] [--rules PATH]
//! nomad-harness --windows [--seed N] [--days N] [--rules PATH]
//! nomad-harness --season [--seed N] [--days N] [--claim-hours N] [--early-win]
//!               [--restore-at N] [--rules PATH]
//! nomad-harness --season --ai [--solo] [--seed N] [--days N] [--trace-hours N] [--csv PATH]
//!               [--rules PATH]
//! nomad-harness --siege [--seed N] [--rules PATH]
//! nomad-harness --fog [--seed N] [--hours N | --ticks N] [--rules PATH]
//! nomad-harness --deputy [bench] [--seed N] [--hours N | --ticks N] [--rules PATH]
//! nomad-harness --raids [--seed N] [--days N] [--claim-hours N] [--catch-up-from H]
//!               [--catch-up-hours N] [--restore-at N] [--rules PATH]
//! ```

mod combat;
mod deputy;
mod economy;
mod fleets;
mod fog;
mod galaxy;
mod pacing;
mod raids;
mod season;
mod season_ai;
mod siege;
mod station;
mod windows;

use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use nomad_sim::snapshot::{decode, encode, state_hash};
use nomad_sim::{
    OperatorInput, Rules, SeasonConfig, TICKS_PER_SECOND, TickInputs, Ticks, UniverseState, step,
};

#[expect(
    clippy::struct_excessive_bools,
    reason = "one flag per mode, as given on the command line"
)]
struct Args {
    seed: u64,
    ticks: Option<u64>,
    hours: u64,
    hours_given: bool,
    order_interval_secs: u64,
    fleets: bool,
    diagnostics: u32,
    mean_interval_secs: u64,
    restore_at: Option<u64>,
    galaxy: bool,
    combat: Option<combat::Set>,
    economy: bool,
    windows: bool,
    siege: bool,
    fog: bool,
    /// `--deputy`, and whether it is `--deputy bench`.
    deputy: Option<bool>,
    days: Option<u64>,
    rules: Option<PathBuf>,
    csv: Option<PathBuf>,
    // S8: `--season`.
    season: bool,
    claim_hours: u64,
    early_win: bool,
    // A3: `--raids`.
    raids: bool,
    catch_up_from: u64,
    catch_up_hours: u64,
    // A2: `--season --ai`.
    ai: bool,
    solo: bool,
    trace_hours: Option<u64>,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            seed: 1,
            ticks: None,
            hours: 6,
            hours_given: false,
            order_interval_secs: 60,
            fleets: false,
            diagnostics: 10_000,
            mean_interval_secs: 10,
            restore_at: None,
            galaxy: false,
            combat: None,
            economy: false,
            windows: false,
            siege: false,
            fog: false,
            deputy: None,
            days: None,
            rules: None,
            csv: None,
            season: false,
            claim_hours: 12,
            early_win: false,
            raids: false,
            catch_up_from: 30,
            catch_up_hours: 6,
            ai: false,
            solo: false,
            trace_hours: None,
        }
    }
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args::default();
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--seed" => args.seed = parse(&value()?)?,
            "--ticks" => args.ticks = Some(parse(&value()?)?),
            "--hours" => {
                args.hours = parse(&value()?)?;
                args.hours_given = true;
            }
            "--order-interval-secs" => args.order_interval_secs = parse(&value()?)?,
            "--fleets" => args.fleets = true,
            "--diagnostics" => args.diagnostics = parse(&value()?)?,
            "--mean-interval-secs" => args.mean_interval_secs = parse(&value()?)?,
            "--restore-at" => args.restore_at = Some(parse(&value()?)?),
            "--galaxy" => args.galaxy = true,
            "--combat" => args.combat = Some(combat::Set::parse(&value()?)?),
            "--economy" => args.economy = true,
            "--windows" => args.windows = true,
            "--siege" => args.siege = true,
            "--fog" => args.fog = true,
            "--deputy" => args.deputy = Some(false),
            "bench" if args.deputy.is_some() => args.deputy = Some(true),
            "--days" => args.days = Some(parse(&value()?)?),
            "--rules" => args.rules = Some(PathBuf::from(value()?)),
            "--csv" => args.csv = Some(PathBuf::from(value()?)),
            "--season" => args.season = true,
            "--claim-hours" => args.claim_hours = parse(&value()?)?,
            "--raids" => args.raids = true,
            "--catch-up-from" => args.catch_up_from = parse(&value()?)?,
            "--catch-up-hours" => args.catch_up_hours = parse(&value()?)?,
            "--early-win" => args.early_win = true,
            "--ai" => args.ai = true,
            "--solo" => args.solo = true,
            "--trace-hours" => args.trace_hours = Some(parse(&value()?)?),
            "--help" | "-h" => {
                return Err(
                    "usage: nomad-harness [--seed N] [--ticks N] [--diagnostics N] \
                            [--mean-interval-secs N] [--restore-at N] [--rules PATH]\n       \
                     nomad-harness --galaxy [--seed N] [--rules PATH]\n       \
                     nomad-harness --fleets [--seed N] [--hours N | --ticks N] \
                            [--order-interval-secs N] [--restore-at N] [--rules PATH]\n       \
                     nomad-harness --combat skirmish|battle|triangle|stress|breach|core [--seed N] \
                            [--rules PATH]\n       \
                     nomad-harness --economy [--seed N] [--days N] [--csv PATH] [--rules PATH]\n       \
                     nomad-harness --windows [--seed N] [--days N] [--rules PATH]\n       \
                     nomad-harness --season [--seed N] [--days N] [--claim-hours N] \
                            [--early-win] [--restore-at N] [--rules PATH]\n       \
                     nomad-harness --season --ai [--solo] [--seed N] [--days N] [--trace-hours N] \
                            [--csv PATH] [--rules PATH]\n       \
                     nomad-harness --siege [--seed N] [--rules PATH]\n       \
                     nomad-harness --fog [--seed N] [--hours N | --ticks N] [--rules PATH]\n       \
                     nomad-harness --deputy [bench] [--seed N] [--hours N | --ticks N] \
                            [--rules PATH]\n       \
                     nomad-harness --raids [--seed N] [--days N] [--claim-hours N] \
                            [--catch-up-from H] [--catch-up-hours N] [--restore-at N] \
                            [--rules PATH]"
                        .to_owned(),
                );
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(args)
}

fn parse<T: std::str::FromStr>(text: &str) -> Result<T, String> {
    text.parse()
        .map_err(|_| format!("not a valid number: {text}"))
}

/// The embedded rules, or the rules file given with `--rules` (D-104).
fn load_rules(path: Option<&PathBuf>) -> Result<Rules, String> {
    let Some(path) = path else {
        return Ok(Rules::embedded());
    };
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read rules file {}: {e}", path.display()))?;
    Rules::from_json(&text).map_err(|e| format!("{}: {e}", path.display()))
}

fn run() -> Result<(), String> {
    let args = parse_args()?;
    let rules = load_rules(args.rules.as_ref())?;
    if args.galaxy {
        return galaxy::print(&mut io::stdout().lock(), args.seed, rules);
    }
    if let Some(set) = args.combat {
        return combat::run(&mut io::stdout().lock(), set, args.seed, &rules);
    }
    if args.economy {
        return economy::run(
            &mut io::stdout().lock(),
            economy::EconomyRun {
                seed: args.seed,
                days: args.days.unwrap_or(7),
                rules,
                csv: args.csv,
            },
        );
    }
    if args.season && args.ai {
        return season_ai::run(&mut io::stdout().lock(), pacing_run(&args, rules));
    }
    if args.season {
        return season::run(
            &mut io::stdout().lock(),
            season::SeasonRun {
                seed: args.seed,
                final_tick: nomad_sim::Tick::ZERO.after(Ticks::from_days(args.days.unwrap_or(14))),
                claim_hours: args.claim_hours,
                early_win: args.early_win,
                restore_at: args.restore_at,
                rules,
            },
        );
    }
    if args.raids {
        return raids::run(&mut io::stdout().lock(), raid_run(&args, rules));
    }
    if args.windows {
        return windows::run(
            &mut io::stdout().lock(),
            windows::WindowRun {
                seed: args.seed,
                days: args.days.unwrap_or(30),
                rules,
            },
        );
    }
    if args.siege {
        return siege::run(
            &mut io::stdout().lock(),
            &siege::SiegeRun {
                seed: args.seed,
                rules,
            },
        );
    }
    if args.fog {
        // A simulated day unless --hours or --ticks says otherwise.
        let hours = if args.hours_given { args.hours } else { 24 };
        return fog::run(
            &mut io::stdout().lock(),
            &fog::FogRun {
                seed: args.seed,
                ticks: args.ticks.unwrap_or(Ticks::from_hours(hours).get()),
                rules,
            },
        );
    }
    if let Some(bench) = args.deputy {
        let run = deputy::DeputyRun {
            seed: args.seed,
            rules,
        };
        let out = &mut io::stdout().lock();
        return if bench {
            let ticks = args.ticks.unwrap_or(Ticks::from_hours(args.hours).get());
            deputy::bench(out, &run, ticks)
        } else {
            deputy::run(out, &run)
        };
    }
    if args.fleets {
        let hours = Ticks::from_hours(args.hours).get();
        return fleets::run(
            &mut io::stdout().lock(),
            fleets::FleetRun {
                seed: args.seed,
                ticks: args.ticks.unwrap_or(hours),
                order_interval_secs: args.order_interval_secs,
                restore_at: args.restore_at,
                rules,
            },
        );
    }
    diagnostics(&args, rules)
}

/// What `--season --ai` runs, from the command line: 7 days unless `--days` says otherwise.
fn pacing_run(args: &Args, rules: Rules) -> season_ai::PacingRun {
    season_ai::PacingRun {
        seed: args.seed,
        days: args.days.unwrap_or(7),
        solo: args.solo,
        rules,
        trace_hours: args.trace_hours,
        csv: args.csv.clone(),
    }
}

/// What `--raids` runs, from the command line: 7 days unless `--days` says otherwise.
fn raid_run(args: &Args, rules: Rules) -> raids::RaidRun {
    raids::RaidRun {
        seed: args.seed,
        days: args.days.unwrap_or(7),
        claim_hours: args.claim_hours,
        catch_up_from: args.catch_up_from,
        catch_up_hours: args.catch_up_hours,
        restore_at: args.restore_at,
        rules,
    }
}

/// The default run: diagnostic timers for `--ticks`, reporting the hash and the speed.
fn diagnostics(args: &Args, rules: Rules) -> Result<(), String> {
    // One hour of Universe time by default.
    let ticks = args.ticks.unwrap_or(36_000);
    let mut state = UniverseState::with_rules(SeasonConfig::new(args.seed, 0), rules)
        .map_err(|e| e.to_string())?;
    let spawn = TickInputs {
        operator: vec![OperatorInput::SpawnDiagnostics {
            count: args.diagnostics,
            mean_interval: Ticks::from_secs(args.mean_interval_secs),
        }],
        commanders: vec![],
    };

    let started = Instant::now();
    let mut fired: u64 = 0;
    for t in 0..ticks {
        if Some(t) == args.restore_at {
            let bytes = encode(&state).map_err(|e| e.to_string())?;
            state = decode(&bytes).map_err(|e| e.to_string())?;
        }
        let inputs = if t == 0 {
            &spawn
        } else {
            &TickInputs::default()
        };
        fired += u64::from(step(&mut state, inputs).timers_fired);
    }
    let elapsed = started.elapsed().as_secs_f64();

    let hash = state_hash(&state).map_err(|e| e.to_string())?;
    let universe_secs = ticks as f64 / TICKS_PER_SECOND as f64;
    let speed = if elapsed > 0.0 {
        universe_secs / elapsed
    } else {
        f64::INFINITY
    };
    writeln!(
        io::stdout(),
        "ticks {} | universe time {:.1} h | timers fired {fired} | hash {hash:016x} | \
         elapsed {elapsed:.3} s | speed {speed:.0}x real time",
        ticks,
        universe_secs / 3_600.0,
    )
    .map_err(|e| e.to_string())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            let _ = writeln!(io::stderr(), "nomad-harness: {message}");
            ExitCode::FAILURE
        }
    }
}
