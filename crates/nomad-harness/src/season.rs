//! `--season`: a scripted season run to its result (S8 plan §7 task 6, §8 "Harness"; D-020,
//! D-041, D-116, D-191 to D-198).
//!
//! Until A2's Strategist expands for real, a script claims for the four stage-1 commanders:
//! every `--claim-hours` (default 12) each deploys an outpost on a free site
//! ([`nomad_sim::scenario::place_outpost`]: the claim without the MULE trip, which completes
//! after the rules' 2 h deployment, D-054). It picks the system where it is closest to a
//! majority that it can still reach, nearest its home first. The order of the commanders
//! rotates every round, so nobody claims first every time. With `--early-win`, commander 2
//! claims every hour instead, which drives an early win (D-192). The season's final tick is
//! `--days` after its start (default 14; a real season lasts 8 weeks, D-106).
//!
//! It prints every control change, standings change and near-win warning as it happens, then
//! the result and the final standings, and checks them against a brute-force recount of the
//! final holdings: control, each commander's systems, holdings and deposit value, *since*
//! from the history of changes, the ranks and the winners. It also checks the simulation's
//! own cache against its recount ([`UniverseState::season_problems`]). A failed check is an
//! error. `--restore-at N` restores the Universe from a snapshot at tick N, which must not
//! change the result or the hash.

use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Write;
use std::time::Instant;

use nomad_sim::galaxy::{
    DepositValue, STAGE1_COMMANDERS, generate_stage1_with_rules, is_claimable,
};
use nomad_sim::snapshot::{decode, encode, state_hash};
use nomad_sim::{
    AnnouncementKind, CommanderId, ControlState, Rules, SeasonConfig, SeasonEndReason, SiteId,
    Standing, SystemId, TICKS_PER_SECOND, Tick, TickInputs, UniverseState, scenario, step,
};

const COMMANDERS: [CommanderId; STAGE1_COMMANDERS] = [
    CommanderId(1),
    CommanderId(2),
    CommanderId(3),
    CommanderId(4),
];
const HOUR: u64 = 3_600 * TICKS_PER_SECOND;
const DAY: u64 = 24 * HOUR;

/// The commander that expands fast with `--early-win`.
const EXPANDER: CommanderId = CommanderId(2);

/// What `--season` runs.
pub struct SeasonRun {
    /// The season seed.
    pub seed: u64,
    /// The season's scheduled final tick (D-195).
    pub final_tick: Tick,
    /// Hours between one commander's claims.
    pub claim_hours: u64,
    /// Whether commander 2 claims every hour.
    pub early_win: bool,
    /// Restore from a snapshot at this tick.
    pub restore_at: Option<u64>,
    /// The rules (D-104).
    pub rules: Rules,
}

fn when(tick: Tick) -> String {
    let t = tick.get();
    format!(
        "day {:>2} {:02}:{:02}",
        t / DAY,
        t % DAY / HOUR,
        t % HOUR / (60 * TICKS_PER_SECOND)
    )
}

/// The free site `commander` claims next: in the claimable system where it holds the most
/// sites, completed or not, and can still reach a majority, nearest its home first; `None`
/// if there is none.
fn next_claim(state: &UniverseState, commander: CommanderId) -> Option<SiteId> {
    let home = state
        .holdings()
        .find(|(_, h)| h.owner == commander && h.mother_station)
        .and_then(|(_, h)| state.galaxy().site(h.site))
        .map(|s| s.system)?;
    let jumps = state.galaxy().jumps_from(home);
    let mut best: Option<(Reverse<usize>, u32, SiteId)> = None;
    for (system_id, system) in state.galaxy().systems() {
        if !is_claimable(system.kind) {
            continue;
        }
        let mut own = 0;
        let mut free = Vec::new();
        for &site in &system.sites {
            match state.holding_at(site) {
                Some((_, h)) if h.owner == commander => own += 1,
                Some(_) => {}
                None => free.push(site),
            }
        }
        let majority = system.sites.len() / 2 + 1;
        let Some(&site) = free.first() else {
            continue;
        };
        if own >= majority || own + free.len() < majority {
            continue;
        }
        let key = (
            Reverse(own),
            jumps.get(&system_id).copied().unwrap_or(u32::MAX),
            site,
        );
        if best.is_none_or(|k| key < k) {
            best = Some(key);
        }
    }
    best.map(|(_, _, site)| site)
}

/// What a brute-force recount of the holdings gives one commander.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Recount {
    systems: u32,
    holdings: u32,
    deposit_value: DepositValue,
}

