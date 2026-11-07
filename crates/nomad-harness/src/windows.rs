//! `--windows`: exposure windows, presence and the lapse of orders over simulated days (S6
//! plan §8 "Windows"; D-032, D-047, D-154 to D-157, D-160 to D-162, D-171).
//!
//! The four stage-1 commanders are AI commanders whose presence the script sets with
//! `SetPresence`, as A5's schedules will (S6 plan §4.1):
//!
//! | | Online (UTC) | What it shows |
//! |---|---|---|
//! | C1 | 13:00–17:00 | 6 h before its 19:00 window: 1 h a day, there in 6 days (D-155). Patrols while online; the orders lapse 120 s after it goes (D-162). |
//! | C2 | 18:00–22:00, from day 14 00:00–04:00 | Settles at 18:00; after the shift the window holds until the new hours lead the 14-day history (ties keep it), then moves forward 1 h a day. |
//! | C3 | 08:00–09:30 and 18:00–20:00 | Two sessions: the window moves to the nearest of the tied best starts. |
//! | C4 | 06:00–10:00 | Drifts forward from 19:00, the shorter way; chooses 06:00 on day 1, applied at the next close; a choice on day 8 is refused (D-157). |
//!
//! On day 2 the "PC is off" from 13:00 to 15:00: a catch-up, during which nobody is online
//! (D-047) and the minutes count as offline (D-171). Commands are not sent then, as the server
//! drops them.
//!
//! It checks D-155 on every tick: a window changes only at its close, a drift is at most 1 h,
//! and a moved window does not reopen at once. It also checks each row of the table against
//! the moves, choices and lapses expected up to the end of the run. A failed check is an
//! error.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Write;
use std::time::Instant;

use nomad_sim::galaxy::{STAGE1_COMMANDERS, generate_stage1_roster};
use nomad_sim::snapshot::state_hash;
use nomad_sim::{
    AnnouncementKind, Command, CommandRejection, CommanderId, CommanderInput, CommanderInputKind,
    CommanderSetup, ExposureWindow, FleetOrder, Location, OperatorInput, Owner, Place, QuarterHour,
    Queueing, Rules, SeasonConfig, Tick, TickInputs, UniverseState, step,
};

const COMMANDERS: [CommanderId; STAGE1_COMMANDERS] = [
    CommanderId(1),
    CommanderId(2),
    CommanderId(3),
    CommanderId(4),
];
const DAY: u64 = 864_000;
const HOUR: u64 = 36_000;
const MINUTE: u64 = 600;

/// What `--windows` runs.
pub struct WindowRun {
    /// The season seed.
    pub seed: u64,
    /// Simulated days.
    pub days: u64,
    /// The rules (D-104).
    pub rules: Rules,
}

/// Something the script does on a tick.
#[derive(Clone, Copy, Debug)]
enum Event {
    Presence(CommanderId, bool),
    /// C1's fleet patrols (launching first if docked).
    Patrol(CommanderId),
    Choose(CommanderId, QuarterHour),
    CatchUpStart,
    CatchUpEnd,
}

fn at(day: u64, hours: u64, minutes: u64) -> u64 {
    day * DAY + hours * HOUR + minutes * MINUTE
}

fn q(hours: u8, minutes: u8) -> QuarterHour {
    QuarterHour::at(hours, minutes).expect("invariant: the script uses quarter hours")
}

fn clock(start: QuarterHour) -> String {
    let m = start.minutes_of_day();
    format!("{:02}:{:02}", m / 60, m % 60)
}

/// `day d HH:MM` for a tick.
fn when(tick: Tick) -> String {
    let t = tick.get();
    let in_day = t % DAY;
    format!(
        "day {:>2} {:02}:{:02}",
        t / DAY,
        in_day / HOUR,
        in_day % HOUR / MINUTE
    )
}

