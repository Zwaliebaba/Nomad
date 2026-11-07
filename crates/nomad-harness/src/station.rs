//! `--combat breach` and `--combat core`: the station time-to-kill scenarios of D-058 (S5 plan
//! §5, §7 task 13), measured as D-147 says.
//!
//! The defender is commander 1's mother station from the stage-1 start kit: four pylons, so
//! every sector is full (150,000), 1,140 MW of supply, and no turrets. The attackers are D-058's
//! mid-size fleet, 10 LANCER and 4 WARDEN of commander 2, placed 6 km out from the station
//! in the direction of the sector they attack (a fixture, `nomad_sim::scenario`, not a game
//! rule). Peripheral modules lie wholly outside the dome on the sector boundaries (D-228), so
//! from any sector's centre direction nothing but the dome lies in their line of fire. Each
//! run ends when its goal is reached or the time cap passes. The time to kill runs from the
//! attackers' first hit on the holding to the goal, so the approach does not count.
//!
//! - `breach`: Attack on the holding until the attacked sector breaches, the east sector
//!   unless named. D-147 measures the D-058 target (5–10 min) with the Deputy's default
//!   ability use: SHLD SURGE on the sector once it falls below 30% (doc 15 §5.3, doc 12
//!   §5.2). The Deputy is A1's, so the harness sends that order itself, as the default trigger
//!   would. Also run without abilities (the S5 plan's 4.7 min hand calculation), with two
//!   turrets added, which the attackers out-range (S5 plan §5), and from the north, which the
//!   solar array blocked before D-228 moved it: the same breach from another side.
//! - `core`: the east sector is opened as S6's siege will (`scenario::open_sector`) and the
//!   fleet attacks the core. D-147 measures the D-058 target (15–25 min) with the defender's
//!   turrets absent. Also run with two turrets on the attackers' side, which the attackers
//!   destroy first, for comparison.
//!
//! Every scenario reports the tick time (mean, p99, max), as the ship scenarios do.

use std::io::Write;
use std::time::Instant;

use nomad_sim::galaxy::{STAGE1_COMMANDERS, generate_stage1_with_rules};
use nomad_sim::scenario::{open_sector, place_fleet, place_module, sense};
use nomad_sim::{
    Command, CommanderId, CommanderInput, CommanderInputKind, DefenceEvent, FleetId, FleetOrder,
    HoldingId, HoldingOrder, ModuleKind, Owner, Queueing, Rules, SeasonConfig, ShipClass, SimId,
    Slot, StationAbility, TICKS_PER_SECOND, TickInputs, UniverseState, Vec2, step,
};

const COMMANDERS: [CommanderId; STAGE1_COMMANDERS] = [
    CommanderId(1),
    CommanderId(2),
    CommanderId(3),
    CommanderId(4),
];
/// The north and east sectors, in pylon slot order N, E, S, W (D-046).
const NORTH: u8 = 0;
const EAST: u8 = 1;

/// The direction of `sector`'s centre from the dome centre, counter-clockwise from east, in
/// the system frame: N, E, S, W (D-046).
fn sector_direction(sector: u8) -> Vec2 {
    let degrees: f64 = match sector {
        0 => 90.0,
        1 => 0.0,
        2 => -90.0,
        _ => 180.0,
    };
    Vec2::from_angle(degrees.to_radians())
}
/// The Deputy's default SHLD SURGE trigger: a sector below 30% (doc 15 §5.3).
const SURGE_BELOW_PERCENT: u64 = 30;

/// What a station scenario attacks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Goal {
    /// Breach the east sector.
    Breach,
    /// Bring the core to 0 through the open east sector.
    Core,
}

/// One station scenario.
struct Scenario {
    name: &'static str,
    goal: Goal,
    /// The sector attacked, from its centre direction.
    sector: u8,
    /// Whether the defender uses SHLD SURGE on the Deputy's default trigger.
    surge: bool,
    /// Turrets added to the station, in these inner slots (D-130).
    turrets: &'static [u8],
    /// The D-058 target in seconds, when this run is the one D-147 measures it with.
    target_s: Option<(u64, u64)>,
    /// The S5 plan §5 hand estimate.
    estimate: Option<&'static str>,
    cap_s: u64,
}