/// Control and every commander's systems, holdings and deposit value, counted from the
/// holdings alone (D-041, D-066), without the simulation's season code.
fn recount(
    state: &UniverseState,
) -> (
    BTreeMap<SystemId, ControlState>,
    BTreeMap<CommanderId, Recount>,
) {
    let mut counts: BTreeMap<CommanderId, Recount> = COMMANDERS
        .iter()
        .map(|&c| (c, Recount::default()))
        .collect();
    let mut per_system: BTreeMap<SystemId, BTreeMap<CommanderId, usize>> = BTreeMap::new();
    for (_, h) in state.holdings() {
        if !h.core_complete {
            continue;
        }
        let entry = counts.entry(h.owner).or_default();
        entry.holdings += 1;
        entry.deposit_value =
            entry.deposit_value + state.galaxy().site_value(h.site).unwrap_or_default();
        if let Some(site) = state.galaxy().site(h.site) {
            *per_system
                .entry(site.system)
                .or_default()
                .entry(h.owner)
                .or_default() += 1;
        }
    }
    let mut control = BTreeMap::new();
    for (id, system) in state.galaxy().systems() {
        if !is_claimable(system.kind) {
            continue;
        }
        let owners = per_system.get(&id).cloned().unwrap_or_default();
        let controller = owners
            .iter()
            .find(|&(_, &n)| 2 * n > system.sites.len())
            .map(|(&c, _)| c);
        let state = match controller {
            Some(c) => {
                counts.entry(c).or_default().systems += 1;
                ControlState::Controlled(c)
            }
            None if owners.values().filter(|&&n| n > 0).count() >= 2 => ControlState::Contested,
            None => ControlState::Open,
        };
        control.insert(id, state);
    }
    (control, counts)
}

