//! `--combat`: time-to-kill and combat cost scenarios (S5 plan §7 task 7, §8 "TTK" and
//! "Performance"; D-058).
//!
//! Each scenario places two fleets 4 km apart in open space in a stage-1 system (a fixture,
//! `nomad_sim::scenario`, not a game rule), orders each to attack the other on *weapons
//! free*, and runs until one side has no ships left or the time cap passes. The time to kill
//! is measured from the first hit to the last kill, so the approach does not count; the time
//! of the first hit is reported too.
//!
//! - `skirmish`: the D-058 small-group target (6 against 6, 30–90 s) for the counter
//!   pairings, count for count, and a mixed mirror;
//! - `battle`: the fleet-battle target (20 against 20, 3–8 min) for a mirror, and a counter
//!   test: 12/8/0 against 8/4/8, whose sides differ in value and one is counter-loaded, is
//!   not a time-to-kill target (D-284);
//! - `triangle`: the counter triangle (D-052) at equal value, as doc 13 §10 and D-083 test it
//!   (D-284): each pairing reports whether the side the triangle favours wins. Value is
//!   counted in alloy at the rules file's ship costs; equal-command pairings are shown
//!   alongside;
//! - `stress`: 100 against 100, for the tick time of a large battle (doc 15 §13).
//!
//! Every scenario reports the tick time (mean, p99, max) and the most projectiles in
//! flight. The breach and core-kill targets (`breach`, `core`) are in [`crate::station`].

use std::io::Write;
use std::time::Instant;

use nomad_sim::galaxy::{STAGE1_COMMANDERS, generate_stage1_with_rules};
use nomad_sim::scenario::{place_fleet, sense};
use nomad_sim::{
    CombatEvent, Command, CommanderId, CommanderInput, CommanderInputKind, FleetId, FleetOrder,
    Owner, Queueing, Rules, SeasonConfig, ShipClass, TICKS_PER_SECOND, TickInputs, Vec2, step,
};

const COMMANDERS: [CommanderId; STAGE1_COMMANDERS] = [
    CommanderId(1),
    CommanderId(2),
    CommanderId(3),
    CommanderId(4),
];
const VX: ShipClass = ShipClass::Vx;
const LANCER: ShipClass = ShipClass::Lancer;
const WARDEN: ShipClass = ShipClass::Warden;

/// Which scenarios to run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Set {
    /// 6 against 6 (D-058: 30–90 s).
    Skirmish,
    /// 20 against 20 (D-058: 3–8 min).
    Battle,
    /// 100 against 100, for tick time.
    Stress,
    /// The counter triangle at equal value (D-052, D-284).
    Triangle,
    /// Breaching a full mother-station sector (D-058: 5–10 min; S5b).
    Breach,
    /// Killing the core in a siege (D-058: 15–25 min; S5b).
    Core,
}

impl Set {
    /// Parses `skirmish`, `battle`, `triangle`, `stress`, `breach` or `core`.
    pub fn parse(text: &str) -> Result<Self, String> {
        match text {
            "skirmish" => Ok(Self::Skirmish),
            "battle" => Ok(Self::Battle),
            "stress" => Ok(Self::Stress),
            "triangle" => Ok(Self::Triangle),
            "breach" => Ok(Self::Breach),
            "core" => Ok(Self::Core),
            other => Err(format!(
                "--combat takes skirmish, battle, triangle, stress, breach or core, not {other}"
            )),
        }
    }
}

/// One scenario: two compositions and the D-058 target it is measured against.
struct Scenario {
    name: &'static str,
    a: Vec<ShipClass>,
    b: Vec<ShipClass>,
    /// The target range in seconds, if D-058 has one for it.
    target_s: Option<(u64, u64)>,
    /// The S5 plan §5 hand estimate, if there is one.
    estimate: Option<&'static str>,
    /// For a counter test, the side that should win.
    winner: Option<Side>,
    /// Give up after this long.
    cap_s: u64,
}

/// A side of a scenario.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    /// The first fleet.
    A,
    /// The second fleet.
    B,
}

fn repeat(class: ShipClass, n: usize) -> Vec<ShipClass> {
    vec![class; n]
}

fn mixed(vx: usize, lancer: usize, warden: usize) -> Vec<ShipClass> {
    [
        repeat(VX, vx),
        repeat(LANCER, lancer),
        repeat(WARDEN, warden),
    ]
    .concat()
}