/// The script for `days` days.
fn script(days: u64) -> BTreeMap<u64, Vec<Event>> {
    let mut events: BTreeMap<u64, Vec<Event>> = BTreeMap::new();
    let mut add = |tick: u64, event| events.entry(tick).or_default().push(event);
    let [c1, c2, c3, c4] = COMMANDERS;
    for day in 0..days {
        let mut session = |c, from: (u64, u64), to: (u64, u64)| {
            add(at(day, from.0, from.1), Event::Presence(c, true));
            add(at(day, to.0, to.1), Event::Presence(c, false));
        };
        session(c1, (13, 0), (17, 0));
        if day < 14 {
            session(c2, (18, 0), (22, 0));
        } else {
            session(c2, (0, 0), (4, 0));
        }
        session(c3, (8, 0), (9, 30));
        session(c3, (18, 0), (20, 0));
        session(c4, (6, 0), (10, 0));
        add(at(day, 13, 0), Event::Patrol(c1));
    }
    add(at(1, 8, 0), Event::Choose(c4, q(6, 0)));
    add(at(8, 8, 0), Event::Choose(c4, q(12, 0)));
    add(at(2, 13, 0), Event::CatchUpStart);
    add(at(2, 15, 0), Event::CatchUpEnd);
    events
}

/// A commander's start-kit fleet.
fn kit_fleet(state: &UniverseState, c: CommanderId) -> Option<nomad_sim::FleetId> {
    state.fleet_ids().into_iter().find(|&f| {
        state
            .fleet(f)
            .is_some_and(|v| v.owner == Owner::Commander(c))
    })
}

/// The commands of a `Patrol` event: launch if docked, then patrol two sites of the home
/// system.
fn patrol(state: &UniverseState, c: CommanderId) -> Vec<Command> {
    let Some(fleet) = kit_fleet(state, c) else {
        return Vec::new();
    };
    let Some((_, mother)) = state
        .holdings()
        .find(|(_, h)| h.owner == c && h.mother_station)
    else {
        return Vec::new();
    };
    let Some(system) = state.galaxy().site(mother.site).map(|s| s.system) else {
        return Vec::new();
    };
    let route: Vec<Location> = state
        .galaxy()
        .system(system)
        .map(|s| s.sites.iter().skip(2).take(2).copied().collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|site| state.galaxy().site(site))
        .map(|site| Location {
            system,
            position: site.position,
        })
        .collect();
    let mut commands = Vec::new();
    if matches!(state.fleet(fleet).map(|v| v.place), Some(Place::Docked(_))) {
        commands.push(Command::Fleet {
            fleet,
            order: FleetOrder::Launch,
            queue: Queueing::Replace,
        });
    }
    commands.push(Command::Fleet {
        fleet,
        order: FleetOrder::Patrol { route },
        queue: Queueing::Replace,
    });
    commands
}

/// What happened to one commander.
#[derive(Default)]
struct Log {
    /// Window moves: tick, start, whether it was chosen.
    moves: Vec<(Tick, QuarterHour, bool)>,
    lapses: u32,
    patrols: u32,
    choices: Vec<(Tick, QuarterHour, Result<(), CommandRejection>)>,
}