fn scenarios(goal: Goal) -> Vec<Scenario> {
    match goal {
        Goal::Breach => vec![
            Scenario {
                name: "breach, Deputy SHLD SURGE (D-147)",
                goal,
                sector: EAST,
                surge: true,
                turrets: &[],
                target_s: Some((300, 600)),
                estimate: Some("about 9 min with SHLD SURGE below 30% (S5 plan §5)"),
                cap_s: 1_800,
            },
            Scenario {
                name: "breach, no abilities",
                goal,
                sector: EAST,
                surge: false,
                turrets: &[],
                target_s: None,
                estimate: Some("530/s net against 150,000: 4.7 min (S5 plan §5)"),
                cap_s: 1_800,
            },
            Scenario {
                name: "breach, SHLD SURGE and 2 turrets",
                goal,
                sector: EAST,
                surge: true,
                turrets: &[4, 2],
                target_s: None,
                estimate: Some("turrets reach about 3.3 km, strike craft sit about 4 km out"),
                cap_s: 1_800,
            },
            Scenario {
                name: "breach from the north (D-228)",
                goal,
                sector: NORTH,
                surge: true,
                turrets: &[],
                target_s: Some((300, 600)),
                estimate: Some("as from the east: the line of fire is clear of the solar array"),
                cap_s: 1_800,
            },
        ],
        Goal::Core => vec![
            Scenario {
                name: "core, open sector, no turrets (D-147)",
                goal,
                sector: EAST,
                surge: false,
                turrets: &[],
                target_s: Some((900, 1_500)),
                estimate: Some("600/s + 180/s against 600,000: 12.8 min (S5 plan §5)"),
                cap_s: 3_600,
            },
            Scenario {
                name: "core, open sector, 2 turrets first",
                goal,
                sector: EAST,
                surge: false,
                turrets: &[4, 2],
                target_s: None,
                estimate: Some("longer: the turrets on the attackers' side go first"),
                cap_s: 3_600,
            },
        ],
    }
}

/// What a run measured.
struct Outcome {
    first_hit_tick: Option<u64>,
    goal_tick: Option<u64>,
    ticks: u64,
    surges: u32,
    attackers_left: usize,
    micros: Vec<f64>,
}

/// The attacking fleet: D-058's 10 strike craft and 4 frigates.
fn attackers() -> Vec<ShipClass> {
    [vec![ShipClass::Lancer; 10], vec![ShipClass::Warden; 4]].concat()
}

/// The core module of `holding`.
fn core_of(state: &UniverseState, holding: HoldingId) -> Option<SimId> {
    state
        .holding_economy(holding)?
        .slots
        .into_iter()
        .find_map(|(slot, module)| (slot == Slot::Core).then_some(module?.id))
}

fn command(n: u32, seq: u64, command: Command) -> CommanderInput {
    CommanderInput {
        commander: CommanderId(n),
        client_seq: seq,
        kind: CommanderInputKind::Command(command),
    }
}

/// The attackers' orders: for a breach, the holding; for the core, the turrets on their side
/// first and then the core.
fn orders(
    state: &UniverseState,
    scenario: &Scenario,
    fleet: FleetId,
    holding: HoldingId,
    turrets: &[SimId],
) -> Result<Vec<CommanderInput>, String> {
    let targets: Vec<SimId> = match scenario.goal {
        Goal::Breach => vec![holding.sim_id()],
        Goal::Core => {
            let core = core_of(state, holding).ok_or("no core")?;
            turrets.iter().copied().chain([core]).collect()
        }
    };
    Ok((1_u64..)
        .zip(targets)
        .map(|(seq, target)| {
            command(
                2,
                seq,
                Command::Fleet {
                    fleet,
                    order: FleetOrder::Attack { target },
                    queue: if seq == 1 {
                        Queueing::Replace
                    } else {
                        Queueing::Append
                    },
                },
            )
        })
        .collect())
}

/// A scenario's Universe after its first tick: the station with its turrets, the attackers,
/// and their orders, to send on the next tick.
struct Setup {
    state: UniverseState,
    holding: HoldingId,
    fleet: FleetId,
    orders: TickInputs,
}

