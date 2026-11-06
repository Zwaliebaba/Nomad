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
//! the target and the tick time.
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
//! nomad-harness --combat skirmish|battle|stress [--seed N] [--rules PATH]
//! nomad-harness --economy [--seed N] [--days N] [--rules PATH]
//! ```

mod combat;
mod economy;
mod fleets;
mod galaxy;

use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use nomad_sim::snapshot::{decode, encode, state_hash};
use nomad_sim::{
    OperatorInput, Rules, SeasonConfig, TICKS_PER_SECOND, TickInputs, Ticks, UniverseState, step,
};

struct Args {
    seed: u64,
    ticks: Option<u64>,
    hours: u64,
    order_interval_secs: u64,
    fleets: bool,
    diagnostics: u32,
    mean_interval_secs: u64,
    restore_at: Option<u64>,
    galaxy: bool,
    combat: Option<combat::Set>,
    economy: bool,
    days: u64,
    rules: Option<PathBuf>,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            seed: 1,
            ticks: None,
            hours: 6,
            order_interval_secs: 60,
            fleets: false,
            diagnostics: 10_000,
            mean_interval_secs: 10,
            restore_at: None,
            galaxy: false,
            combat: None,
            economy: false,
            days: 7,
            rules: None,
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
            "--hours" => args.hours = parse(&value()?)?,
            "--order-interval-secs" => args.order_interval_secs = parse(&value()?)?,
            "--fleets" => args.fleets = true,
            "--diagnostics" => args.diagnostics = parse(&value()?)?,
            "--mean-interval-secs" => args.mean_interval_secs = parse(&value()?)?,
            "--restore-at" => args.restore_at = Some(parse(&value()?)?),
            "--galaxy" => args.galaxy = true,
            "--combat" => args.combat = Some(combat::Set::parse(&value()?)?),
            "--economy" => args.economy = true,
            "--days" => args.days = parse(&value()?)?,
            "--rules" => args.rules = Some(PathBuf::from(value()?)),
            "--help" | "-h" => {
                return Err(
                    "usage: nomad-harness [--seed N] [--ticks N] [--diagnostics N] \
                            [--mean-interval-secs N] [--restore-at N] [--rules PATH]\n       \
                     nomad-harness --galaxy [--seed N] [--rules PATH]\n       \
                     nomad-harness --fleets [--seed N] [--hours N | --ticks N] \
                            [--order-interval-secs N] [--restore-at N] [--rules PATH]\n       \
                     nomad-harness --combat skirmish|battle|stress [--seed N] [--rules PATH]\n       \
                     nomad-harness --economy [--seed N] [--days N] [--rules PATH]"
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
                days: args.days,
                rules,
            },
        );
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
    // One hour of Universe time by default.
    let ticks = args.ticks.unwrap_or(36_000);
    let mut state = UniverseState::with_rules(
        SeasonConfig {
            seed: args.seed,
            season_start_unix_ms: 0,
        },
        rules,
    )
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