fn standings_line(standings: &[Standing]) -> String {
    standings
        .iter()
        .map(|s| {
            format!(
                "{}. C{} {}s {}h {}v",
                s.rank, s.commander.0, s.systems, s.holdings, s.deposit_value
            )
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

/// Runs `--season` and prints the history, the result and the checks.
#[expect(
    clippy::too_many_lines,
    reason = "the run loop, its report and its checks read best in one place"
)]
pub fn run(out: &mut impl Write, cfg: SeasonRun) -> Result<(), String> {
    let final_tick = cfg.final_tick;
    let season = SeasonConfig {
        final_tick,
        ..SeasonConfig::new(cfg.seed, 0)
    };
    let mut state =
        generate_stage1_with_rules(season, COMMANDERS, cfg.rules).map_err(|e| format!("{e:?}"))?;
    let interval = |c: CommanderId| {
        if cfg.early_win && c == EXPANDER {
            HOUR
        } else {
            cfg.claim_hours.max(1).saturating_mul(HOUR)
        }
    };
    let mut report = String::new();
    let mut errors: Vec<String> = Vec::new();
    // Each commander's triple and since, from the announced history of changes (D-193).
    let mut observed: BTreeMap<CommanderId, ((u32, u32, DepositValue), Tick)> = state
        .standings()
        .iter()
        .map(|s| {
            (
                s.commander,
                ((s.systems, s.holdings, s.deposit_value), Tick::ZERO),
            )
        })
        .collect();
    let mut changes = 0_usize;
    let mut claims = 0_usize;
    let mut ended_at = None;
    let mut times = Vec::new();
    let started = Instant::now();
    let mut round = 0_usize;
    while !state.season_over() {
        let t = state.tick().get();
        if Some(t) == cfg.restore_at {
            let bytes = encode(&state).map_err(|e| e.to_string())?;
            state = decode(&bytes).map_err(|e| e.to_string())?;
        }
        // Claims: whoever is due, in an order that rotates each hour.
        if t.is_multiple_of(HOUR) {
            round += 1;
            for i in 0..COMMANDERS.len() {
                let c = COMMANDERS[(i + round) % COMMANDERS.len()];
                if t.is_multiple_of(interval(c))
                    && let Some(site) = next_claim(&state, c)
                    && scenario::place_outpost(&mut state, c, site).is_some()
                {
                    claims += 1;
                }
            }
        }
        let tick_started = Instant::now();
        let outputs = step(&mut state, &TickInputs::default());
        times.push(tick_started.elapsed().as_micros());
        for a in &outputs.announcements {
            let line = match &a.kind {
                AnnouncementKind::ControlChanged { system, from, to } => {
                    format!(
                        "control    system {}: {from:?} -> {to:?}",
                        system.sim_id().get()
                    )
                }
                AnnouncementKind::StandingsChanged { standings } => {
                    changes += 1;
                    for s in standings {
                        let triple = (s.systems, s.holdings, s.deposit_value);
                        let entry = observed.entry(s.commander).or_insert((triple, a.tick));
                        if entry.0 != triple {
                            *entry = (triple, a.tick);
                        }
                    }
                    format!("standings  {}", standings_line(standings))
                }
                AnnouncementKind::NearEarlyWin {
                    commander,
                    systems,
                    threshold,
                } => format!(
                    "near win   C{} controls {systems} of the {threshold} systems an early win needs",
                    commander.0
                ),
                AnnouncementKind::NearEarlyWinEnded { commander } => {
                    format!("near win   C{} dropped back", commander.0)
                }
                AnnouncementKind::SeasonEnded { result, .. } => {
                    ended_at = Some(a.tick);
                    format!(
                        "season end {:?}, winners {:?}",
                        result.reason, result.winners
                    )
                }
                _ => continue,
            };
            let _ = writeln!(report, "  {}  {line}", when(a.tick));
        }
    }
    let elapsed = started.elapsed().as_secs_f64();
    times.sort_unstable();
    let mean = times.iter().sum::<u128>() as f64 / times.len().max(1) as f64;
    let p99 = times.get(times.len() * 99 / 100).copied().unwrap_or(0);

    let result = state
        .season_result()
        .cloned()
        .ok_or("the season has no result")?;
    // ---- checks against the brute-force recount
    let (control, counts) = recount(&state);
    if control != state.control_states() {
        errors.push(format!(
            "control: recount {control:?}, simulation {:?}",
            state.control_states()
        ));
    }
    let claimable = control.len() as u64;
    let share = u64::from(state.rules().season.early_win_share_permille);
    let threshold = u32::try_from((claimable * share).div_ceil(1_000).max(1)).unwrap_or(u32::MAX);
    if threshold != state.early_win_threshold() {
        errors.push(format!(
            "threshold: recount {threshold}, simulation {}",
            state.early_win_threshold()
        ));
    }
    let standings = state.standings().ranked().to_vec();
    let key = |s: &Standing| {
        (
            Reverse(s.systems),
            Reverse(s.holdings),
            Reverse(s.deposit_value),
            s.since,
        )
    };
    for s in &standings {
        let expected = counts.get(&s.commander).copied().unwrap_or_default();
        if (s.systems, s.holdings, s.deposit_value)
            != (expected.systems, expected.holdings, expected.deposit_value)
        {
            errors.push(format!(
                "C{}: standing {s:?}, recount {expected:?}",
                s.commander.0
            ));
        }
        if let Some(&(_, since)) = observed.get(&s.commander)
            && since != s.since
        {
            errors.push(format!(
                "C{}: since {}, the history says {}",
                s.commander.0,
                s.since.get(),
                since.get()
            ));
        }
        let better = standings.iter().filter(|o| key(o) < key(s)).count();
        if s.rank as usize != better + 1 {
            errors.push(format!(
                "C{}: rank {}, brute force {}",
                s.commander.0,
                s.rank,
                better + 1
            ));
        }
    }
    let expected_winners: Vec<CommanderId> = match result.reason {
        SeasonEndReason::EarlyWin => counts
            .iter()
            .filter(|(_, r)| r.systems >= threshold)
            .map(|(&c, _)| c)
            .collect(),
        SeasonEndReason::FinalTick => {
            let best = standings.iter().map(key).min();
            let mut first: Vec<CommanderId> = standings
                .iter()
                .filter(|s| Some(key(s)) == best)
                .map(|s| s.commander)
                .collect();
            first.sort_unstable();
            first
        }
    };
    if result.winners != expected_winners {
        errors.push(format!(
            "winners {:?}, brute force {expected_winners:?}",
            result.winners
        ));
    }
    if result.reason == SeasonEndReason::FinalTick && result.tick != final_tick {
        errors.push(format!(
            "ended at {}, scheduled {}",
            result.tick.get(),
            final_tick.get()
        ));
    }
    if ended_at != Some(result.tick) || state.tick() != result.tick {
        errors.push(format!(
            "the result is for tick {}, announced at {ended_at:?}, the Universe at {}",
            result.tick.get(),
            state.tick().get()
        ));
    }
    errors.extend(state.season_problems());

    let hash = state_hash(&state).map_err(|e| e.to_string())?;
    let mut text = String::new();
    let _ = writeln!(
        text,
        "season | seed {} | 4 scripted commanders, a claim every {} h{} | final tick {} ({}) | early win at {threshold} of {claimable} systems (D-192)",
        cfg.seed,
        cfg.claim_hours,
        if cfg.early_win {
            ", commander 2 every hour"
        } else {
            ""
        },
        final_tick.get(),
        when(final_tick),
    );
    text.push_str(&report);
    let _ = writeln!(
        text,
        "result: {:?} at tick {} ({}), winners {} | {claims} claims, {changes} standings changes",
        result.reason,
        result.tick.get(),
        when(result.tick),
        result
            .winners
            .iter()
            .map(|c| format!("C{}", c.0))
            .collect::<Vec<_>>()
            .join(", "),
    );
    let _ = writeln!(text, "final standings:");
    for s in &standings {
        let _ = writeln!(
            text,
            "  {}. C{}: {} systems, {} holdings, deposit value {}, since {}",
            s.rank,
            s.commander.0,
            s.systems,
            s.holdings,
            s.deposit_value,
            when(s.since)
        );
    }
    if errors.is_empty() {
        let _ = writeln!(
            text,
            "checks passed: the brute-force recount gives the same control, standings, ranks and winners"
        );
    } else {
        for e in &errors {
            let _ = writeln!(text, "CHECK FAILED: {e}");
        }
    }
    let _ = writeln!(
        text,
        "ticks {} | tick time: mean {mean:.0} µs, p99 {p99} µs | elapsed {elapsed:.1} s | hash {hash:016x}",
        state.tick().get()
    );
    out.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!("{} season checks failed", errors.len()))
    }
}