fn scenarios(set: Set) -> Vec<Scenario> {
    let skirmish = Some((30, 90));
    let battle = Some((180, 480));
    match set {
        Set::Skirmish => vec![
            Scenario {
                name: "6 VX v 6 VX",
                a: repeat(VX, 6),
                b: repeat(VX, 6),
                target_s: skirmish,
                estimate: Some("about 19 s + 2 s lock (S5 plan §5)"),
                winner: None,
                cap_s: 600,
            },
            Scenario {
                name: "6 VX v 6 LANCER",
                a: repeat(VX, 6),
                b: repeat(LANCER, 6),
                target_s: skirmish,
                estimate: None,
                winner: None,
                cap_s: 600,
            },
            Scenario {
                name: "6 LANCER v 6 WARDEN",
                a: repeat(LANCER, 6),
                b: repeat(WARDEN, 6),
                target_s: skirmish,
                estimate: None,
                winner: None,
                cap_s: 900,
            },
            Scenario {
                name: "6 WARDEN v 6 VX",
                a: repeat(WARDEN, 6),
                b: repeat(VX, 6),
                target_s: skirmish,
                estimate: None,
                winner: None,
                cap_s: 900,
            },
            Scenario {
                name: "2/2/2 v 2/2/2 (VX/LANCER/WARDEN)",
                a: mixed(2, 2, 2),
                b: mixed(2, 2, 2),
                target_s: skirmish,
                estimate: None,
                winner: None,
                cap_s: 900,
            },
        ],
        Set::Battle => vec![
            Scenario {
                name: "8/8/4 v 8/8/4 (VX/LANCER/WARDEN)",
                a: mixed(8, 8, 4),
                b: mixed(8, 8, 4),
                target_s: battle,
                estimate: Some("not estimated by hand (S5 plan §5)"),
                winner: None,
                cap_s: 1_800,
            },
            Scenario {
                name: "12/8/0 v 8/4/8 (VX/LANCER/WARDEN)",
                a: mixed(12, 8, 0),
                b: mixed(8, 4, 8),
                target_s: None,
                estimate: Some("a counter test, not a time-to-kill target (D-284)"),
                winner: Some(Side::B),
                cap_s: 1_800,
            },
        ],
        Set::Triangle => triangle(),
        // The station scenarios are crate::station's.
        Set::Breach | Set::Core => vec![],
        Set::Stress => vec![Scenario {
            name: "40/40/20 v 40/40/20 (VX/LANCER/WARDEN)",
            a: mixed(40, 40, 20),
            b: mixed(40, 40, 20),
            target_s: None,
            estimate: None,
            winner: None,
            cap_s: 1_800,
        }],
    }
}

/// The counter pairings at equal value in alloy (D-284: VX 1,200, LANCER 2,000, WARDEN
/// 4,000 A at the rules file's costs) and, for comparison, at equal command (VX 6, LANCER 10,
/// WARDEN 25).
fn triangle() -> Vec<Scenario> {
    let pair = |name, a, b, winner| Scenario {
        name,
        a,
        b,
        target_s: None,
        estimate: None,
        winner: Some(winner),
        cap_s: 1_200,
    };
    vec![
        pair(
            "value: 10 VX v 6 LANCER",
            repeat(VX, 10),
            repeat(LANCER, 6),
            Side::A,
        ),
        pair(
            "value: 12 LANCER v 6 WARDEN",
            repeat(LANCER, 12),
            repeat(WARDEN, 6),
            Side::A,
        ),
        pair(
            "value: 8 LANCER v 4 WARDEN",
            repeat(LANCER, 8),
            repeat(WARDEN, 4),
            Side::A,
        ),
        pair(
            "value: 6 WARDEN v 20 VX",
            repeat(WARDEN, 6),
            repeat(VX, 20),
            Side::A,
        ),
        pair(
            "command: 10 VX v 6 LANCER",
            repeat(VX, 10),
            repeat(LANCER, 6),
            Side::A,
        ),
        pair(
            "command: 15 LANCER v 6 WARDEN",
            repeat(LANCER, 15),
            repeat(WARDEN, 6),
            Side::A,
        ),
        pair(
            "command: 6 WARDEN v 25 VX",
            repeat(WARDEN, 6),
            repeat(VX, 25),
            Side::A,
        ),
    ]
}