fn set_up(scenario: &Scenario, seed: u64, rules: &Rules) -> Result<Setup, String> {
    let mut state =
        generate_stage1_with_rules(SeasonConfig::new(seed, 0), COMMANDERS, rules.clone())
            .map_err(|e| e.to_string())?;
    let (holding, site) = state
        .holdings()
        .find(|(_, h)| h.owner == COMMANDERS[0] && h.mother_station)
        .map(|(id, h)| (id, h.site))
        .ok_or("no mother station")?;
    let site = state.galaxy().site(site).ok_or("no site")?;
    let (system, centre) = (site.system, Vec2::from(site.position));
    let mut turrets = Vec::new();
    for &slot in scenario.turrets {
        turrets.push(
            place_module(&mut state, holding, Slot::Inner(slot), ModuleKind::Turret)
                .ok_or("a turret slot is taken")?,
        );
    }
    let direction = sector_direction(scenario.sector);
    let fleet = place_fleet(
        &mut state,
        Owner::Commander(COMMANDERS[1]),
        system,
        centre + direction * 6_000.0,
        (direction * -1.0).angle(),
        &attackers(),
    );
    // The fleet was there at the last sensors pass (S7): the station and it see each other.
    sense(&mut state);
    // A siege opens the attacked sector; the dome comes up with it, or else in the holding's
    // first economy slot.
    step(&mut state, &TickInputs::default());
    if scenario.goal == Goal::Core && !open_sector(&mut state, holding, scenario.sector) {
        return Err("the sector cannot be opened".to_owned());
    }
    let orders = TickInputs {
        operator: vec![],
        commanders: orders(&state, scenario, fleet, holding, &turrets)?,
    };
    Ok(Setup {
        state,
        holding,
        fleet,
        orders,
    })
}

fn run_scenario(scenario: &Scenario, seed: u64, rules: &Rules) -> Result<Outcome, String> {
    let Setup {
        mut state,
        holding,
        fleet,
        orders: first,
    } = set_up(scenario, seed, rules)?;
    let core = core_of(&state, holding).ok_or("no core")?;
    let cap = scenario.cap_s * TICKS_PER_SECOND;
    let mut outcome = Outcome {
        first_hit_tick: None,
        goal_tick: None,
        ticks: 0,
        surges: 0,
        attackers_left: attackers().len(),
        micros: Vec::with_capacity(usize::try_from(cap).unwrap_or(0)),
    };
    let mut seq = 0_u64;
    for t in 0..cap {
        let mut inputs = if t == 0 {
            first.clone()
        } else {
            TickInputs::default()
        };
        if scenario.surge && deputy_surges(&state, holding, scenario.sector) {
            seq += 1;
            inputs.commanders.push(command(
                1,
                seq,
                Command::Holding {
                    holding,
                    order: HoldingOrder::Ability {
                        ability: StationAbility::ShieldSurge {
                            sector: scenario.sector,
                        },
                    },
                },
            ));
        }
        let started = Instant::now();
        let out = step(&mut state, &inputs);
        outcome.micros.push(started.elapsed().as_secs_f64() * 1e6);
        if t == 0 && out.commands.iter().any(|c| c.outcome.is_err()) {
            return Err(format!(
                "{}: an attack order was rejected: {:?}",
                scenario.name, out.commands
            ));
        }
        outcome.surges += u32::try_from(
            out.commands
                .iter()
                .filter(|c| c.commander == COMMANDERS[0] && c.outcome.is_ok())
                .count(),
        )
        .unwrap_or(0);
        let attacker = Owner::Commander(COMMANDERS[1]);
        for event in &out.defence_events {
            match *event {
                DefenceEvent::SectorDamaged { by, .. } | DefenceEvent::ModuleDamaged { by, .. }
                    if by == attacker && outcome.first_hit_tick.is_none() =>
                {
                    outcome.first_hit_tick = Some(t);
                }
                DefenceEvent::SectorBreached { sector, .. }
                    if scenario.goal == Goal::Breach && sector == scenario.sector =>
                {
                    outcome.goal_tick = outcome.goal_tick.or(Some(t));
                }
                DefenceEvent::CoreDown { .. } if scenario.goal == Goal::Core => {
                    outcome.goal_tick = outcome.goal_tick.or(Some(t));
                }
                _ => {}
            }
        }
        outcome.ticks = t + 1;
        outcome.attackers_left = state.fleet(fleet).map_or(0, |f| f.ships.len());
        if outcome.goal_tick.is_some() || outcome.attackers_left == 0 {
            break;
        }
    }
    if scenario.goal == Goal::Core && outcome.goal_tick.is_some() {
        let left = state.module(core).map_or(0, |m| m.hp);
        if left != 0 {
            return Err(format!("{}: core down with {left} hull", scenario.name));
        }
    }
    Ok(outcome)
}