#[cfg(test)]
mod tests {
    use nomad_sim::Ticks;

    use super::*;

    #[test]
    #[ignore = "long simulation: days of a stage-1 Universe; CI runs fast tests only (D-190), run with --ignored"]
    fn a_scripted_early_win_matches_the_recount_and_survives_a_restore() {
        let run_once = |restore_at| {
            let mut out = Vec::new();
            let result = run(
                &mut out,
                SeasonRun {
                    seed: 7,
                    final_tick: Tick::new(Ticks::from_days(7).get()),
                    claim_hours: 12,
                    early_win: true,
                    restore_at,
                    rules: Rules::embedded(),
                },
            );
            let text = String::from_utf8_lossy(&out).into_owned();
            assert!(result.is_ok(), "{result:?}\n{text}");
            text
        };
        // A restore in the middle, a minute before an outpost completes.
        let text = run_once(Some(Ticks::from_hours(12).get() - 600));
        assert!(text.contains("result: EarlyWin"), "{text}");
        assert!(text.contains("winners C2"), "{text}");
        assert!(text.contains("near win   C2 controls 5 of the 6"), "{text}");
        assert!(text.contains("checks passed"), "{text}");
    }

    #[test]
    #[ignore = "long simulation: hours of a stage-1 Universe; CI runs fast tests only (D-190), run with --ignored"]
    fn a_scripted_season_runs_to_its_final_tick() {
        let mut out = Vec::new();
        let result = run(
            &mut out,
            SeasonRun {
                seed: 7,
                final_tick: Tick::new(Ticks::from_hours(6).get()),
                claim_hours: 1,
                early_win: false,
                restore_at: None,
                rules: Rules::embedded(),
            },
        );
        let text = String::from_utf8_lossy(&out);
        assert!(result.is_ok(), "{result:?}\n{text}");
        assert!(text.contains("result: FinalTick at tick 216000"), "{text}");
        assert!(text.contains("checks passed"), "{text}");
    }

    #[test]
    fn claims_go_where_a_majority_is_closest() {
        let state =
            generate_stage1_with_rules(SeasonConfig::new(7, 0), COMMANDERS, Rules::embedded())
                .unwrap();
        // Everyone starts in its home system, with its mother station there.
        for c in COMMANDERS {
            let site = next_claim(&state, c).unwrap();
            let system = state.galaxy().site(site).unwrap().system;
            let home = state
                .holdings()
                .find(|(_, h)| h.owner == c && h.mother_station)
                .map(|(_, h)| state.galaxy().site(h.site).unwrap().system);
            assert_eq!(Some(system), home);
        }
        assert_eq!(
            when(Tick::new(DAY + 90 * 60 * TICKS_PER_SECOND)),
            "day  1 01:30"
        );
    }
}