/// What a scenario run measured.
struct Outcome {
    first_hit_tick: Option<u64>,
    end_tick: Option<u64>,
    ticks: u64,
    survivors: (usize, usize),
    peak_projectiles: usize,
    micros: Vec<f64>,
}

fn run_scenario(scenario: &Scenario, seed: u64, rules: &Rules) -> Result<Outcome, String> {
    let mut state =
        generate_stage1_with_rules(SeasonConfig::new(seed, 0), COMMANDERS, rules.clone())
            .map_err(|e| e.to_string())?;
    let site = state
        .holdings()
        .find(|(_, h)| h.owner == COMMANDERS[0])
        .map(|(_, h)| h.site)
        .ok_or("no mother station")?;
    let site = state.galaxy().site(site).ok_or("no site")?;
    let system = site.system;
    let centre = Vec2::from(site.position) + Vec2::new(9_000.0, 0.0);
    let owner = |n: u32| Owner::Commander(CommanderId(n));
    let a = place_fleet(&mut state, owner(1), system, centre, 0.0, &scenario.a);
    let b = place_fleet(
        &mut state,
        owner(2),
        system,
        centre + Vec2::new(4_000.0, 0.0),
        std::f64::consts::PI,
        &scenario.b,
    );
    // Both fleets were there at the last sensors pass, 4 km apart: each sees the other
    // (S7; a VX sees a Light ship at 9 km).
    sense(&mut state);
    let attack = |n: u32, fleet: FleetId, target: FleetId| CommanderInput {
        commander: CommanderId(n),
        client_seq: 1,
        kind: CommanderInputKind::Command(Command::Fleet {
            fleet,
            order: FleetOrder::Attack {
                target: target.sim_id(),
            },
            queue: Queueing::Replace,
            retreat: false,
        }),
    };
    let first = TickInputs {
        operator: vec![],
        commanders: vec![attack(1, a, b), attack(2, b, a)],
    };
    let alive =
        |state: &nomad_sim::UniverseState, f: FleetId| state.fleet(f).map_or(0, |v| v.ships.len());
    let cap = scenario.cap_s * TICKS_PER_SECOND;
    let mut outcome = Outcome {
        first_hit_tick: None,
        end_tick: None,
        ticks: 0,
        survivors: (scenario.a.len(), scenario.b.len()),
        peak_projectiles: 0,
        micros: Vec::with_capacity(usize::try_from(cap).unwrap_or(0)),
    };
    for t in 0..cap {
        let inputs = if t == 0 {
            first.clone()
        } else {
            TickInputs::default()
        };
        let started = Instant::now();
        let out = step(&mut state, &inputs);
        outcome.micros.push(started.elapsed().as_secs_f64() * 1e6);
        if t == 0 && out.commands.iter().any(|c| c.outcome.is_err()) {
            return Err(format!(
                "{}: an attack order was rejected: {:?}",
                scenario.name, out.commands
            ));
        }
        if outcome.first_hit_tick.is_none()
            && out
                .combat_events
                .iter()
                .any(|e| matches!(e, CombatEvent::ShipDamaged { .. }))
        {
            outcome.first_hit_tick = Some(t);
        }
        outcome.peak_projectiles = outcome.peak_projectiles.max(state.projectile_count());
        outcome.ticks = t + 1;
        outcome.survivors = (alive(&state, a), alive(&state, b));
        if outcome.survivors.0 == 0 || outcome.survivors.1 == 0 {
            outcome.end_tick = Some(t);
            break;
        }
    }
    Ok(outcome)
}