/// Whether the Deputy's default trigger fires SHLD SURGE on `sector` now: below 30% of its
/// maximum, the ability ready on the tick the order would apply (doc 15 §5.3).
fn deputy_surges(state: &UniverseState, holding: HoldingId, sector: u8) -> bool {
    let Some(view) = state.defence(holding) else {
        return false;
    };
    let sector = view.sectors[usize::from(sector)];
    u128::from(sector.strength_milli) * 100
        < u128::from(sector.max_milli) * u128::from(SURGE_BELOW_PERCENT)
        && view.surge_ready_at <= state.tick().next()
        && !sector.open
}

/// Runs the scenarios of `goal` and writes the report.
fn run_goal(out: &mut dyn Write, goal: Goal, seed: u64, rules: &Rules) -> Result<(), String> {
    let w = |out: &mut dyn Write, line: String| writeln!(out, "{line}").map_err(|e| e.to_string());
    w(
        out,
        format!(
            "combat {goal:?} | seed {seed} | 10 LANCER + 4 WARDEN from 6 km out (east unless \
             named) of a full mother station"
        ),
    )?;
    for scenario in scenarios(goal) {
        let outcome = run_scenario(&scenario, seed, rules)?;
        let secs = |ticks: u64| ticks as f64 / TICKS_PER_SECOND as f64;
        let ttk_s = match (outcome.first_hit_tick, outcome.goal_tick) {
            (Some(first), Some(end)) => Some(secs(end - first + 1)),
            _ => None,
        };
        let ttk = match (ttk_s, outcome.first_hit_tick) {
            (Some(s), _) => format!("{s:.1} s ({:.1} min)", s / 60.0),
            (None, Some(_)) => format!("unresolved after {:.0} s", secs(outcome.ticks)),
            (None, None) => "no hits".to_owned(),
        };
        let verdict = match (scenario.target_s, ttk_s) {
            (Some((lo, hi)), Some(s)) => {
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
            (Some((lo, hi)), None) => format!("target {lo}–{hi} s"),
            (None, _) => "comparison, no D-058 target".to_owned(),
        };
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
                "{:<38} | first hit {:>5.1} s | TTK {ttk} ({verdict}) | surges {} | \
                 attackers left {} of {} | tick mean {mean:.0} µs, p99 {:.0} µs, max {:.0} µs",
                scenario.name,
                outcome.first_hit_tick.map_or(0.0, secs),
                outcome.surges,
                outcome.attackers_left,
                attackers().len(),
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

/// `--combat breach`.
pub fn breach(out: &mut dyn Write, seed: u64, rules: &Rules) -> Result<(), String> {
    run_goal(out, Goal::Breach, seed, rules)
}

/// `--combat core`.
pub fn core(out: &mut dyn Write, seed: u64, rules: &Rules) -> Result<(), String> {
    run_goal(out, Goal::Core, seed, rules)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "long simulation: a breach takes minutes of ticks; CI runs fast tests only (D-190), run with --ignored"]
    fn the_d147_breach_resolves_and_repeats() {
        let rules = Rules::embedded();
        let scenario = scenarios(Goal::Breach).remove(0);
        let once = || run_scenario(&scenario, 1, &rules).unwrap();
        let a = once();
        let (Some(first), Some(end)) = (a.first_hit_tick, a.goal_tick) else {
            panic!("the sector did not breach");
        };
        assert!(first > 0 && end > first);
        assert!(a.surges > 0, "the Deputy's trigger used SHLD SURGE");
        let b = once();
        assert_eq!(
            (b.first_hit_tick, b.goal_tick, b.surges),
            (a.first_hit_tick, a.goal_tick, a.surges),
            "same seed, same siege"
        );
    }

    #[test]
    #[ignore = "long simulation: a core kill takes many minutes of ticks; CI runs fast tests only (D-190), run with --ignored"]
    fn the_d147_core_kill_resolves() {
        let rules = Rules::embedded();
        let scenario = scenarios(Goal::Core).remove(0);
        let outcome = run_scenario(&scenario, 1, &rules).unwrap();
        assert!(outcome.goal_tick.is_some(), "the core went down");
        assert_eq!(outcome.attackers_left, attackers().len(), "no turrets");
    }

    #[test]
    fn every_scenario_sets_up() {
        // The station accepts the turrets and the orders of every scenario (one tick each).
        let rules = Rules::embedded();
        for goal in [Goal::Breach, Goal::Core] {
            for mut scenario in scenarios(goal) {
                scenario.cap_s = 1;
                let outcome = run_scenario(&scenario, 1, &rules).unwrap();
                assert_eq!(outcome.ticks, TICKS_PER_SECOND, "{}", scenario.name);
            }
        }
    }
}