#[expect(
    clippy::too_many_lines,
    reason = "one scripted run with its checks and report, read top to bottom"
)]
pub fn run(out: &mut impl Write, run: WindowRun) -> Result<(), String> {
    let season = SeasonConfig::new(run.seed, 0);
    let drift = run.rules.sieges.drift_quarters();
    let length_minutes = run.rules.sieges.window_minutes;
    let mut state = generate_stage1_roster(season, COMMANDERS.map(CommanderSetup::ai), run.rules)
        .map_err(|e| e.to_string())?;
    let script = script(run.days);
    let mut logs: BTreeMap<CommanderId, Log> =
        COMMANDERS.iter().map(|&c| (c, Log::default())).collect();
    let mut errors: Vec<String> = Vec::new();
    let mut pending_choices: BTreeMap<(CommanderId, u64), QuarterHour> = BTreeMap::new();
    let mut chosen: BTreeMap<CommanderId, QuarterHour> = BTreeMap::new();
    let mut seq = 0_u64;
    let window_of = |state: &UniverseState, c| -> ExposureWindow {
        state
            .exposure_window(c)
            .expect("invariant: every stage-1 commander has a record")
    };

    let ticks = run.days * DAY;
    let started = Instant::now();
    // Tick times in whole microseconds, the last bucket collecting everything slower.
    let mut histogram = vec![0_u64; 10_001];
    let mut total_nanos = 0_u64;
    for t in 1..=ticks {
        let mut inputs = TickInputs::default();
        let events = script.get(&t).map_or(&[][..], Vec::as_slice);
        let catching_up =
            state.catching_up() || events.iter().any(|e| matches!(e, Event::CatchUpStart));
        for &event in events {
            match event {
                Event::CatchUpStart => inputs.operator.insert(0, OperatorInput::CatchUpStart),
                Event::CatchUpEnd => inputs.operator.insert(0, OperatorInput::CatchUpEnd),
                Event::Presence(commander, online) => {
                    inputs
                        .operator
                        .push(OperatorInput::SetPresence { commander, online });
                }
                // The server drops commander inputs during catch-up.
                Event::Patrol(_) | Event::Choose(..) if catching_up => {}
                Event::Patrol(c) => {
                    for command in patrol(&state, c) {
                        seq += 1;
                        inputs.commanders.push(CommanderInput {
                            commander: c,
                            client_seq: seq,
                            kind: CommanderInputKind::Command(command),
                        });
                    }
                    if let Some(log) = logs.get_mut(&c) {
                        log.patrols += 1;
                    }
                }
                Event::Choose(c, start) => {
                    seq += 1;
                    pending_choices.insert((c, seq), start);
                    inputs.commanders.push(CommanderInput {
                        commander: c,
                        client_seq: seq,
                        kind: CommanderInputKind::Command(Command::SetExposureWindow { start }),
                    });
                }
            }
        }
        let before: Vec<ExposureWindow> =
            COMMANDERS.iter().map(|&c| window_of(&state, c)).collect();
        let tick_start = Instant::now();
        let outputs = step(&mut state, &inputs);
        let nanos = u64::try_from(tick_start.elapsed().as_nanos()).unwrap_or(u64::MAX);
        total_nanos = total_nanos.saturating_add(nanos);
        let bucket = usize::try_from(nanos / 1_000)
            .unwrap_or(usize::MAX)
            .min(10_000);
        if let Some(count) = histogram.get_mut(bucket) {
            *count += 1;
        }

        for result in &outputs.commands {
            if let Some(start) = pending_choices.remove(&(result.commander, result.client_seq)) {
                if result.outcome.is_ok() {
                    chosen.insert(result.commander, start);
                }
                if let Some(log) = logs.get_mut(&result.commander) {
                    log.choices.push((outputs.tick, start, result.outcome));
                }
            }
        }
        for (&c, before) in COMMANDERS.iter().zip(&before) {
            let after = window_of(&state, c);
            if after != *before && outputs.tick != before.closes {
                errors.push(format!(
                    "{c:?}'s window changed at {} before its close at {}",
                    when(outputs.tick),
                    when(before.closes)
                ));
            }
        }
        for announcement in &outputs.announcements {
            match &announcement.kind {
                AnnouncementKind::WindowMoved {
                    commander,
                    start,
                    opens,
                    ..
                } => {
                    let index = COMMANDERS.iter().position(|c| c == commander).unwrap_or(0);
                    let from = before[index].start;
                    let was_chosen = chosen.get(commander) == Some(start);
                    if was_chosen {
                        chosen.remove(commander);
                    }
                    let step = (i32::from(start.index()) - i32::from(from.index())).rem_euclid(96);
                    let step = step.min(96 - step);
                    if !was_chosen && step > i32::from(drift) {
                        errors.push(format!(
                            "{commander:?}'s window drifted {step} quarter hours at {}",
                            when(outputs.tick)
                        ));
                    }
                    if *opens < outputs.tick {
                        errors.push(format!("{commander:?}'s moved window reopened at once"));
                    }
                    if let Some(log) = logs.get_mut(commander) {
                        log.moves.push((outputs.tick, *start, was_chosen));
                    }
                }
                AnnouncementKind::OrdersLapsed { commander, .. } => {
                    if let Some(log) = logs.get_mut(commander) {
                        log.lapses += 1;
                    }
                }
                // Control, standings and the season's end are `--season`'s (S8).
                _ => {}
            }
        }
    }
    let elapsed = started.elapsed().as_secs_f64();

    // The rows of the table: every move and choice expected up to the end of the run.
    let [c1, c2, c3, c4] = COMMANDERS;
    let log = |c| logs.get(&c).expect("invariant: a log per commander");
    let drifts = |moves: &[(u64, (u8, u8))]| -> Vec<(Tick, QuarterHour, bool)> {
        moves
            .iter()
            .map(|&(tick, (h, m))| (Tick::new(tick), q(h, m), false))
            .filter(|m| m.0.get() <= ticks)
            .collect()
    };
    // C1: from 19:00 back to 13:00, 1 h at each close (D-155).
    let c1_moves = drifts(&[
        (at(0, 23, 0), (18, 0)),
        (at(1, 22, 0), (17, 0)),
        (at(2, 21, 0), (16, 0)),
        (at(3, 20, 0), (15, 0)),
        (at(4, 19, 0), (14, 0)),
        (at(5, 18, 0), (13, 0)),
    ]);
    // C2: 18:00; after the shift the old and new hours tie on day 20 and the window holds;
    // from day 21 it moves forward, the shorter way.
    let c2_moves = drifts(&[
        (at(0, 23, 0), (18, 0)),
        (at(21, 22, 0), (19, 0)),
        (at(22, 23, 0), (20, 0)),
        (at(24, 0, 0), (21, 0)),
        (at(25, 1, 0), (22, 0)),
        (at(26, 2, 0), (23, 0)),
        (at(27, 3, 0), (0, 0)),
    ]);
    // C3: the nearest of the starts tied for its 18:00 session.
    let c3_moves = drifts(&[(at(0, 23, 0), (18, 0))]);
    // C4: forward to 20:00, then its choice when the 20:00 window closes at midnight.
    let mut c4_moves = drifts(&[(at(0, 23, 0), (20, 0))]);
    c4_moves.extend(
        [(Tick::new(at(2, 0, 0)), q(6, 0), true)]
            .into_iter()
            .filter(|m| m.0.get() <= ticks),
    );
    let c4_choices: Vec<_> = [
        (Tick::new(at(1, 8, 0)), q(6, 0), Ok(())),
        (
            Tick::new(at(8, 8, 0)),
            q(12, 0),
            Err(CommandRejection::WindowLocked),
        ),
    ]
    .into_iter()
    .filter(|c| c.0.get() <= ticks)
    .collect();
    for (c, expected) in [
        (c1, c1_moves),
        (c2, c2_moves),
        (c3, c3_moves),
        (c4, c4_moves),
    ] {
        if log(c).moves != expected {
            errors.push(format!(
                "C{}'s window moves {:?}, expected {expected:?}",
                c.0,
                log(c).moves
            ));
        }
    }
    if log(c4).choices != c4_choices {
        errors.push(format!(
            "C4's choices {:?}, expected {c4_choices:?}",
            log(c4).choices
        ));
    }
    // Every patrol lapses once C1 has gone, and nothing else does (D-162).
    if log(c1).lapses != log(c1).patrols || log(c1).lapses == 0 {
        errors.push(format!(
            "C1 patrolled {} times and its orders lapsed {} times",
            log(c1).patrols,
            log(c1).lapses
        ));
    }
    if (3..=14).contains(&run.days) {
        // D-171: C1's 13:00 quarter lost day 2 to the catch-up; its 15:00 quarter did not.
        let minutes = state.activity_minutes(c1).unwrap_or_default();
        if minutes.get(52).map(|m| m + 15) != minutes.get(60).copied() {
            errors.push("catch-up minutes did not count as offline".to_owned());
        }
    }

    let mut report = String::new();
    let _ = writeln!(
        report,
        "windows | seed {} | {} days | stage 1, 4 AI commanders on SetPresence schedules | \
         catch-up day 2 13:00-15:00",
        run.seed, run.days
    );
    let schedule = [
        "13:00-17:00, patrols while online",
        "18:00-22:00, from day 14 00:00-04:00",
        "08:00-09:30 and 18:00-20:00",
        "06:00-10:00, chooses 06:00 on day 1 and 12:00 on day 8",
    ];
    let range = |start: QuarterHour| {
        let end = (start.minutes_of_day() + length_minutes) % (24 * 60);
        format!("{}-{:02}:{:02} UTC", clock(start), end / 60, end % 60)
    };
    for (&c, schedule) in COMMANDERS.iter().zip(schedule) {
        let log = log(c);
        let _ = writeln!(report, "  C{} online {schedule}", c.0);
        let mut lines: Vec<(Tick, String)> = log
            .moves
            .iter()
            .map(|&(tick, start, was_chosen)| {
                let why = if was_chosen { " (chosen, D-157)" } else { "" };
                (tick, format!("window moves to {}{why}", range(start)))
            })
            .chain(log.choices.iter().map(|&(tick, start, outcome)| {
                let what = match outcome {
                    Ok(()) => "accepted, applies when the window next closes".to_owned(),
                    Err(e) => format!("refused, {e}"),
                };
                (tick, format!("chooses {}: {what}", clock(start)))
            }))
            .collect();
        lines.sort_by_key(|(tick, _)| *tick);
        for (tick, line) in lines {
            let _ = writeln!(report, "    {}  {line}", when(tick));
        }
        let _ = writeln!(
            report,
            "    final window {} | {} moves | orders lapsed {} times",
            range(window_of(&state, c).start),
            log.moves.len(),
            log.lapses
        );
    }
    let mean = total_nanos / ticks.max(1);
    let mut seen = 0_u64;
    let p99 = histogram
        .iter()
        .position(|&count| {
            seen += count;
            seen * 100 >= ticks * 99
        })
        .unwrap_or(0);
    let _ = writeln!(
        report,
        "checks {} | tick time: mean {} µs, p99 {} µs | elapsed {elapsed:.1} s | hash {:016x}",
        if errors.is_empty() {
            "passed".to_owned()
        } else {
            format!("FAILED {}", errors.len())
        },
        mean / 1_000,
        p99,
        state_hash(&state).map_err(|e| e.to_string())?
    );
    for error in &errors {
        let _ = writeln!(report, "  check failed: {error}");
    }
    out.write_all(report.as_bytes())
        .map_err(|e| e.to_string())?;
    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!("{} window checks failed", errors.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "long simulation: 3 simulated days of a stage-1 Universe; CI runs fast tests only (D-190), run with --ignored"]
    fn three_days_of_windows_pass_their_checks() {
        let mut out = Vec::new();
        let result = run(
            &mut out,
            WindowRun {
                seed: 7,
                days: 3,
                rules: Rules::embedded(),
            },
        );
        let text = String::from_utf8_lossy(&out);
        assert!(result.is_ok(), "{result:?}\n{text}");
        assert!(text.contains("checks passed"), "{text}");
    }

    #[test]
    fn the_script_covers_every_commander() {
        let script = script(2);
        let presence = |c| {
            script
                .values()
                .flatten()
                .filter(|e| matches!(e, Event::Presence(x, _) if *x == c))
                .count()
        };
        assert_eq!(
            COMMANDERS.map(presence),
            [4, 4, 8, 4],
            "one on and one off per session and day"
        );
        assert_eq!(when(Tick::new(at(3, 13, 30))), "day  3 13:30");
    }
}