/// Runs the scenarios of `set` and writes the report.
pub fn run(out: &mut impl Write, set: Set, seed: u64, rules: &Rules) -> Result<(), String> {
    match set {
        Set::Breach => return crate::station::breach(out, seed, rules),
        Set::Core => return crate::station::core(out, seed, rules),
        Set::Skirmish | Set::Battle | Set::Triangle | Set::Stress => {}
    }
    let w = |out: &mut dyn Write, line: String| writeln!(out, "{line}").map_err(|e| e.to_string());
    w(
        out,
        format!(
            "combat {set:?} | seed {seed} | fleets 4 km apart, Attack each other, weapons free"
        ),
    )?;
    for scenario in scenarios(set) {
        let outcome = run_scenario(&scenario, seed, rules)?;
        let secs = |ticks: u64| ticks as f64 / TICKS_PER_SECOND as f64;
        let ttk = match (outcome.first_hit_tick, outcome.end_tick) {
            (Some(first), Some(end)) => format!("{:.1} s", secs(end - first + 1)),
            (Some(_), None) => format!("unresolved after {:.0} s", secs(outcome.ticks)),
            (None, _) => "no hits".to_owned(),
        };
        let verdict = match (scenario.target_s, outcome.first_hit_tick, outcome.end_tick) {
            (Some((lo, hi)), Some(first), Some(end)) => {
                let s = secs(end - first + 1);
                let (lo, hi) = (lo as f64, hi as f64);
                if s < lo {
                    format!(
                        "{:.0}% under the {lo:.0}–{hi:.0} s target",
                        (1.0 - s / lo) * 100.0
                    )
                } else if s > hi {
                    format!(
                        "{:.0}% over the {lo:.0}–{hi:.0} s target",
                        (s / hi - 1.0) * 100.0
                    )
                } else {
                    format!("inside the {lo:.0}–{hi:.0} s target")
                }
            }
            (Some((lo, hi)), _, _) => format!("target {lo}–{hi} s"),
            (None, _, _) => "no D-058 target".to_owned(),
        };
        let counter = scenario.winner.map(|side| {
            let won = match side {
                Side::A => outcome.survivors.0 > 0 && outcome.survivors.1 == 0,
                Side::B => outcome.survivors.1 > 0 && outcome.survivors.0 == 0,
            };
            if won {
                format!(" | counter holds: {side:?} wins")
            } else {
                format!(" | COUNTER FAILS: {side:?} should win")
            }
        });
        let mut micros = outcome.micros;
        micros.sort_by(f64::total_cmp);
        let percentile = |per_mille: usize| {
            let index = micros.len().saturating_sub(1) * per_mille / 1_000;
            micros.get(index).copied().unwrap_or(0.0)
        };
        let mean = micros.iter().sum::<f64>() / micros.len().max(1) as f64;
        w(
            out,
            format!(
                "{:<38} | first hit {:>5.1} s | TTK {ttk} ({verdict}) | survivors {} v {}{} | \
                 peak projectiles {} | tick mean {mean:.0} µs, p99 {:.0} µs, max {:.0} µs",
                scenario.name,
                outcome.first_hit_tick.map_or(0.0, secs),
                outcome.survivors.0,
                outcome.survivors.1,
                counter.unwrap_or_default(),
                outcome.peak_projectiles,
                percentile(990),
                percentile(1_000),
            ),
        )?;
        if let Some(estimate) = scenario.estimate {
            w(out, format!("{:<38} | hand estimate: {estimate}", ""))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_skirmish_resolves_and_repeats() {
        let rules = Rules::embedded();
        let scenario = scenarios(Set::Skirmish).remove(0);
        let once = || run_scenario(&scenario, 1, &rules).unwrap();
        let a = once();
        let (Some(first), Some(end)) = (a.first_hit_tick, a.end_tick) else {
            panic!("the skirmish did not resolve");
        };
        assert!(first > 0 && end > first);
        assert!(a.survivors.0 == 0 || a.survivors.1 == 0);
        assert!(a.peak_projectiles > 0);
        let b = once();
        assert_eq!(
            (b.first_hit_tick, b.end_tick, b.survivors),
            (a.first_hit_tick, a.end_tick, a.survivors),
            "same seed, same battle"
        );
    }

    #[test]
    fn the_report_names_every_scenario() {
        let mut out = Vec::new();
        run(&mut out, Set::Skirmish, 1, &Rules::embedded()).unwrap();
        let text = String::from_utf8(out).unwrap();
        for scenario in scenarios(Set::Skirmish) {
            assert!(text.contains(scenario.name), "{text}");
        }
        assert!(text.contains("TTK"), "{text}");
        assert!(Set::parse("dome").is_err());
        assert_eq!(Set::parse("breach"), Ok(Set::Breach));
        assert_eq!(Set::parse("core"), Ok(Set::Core));
        assert_eq!(Set::parse("triangle"), Ok(Set::Triangle));
    }
}
