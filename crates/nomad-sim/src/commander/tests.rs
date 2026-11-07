//! Commander records, presence, the grace and the lapse of orders, activity and exposure
//! windows (S6 plan §8: "Windows", "Validator", "Determinism").

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use nomad_types::{Command, FactionId, FleetOrder, Owner, QuarterHour};

use super::*;
use crate::announce::{Announcement, AnnouncementKind, Recipients};
use crate::command::CommandRejection as R;
use crate::command::tests::commander_reasons;
use crate::fleet::{ActiveOrder, Fleet, FleetEvent, FleetId, Motion, Place, ShipClass, Task};
use crate::galaxy::{SystemId, generate_stage1_roster};
use crate::geometry::Vec2;
use crate::input::{CommanderInput, CommanderInputKind, OperatorInput, TickInputs};
use crate::rules::Rules;
use crate::snapshot::{decode, encode, state_hash};
use crate::state::SeasonConfig;
use crate::step::step;
use crate::testkit::{
    COMMANDERS, at, commander, home, idle, kit_fleet, order, site_position, spawn_fleet, stage1,
};
use crate::time::runs_on;
use crate::validate_command;

/// 2026-11-06T00:00:00Z.
const DAY0: i64 = 1_793_923_200_000;
const DAY: u64 = 864_000;
const HOUR: u64 = 36_000;
const QUARTER: u64 = 9_000;
const GRACE: u64 = 1_200;

fn q(hours: u8, minutes: u8) -> QuarterHour {
    QuarterHour::at(hours, minutes).unwrap()
}

/// The tick of `hours:minutes` UTC on day `day` of a Universe that starts at 00:00 UTC.
fn t(day: u64, hours: u64, minutes: u64) -> Tick {
    Tick::new(day * DAY + hours * HOUR + minutes * 600)
}

fn ai(n: u32) -> CommanderSetup {
    CommanderSetup::ai(CommanderId(n))
}

/// A Universe with no galaxy, ships or holdings that starts at 00:00 UTC, with these
/// commanders.
fn bare(setups: &[CommanderSetup]) -> UniverseState {
    let mut state = UniverseState::new(SeasonConfig {
        seed: 1,
        season_start_unix_ms: DAY0,
    });
    for &setup in setups {
        add_commander(&mut state, setup).unwrap();
    }
    state
}

fn set_presence(n: u32, online: bool) -> OperatorInput {
    OperatorInput::SetPresence {
        commander: CommanderId(n),
        online,
    }
}

fn connection(n: u32, seq: u64, connected: bool) -> CommanderInput {
    CommanderInput {
        commander: CommanderId(n),
        client_seq: seq,
        kind: if connected {
            CommanderInputKind::Connected
        } else {
            CommanderInputKind::Disconnected
        },
    }
}

fn run(
    state: &mut UniverseState,
    operator: Vec<OperatorInput>,
    commanders: Vec<CommanderInput>,
) -> TickOutputs {
    step(
        state,
        &TickInputs {
            operator,
            commanders,
        },
    )
}

/// Runs ticks until the last completed tick is `tick`.
fn idle_until(state: &mut UniverseState, tick: Tick) -> Vec<TickOutputs> {
    let n = tick.get().saturating_sub(state.tick().get());
    idle(state, n)
}

fn presence(state: &UniverseState, n: u32) -> Presence {
    state.commander(CommanderId(n)).unwrap().presence
}

/// Every `WindowMoved` in `outputs`: tick, commander, start.
fn moves(outputs: &[TickOutputs]) -> Vec<(Tick, CommanderId, QuarterHour)> {
    outputs
        .iter()
        .flat_map(|out| &out.announcements)
        .filter_map(|a| match a.kind {
            AnnouncementKind::WindowMoved {
                commander, start, ..
            } => {
                assert_eq!(a.to, Recipients::Public, "the window is public (D-075)");
                Some((a.tick, commander, start))
            }
            AnnouncementKind::OrdersLapsed { .. } => None,
        })
        .collect()
}

/// Scripted `SetPresence` inputs, by tick.
type Script = BTreeMap<Tick, Vec<OperatorInput>>;

/// AI commander `n` is online from `from` to `to` UTC on each day of `days`.
fn daily(script: &mut Script, n: u32, from: QuarterHour, to: QuarterHour, days: Range<u64>) {
    for day in days {
        let at = |quarter: QuarterHour| Tick::new(day * DAY + u64::from(quarter.index()) * QUARTER);
        script
            .entry(at(from))
            .or_default()
            .push(set_presence(n, true));
        script
            .entry(at(to))
            .or_default()
            .push(set_presence(n, false));
    }
}

/// Runs a Universe without ships or holdings until `until`, computing only the ticks on
/// which something can happen in it: a scripted input, an activity sample or a deadline.
/// Every other tick of such a Universe changes nothing but the tick counter, so this equals
/// stepping every tick (`fast_forward_equals_stepping_every_tick` checks it).
///
/// It also checks D-155 on every tick it computes: a window changes only on its close.
fn fast_forward(state: &mut UniverseState, until: Tick, script: &Script) -> Vec<TickOutputs> {
    assert_eq!(
        state.holdings().count(),
        0,
        "only for Universes without holdings"
    );
    assert!(
        state.fleet_ids().is_empty(),
        "only for Universes without fleets"
    );
    let period = Ticks::from_secs(window::SAMPLE_INTERVAL_S);
    let mut samples: BTreeMap<CommanderId, Tick> = state
        .commander_ids()
        .into_iter()
        .map(|c| {
            let mut tick = state.tick().next();
            while !runs_on(tick, period, window::sample_key(c)) {
                tick = tick.next();
            }
            (c, tick)
        })
        .collect();
    let mut outputs = Vec::new();
    loop {
        let next = [
            samples.values().min().copied(),
            state.commanders.deadlines.next(),
            state.timers.next_deadline(),
            script.range(state.tick().next()..).next().map(|(&t, _)| t),
        ]
        .into_iter()
        .flatten()
        .min();
        let Some(next) = next.filter(|&next| next <= until) else {
            break;
        };
        let before: Vec<_> = state
            .commander_ids()
            .into_iter()
            .map(|c| state.exposure_window(c).unwrap())
            .collect();
        state.tick = Tick::new(next.get() - 1);
        let operator = script.get(&next).cloned().unwrap_or_default();
        let out = run(state, operator, vec![]);
        for (c, before) in state.commander_ids().into_iter().zip(before) {
            let after = state.exposure_window(c).unwrap();
            if after != before {
                assert_eq!(
                    out.tick, before.closes,
                    "{c:?}'s window changed before it closed"
                );
                assert!(
                    after.opens >= out.tick,
                    "a moved window does not reopen at once"
                );
            }
        }
        outputs.push(out);
        for (&c, tick) in &mut samples {
            if *tick == next {
                *tick = next.after(period);
                assert!(runs_on(*tick, period, window::sample_key(c)));
            }
        }
    }
    state.tick = until;
    outputs
}

#[test]
fn generation_makes_a_record_per_commander() {
    let state = stage1(7);
    assert_eq!(state.commander_ids(), COMMANDERS.to_vec());
    for c in COMMANDERS {
        let record = state.commander(c).unwrap();
        assert_eq!(record.kind, CommanderKind::Ai);
        assert_eq!(record.joined, Tick::ZERO);
        assert_eq!(record.presence, Presence::Offline { since: Tick::ZERO });
        assert_eq!(record.grace_until, None);
        assert_eq!(record.chosen_window, None);
        assert_eq!(record.free_choice_until, Tick::new(7 * DAY));
        // The season starts at 00:00 UTC: the default window opens at 19:00 (D-157).
        assert_eq!(
            record.window,
            ExposureWindow {
                start: q(19, 0),
                opens: t(0, 19, 0),
                closes: t(0, 23, 0),
            }
        );
        assert_eq!(state.exposure_window(c), Some(record.window));
        assert!(!state.counts_as_online(c));
        assert_eq!(state.activity_minutes(c), Some(vec![0; 96]));
    }
    assert_eq!(state.commander(CommanderId(9)), None);

    // The server's roster: a human with their own window (D-157).
    let season = SeasonConfig {
        seed: 7,
        season_start_unix_ms: DAY0,
    };
    let roster = [
        CommanderSetup::human(CommanderId(1), q(6, 30)),
        ai(2),
        ai(3),
        ai(4),
    ];
    let state = generate_stage1_roster(season, roster, Rules::embedded()).unwrap();
    let human = state.commander(CommanderId(1)).unwrap();
    assert_eq!(human.kind, CommanderKind::Human);
    assert_eq!(human.window.start, q(6, 30));
    // The roster changes no identity of the galaxy, holdings or ships.
    let plain = crate::galaxy::generate_stage1(season, COMMANDERS).unwrap();
    assert_eq!(state.fleet_ids(), plain.fleet_ids());
    assert_eq!(state.ship_ids(), plain.ship_ids());

    let mut duplicate = roster;
    duplicate[3] = ai(2);
    assert!(generate_stage1_roster(season, duplicate, Rules::embedded()).is_err());
    let invalid: QuarterHour = postcard::from_bytes(&[200]).unwrap();
    let mut bad = roster;
    bad[0].window_start = invalid;
    assert!(matches!(
        generate_stage1_roster(season, bad, Rules::embedded()),
        Err(crate::galaxy::GenerationError::Commander(
            CommanderError::InvalidWindow(CommanderId(1))
        ))
    ));
}

#[test]
fn a_window_open_at_creation_is_the_current_occurrence() {
    // A season that starts at 20:30 UTC: the 19:00 window is open, until 23:00.
    let mut state = UniverseState::new(SeasonConfig {
        seed: 1,
        season_start_unix_ms: DAY0 + 20 * 3_600_000 + 1_800_000,
    });
    add_commander(&mut state, ai(2)).unwrap();
    let window = state.exposure_window(CommanderId(2)).unwrap();
    assert_eq!(window.opens, Tick::ZERO);
    assert_eq!(window.closes, Tick::new(HOUR * 5 / 2));
    assert!(window.is_open(Tick::ZERO));
    // Starting at 23:30 it opens at 19:00 the next day.
    let mut state = UniverseState::new(SeasonConfig {
        seed: 1,
        season_start_unix_ms: DAY0 + 23 * 3_600_000 + 1_800_000,
    });
    add_commander(&mut state, ai(2)).unwrap();
    let window = state.exposure_window(CommanderId(2)).unwrap();
    assert_eq!(window.opens, Tick::new(HOUR * 39 / 2));
    assert!(!window.is_open(Tick::ZERO));
    assert_eq!(
        add_commander(&mut state, ai(2)),
        Err(CommanderError::Duplicate(CommanderId(2)))
    );
}

#[test]
fn presence_follows_connections_and_the_ai_schedule() {
    let mut state = bare(&[CommanderSetup::human(CommanderId(1), q(19, 0)), ai(2)]);
    run(&mut state, vec![], vec![connection(1, 1, true)]);
    assert_eq!(
        presence(&state, 1),
        Presence::Online {
            since: Tick::new(1)
        }
    );
    // SetPresence is for AI commanders only, and must change something.
    let out = run(
        &mut state,
        vec![
            set_presence(1, false),
            set_presence(9, true),
            set_presence(2, true),
        ],
        vec![],
    );
    assert_eq!(out.inputs_rejected, 2);
    assert_eq!(
        presence(&state, 1),
        Presence::Online {
            since: Tick::new(1)
        }
    );
    assert_eq!(
        presence(&state, 2),
        Presence::Online {
            since: Tick::new(2)
        }
    );
    let out = run(&mut state, vec![set_presence(2, true)], vec![]);
    assert_eq!(out.inputs_rejected, 1);
    // A disconnect and a reconnect in one tick change nothing: no grace starts.
    run(
        &mut state,
        vec![],
        vec![connection(1, 2, false), connection(1, 3, true)],
    );
    assert_eq!(
        presence(&state, 1),
        Presence::Online {
            since: Tick::new(1)
        }
    );
    assert_eq!(state.commander(CommanderId(1)).unwrap().grace_until, None);
    // Offline: the grace runs 120 s, a deadline (doc 12 §5.3).
    let out = run(
        &mut state,
        vec![set_presence(2, false)],
        vec![connection(1, 4, false)],
    );
    assert_eq!(presence(&state, 1), Presence::Offline { since: out.tick });
    assert_eq!(presence(&state, 2), Presence::Offline { since: out.tick });
    let grace_until = out.tick.after(Ticks::new(GRACE));
    assert_eq!(
        state.commander(CommanderId(1)).unwrap().grace_until,
        Some(grace_until)
    );
    assert!(state.counts_as_online(CommanderId(1)));
    // A Connected input for an AI, which only tests send, counts as online too.
    run(&mut state, vec![], vec![connection(2, 1, true)]);
    assert!(presence(&state, 2).is_online());
    assert_eq!(state.commander(CommanderId(2)).unwrap().grace_until, None);
    // Commander 1's grace ends: no longer online for sieges (D-160).
    idle_until(&mut state, grace_until);
    assert!(!state.counts_as_online(CommanderId(1)));
    assert_eq!(state.commander(CommanderId(1)).unwrap().grace_until, None);
}

#[test]
fn nobody_is_online_during_catch_up() {
    let mut state = bare(&[CommanderSetup::human(CommanderId(1), q(19, 0)), ai(2)]);
    run(
        &mut state,
        vec![set_presence(2, true)],
        vec![connection(1, 1, true)],
    );
    idle_until(&mut state, Tick::new(6_000));
    let before: u32 = state.activity_minutes(CommanderId(2)).unwrap().iter().sum();
    assert_eq!(before, 10, "ten minutes online, one sample a minute");
    // The server's catch-up: the marker, and a Disconnected for the connected human
    // (D-047). The AI's schedule still says online, but nobody is online now.
    let out = run(
        &mut state,
        vec![OperatorInput::CatchUpStart],
        vec![connection(1, u64::MAX, false)],
    );
    for n in [1, 2] {
        assert_eq!(presence(&state, n), Presence::Offline { since: out.tick });
        // Inside the grace, but catch-up counts nobody as online (D-047).
        assert!(
            state
                .commander(CommanderId(n))
                .unwrap()
                .grace_until
                .is_some()
        );
        assert!(!state.counts_as_online(CommanderId(n)));
    }
    // Catch-up minutes count as offline (D-171).
    idle(&mut state, 6_000);
    assert_eq!(
        state
            .activity_minutes(CommanderId(2))
            .unwrap()
            .iter()
            .sum::<u32>(),
        before
    );
    // Live again: the AI's schedule has it online; the human must reconnect.
    let out = run(&mut state, vec![OperatorInput::CatchUpEnd], vec![]);
    assert_eq!(presence(&state, 2), Presence::Online { since: out.tick });
    assert!(!presence(&state, 1).is_online());
    // The grace ended during catch-up; the AI is back, so no grace is pending for it.
    assert_eq!(state.commander(CommanderId(1)).unwrap().grace_until, None);
    assert_eq!(state.commander(CommanderId(2)).unwrap().grace_until, None);
}

#[test]
fn activity_counts_minutes_online_by_utc_quarter_hour() {
    let mut state = bare(&[ai(2)]);
    let mut script = Script::new();
    // 30 minutes from 19:00, and 5 minutes from 23:55 that run past midnight.
    script.insert(t(0, 19, 0), vec![set_presence(2, true)]);
    script.insert(t(0, 19, 30), vec![set_presence(2, false)]);
    script.insert(t(0, 23, 55), vec![set_presence(2, true)]);
    script.insert(t(1, 0, 0), vec![set_presence(2, false)]);
    fast_forward(&mut state, t(1, 1, 0), &script);
    let minutes = state.activity_minutes(CommanderId(2)).unwrap();
    assert_eq!(minutes[76] + minutes[77], 30);
    assert!(minutes[76] <= 15 && minutes[77] <= 15);
    assert_eq!(minutes[95], 5);
    assert_eq!(minutes.iter().sum::<u32>(), 35);
}

#[test]
fn a_window_moves_at_its_close_by_at_most_an_hour_a_day() {
    // S6 plan §8: a pattern 6 h away from the window is reached in 6 days, 1 h a day; one
    // that shifts after 14 days moves once the new hours dominate the history.
    let mut state = bare(&[ai(2), ai(3), ai(4)]);
    let mut script = Script::new();
    daily(&mut script, 2, q(13, 0), q(17, 0), 0..30);
    daily(&mut script, 3, q(18, 0), q(22, 0), 0..14);
    daily(&mut script, 3, q(0, 0), q(4, 0), 14..30);
    let outputs = fast_forward(&mut state, t(30, 0, 0), &script);
    let of = |n: u32| -> Vec<(Tick, QuarterHour)> {
        moves(&outputs)
            .into_iter()
            .filter(|&(_, c, _)| c == CommanderId(n))
            .map(|(tick, _, start)| (tick, start))
            .collect()
    };
    // Commander 2: 19:00 → 13:00 in six daily steps, each at the window's close.
    assert_eq!(
        of(2),
        vec![
            (t(0, 23, 0), q(18, 0)),
            (t(1, 22, 0), q(17, 0)),
            (t(2, 21, 0), q(16, 0)),
            (t(3, 20, 0), q(15, 0)),
            (t(4, 19, 0), q(14, 0)),
            (t(5, 18, 0), q(13, 0)),
        ]
    );
    // Commander 3: 18:00 after the first day. From day 14 it plays at 00:00–04:00; on day
    // 20 old and new hours tie over 14 days and the window stays (D-155); on day 21 the new
    // hours lead, and the window moves forward, the shorter way, 1 h at each close.
    assert_eq!(
        of(3),
        vec![
            (t(0, 23, 0), q(18, 0)),
            (t(21, 22, 0), q(19, 0)),
            (t(22, 23, 0), q(20, 0)),
            (t(24, 0, 0), q(21, 0)),
            (t(25, 1, 0), q(22, 0)),
            (t(26, 2, 0), q(23, 0)),
            (t(27, 3, 0), q(0, 0)),
        ]
    );
    // Commander 4 was never online: every start ties, the window stays.
    assert!(of(4).is_empty());
    assert_eq!(
        state.exposure_window(CommanderId(4)).unwrap().start,
        q(19, 0)
    );
}

#[test]
fn fast_forward_equals_stepping_every_tick() {
    let setups = [
        CommanderSetup {
            window_start: q(2, 0),
            ..ai(2)
        },
        ai(3),
    ];
    let mut script = Script::new();
    daily(&mut script, 2, q(0, 15), q(1, 30), 0..1);
    daily(&mut script, 3, q(1, 0), q(3, 0), 0..1);
    let until = t(0, 7, 0);
    let mut fast = bare(&setups);
    let fast_out = fast_forward(&mut fast, until, &script);
    let mut slow = bare(&setups);
    let mut slow_out = Vec::new();
    while slow.tick() < until {
        let next = slow.tick().next();
        let operator = script.get(&next).cloned().unwrap_or_default();
        slow_out.push(run(&mut slow, operator, vec![]));
    }
    assert_eq!(state_hash(&fast).unwrap(), state_hash(&slow).unwrap());
    assert_eq!(moves(&fast_out), moves(&slow_out));
    // The 02:00 window closed at 06:00 and moved 1 h towards commander 2's activity.
    assert_eq!(
        moves(&slow_out),
        vec![(t(0, 6, 0), CommanderId(2), q(1, 0))]
    );
}

#[test]
fn a_chosen_window_applies_when_the_window_next_closes() {
    let mut state = bare(&[ai(2)]);
    let choose = |start| Command::SetExposureWindow { start };
    let send = |state: &mut UniverseState, command: Command| {
        let seq = state.tick().get();
        run(
            state,
            vec![],
            vec![CommanderInput {
                commander: CommanderId(2),
                client_seq: seq,
                kind: CommanderInputKind::Command(command),
            }],
        )
    };
    fast_forward(&mut state, t(0, 10, 0), &Script::new());
    let out = send(&mut state, choose(q(6, 0)));
    assert_eq!(out.commands[0].outcome, Ok(()));
    // Not yet: today's window still opens at 19:00 (D-157).
    let window = state.exposure_window(CommanderId(2)).unwrap();
    assert_eq!((window.start, window.opens), (q(19, 0), t(0, 19, 0)));
    assert_eq!(
        state.commander(CommanderId(2)).unwrap().chosen_window,
        Some(q(6, 0))
    );
    // It applies when that window closes, and the 06:00 window opens the next morning.
    let outputs = fast_forward(&mut state, t(1, 0, 0), &Script::new());
    assert_eq!(
        moves(&outputs),
        vec![(t(0, 23, 0), CommanderId(2), q(6, 0))]
    );
    let window = state.exposure_window(CommanderId(2)).unwrap();
    assert_eq!((window.opens, window.closes), (t(1, 6, 0), t(1, 10, 0)));
    assert_eq!(state.commander(CommanderId(2)).unwrap().chosen_window, None);
    // Choosing the current start withdraws a choice.
    send(&mut state, choose(q(8, 0)));
    assert_eq!(
        state.commander(CommanderId(2)).unwrap().chosen_window,
        Some(q(8, 0))
    );
    send(&mut state, choose(q(6, 0)));
    assert_eq!(state.commander(CommanderId(2)).unwrap().chosen_window, None);
}

#[test]
fn the_window_choice_is_refused_after_7_days_and_checked() {
    let mut state = bare(&[ai(2)]);
    let mut seen = BTreeSet::new();
    let mut check = |state: &UniverseState, issuer: Owner, start: QuarterHour| {
        let result = validate_command(state, issuer, &Command::SetExposureWindow { start });
        if let Err(reason) = result {
            seen.insert(reason);
        }
        result
    };
    let two = Owner::Commander(CommanderId(2));
    assert_eq!(check(&state, two, q(6, 0)), Ok(()));
    let invalid: QuarterHour = postcard::from_bytes(&[96]).unwrap();
    assert_eq!(check(&state, two, invalid), Err(R::InvalidWindow));
    assert_eq!(
        check(&state, Owner::Commander(CommanderId(9)), q(6, 0)),
        Err(R::UnknownCommander)
    );
    assert_eq!(
        check(&state, Owner::Npc(FactionId(1)), q(6, 0)),
        Err(R::UnknownCommander)
    );
    // The last tick of day 7 still accepts it; day 8 does not (D-157).
    state.tick = Tick::new(7 * DAY - 2);
    assert_eq!(check(&state, two, q(6, 0)), Ok(()));
    state.tick = Tick::new(7 * DAY - 1);
    assert_eq!(check(&state, two, q(6, 0)), Err(R::WindowLocked));
    assert_eq!(seen, commander_reasons());
}

#[test]
fn a_frozen_window_does_not_move() {
    // D-156: while a defence is pending the window stays, and a chosen start waits. S6b
    // reports pending defences; here the close is driven as `fire_due` would, frozen.
    let mut state = bare(&[ai(2)]);
    let mut script = Script::new();
    daily(&mut script, 2, q(13, 0), q(17, 0), 0..1);
    fast_forward(&mut state, t(0, 22, 0), &script);
    state
        .commanders
        .records
        .get_mut(&CommanderId(2))
        .unwrap()
        .window
        .chosen = Some(q(6, 0));
    let close = t(0, 23, 0);
    state.tick = Tick::new(close.get() - 1);
    let (_, who, kind) = state.commanders.deadlines.pop_due(close).unwrap();
    assert_eq!((who, kind), (CommanderId(2), Deadline::WindowCloses));
    let mut announcements = Vec::new();
    window::close(&mut state, close, CommanderId(2), true, &mut announcements);
    assert!(announcements.is_empty());
    let record = state.commander(CommanderId(2)).unwrap();
    assert_eq!(record.window.start, q(19, 0));
    assert_eq!(
        (record.window.opens, record.window.closes),
        (t(1, 19, 0), t(1, 23, 0))
    );
    assert_eq!(record.chosen_window, Some(q(6, 0)));
    // Unfrozen at the next close, the chosen start applies.
    state.tick = t(1, 23, 0);
    state.commanders.deadlines.pop_due(t(1, 23, 0)).unwrap();
    window::close(
        &mut state,
        t(1, 23, 0),
        CommanderId(2),
        false,
        &mut announcements,
    );
    assert_eq!(
        state.exposure_window(CommanderId(2)).unwrap().start,
        q(6, 0)
    );
    assert_eq!(announcements.len(), 1);
    // S6a has no defences: nothing is ever frozen yet.
    assert!(!window::defence_pending(&state, CommanderId(2)));
}

/// A stage-1 Universe where commanders 1–3 are connected, commander 1's start kit is
/// launched and patrolling, commander 2's kit is docked with an order queued, and commander
/// 3's kit runs a convoy.
fn lapse_scene() -> (UniverseState, FleetId, FleetId, FleetId) {
    let mut state = stage1(7);
    run(
        &mut state,
        vec![],
        (1..=3).map(|n| connection(n, 1, true)).collect(),
    );
    let one = kit_fleet(&state, 1);
    crate::testkit::send(&mut state, 1, order(one, FleetOrder::Launch));
    let (_, system) = home(&state, 1);
    let route = vec![
        at(system, site_position(&state, system, 2)),
        at(system, site_position(&state, system, 3)),
    ];
    let out = crate::testkit::send(&mut state, 1, order(one, FleetOrder::Patrol { route }));
    assert_eq!(out.commands[0].outcome, Ok(()));
    let two = kit_fleet(&state, 2);
    if let Some(f) = state.entities.get_mut::<Fleet>(two.sim_id()) {
        f.queue.push_back(FleetOrder::Launch);
    }
    // Commander 3's kit runs a convoy route: doctrine, not a direct order (D-162).
    let three = kit_fleet(&state, 3);
    let (mother, _) = home(&state, 3);
    state.economy.convoys.insert(
        three,
        crate::economy::Convoy {
            route: vec![mother],
            next: 0,
        },
    );
    if let Some(f) = state.entities.get_mut::<Fleet>(three.sim_id()) {
        f.queue.push_back(FleetOrder::Launch);
    }
    (state, one, two, three)
}

#[test]
fn direct_orders_lapse_when_the_grace_ends() {
    let (mut state, one, two, three) = lapse_scene();
    // An idle fleet has no direct order to lose.
    let (_, system) = home(&state, 1);
    let idle_fleet = spawn_fleet(
        &mut state,
        commander(1),
        system,
        Vec2::new(60_000.0, 0.0),
        &[ShipClass::Vx],
    );
    let gone = run(
        &mut state,
        vec![],
        (1..=3).map(|n| connection(n, 2, false)).collect(),
    )
    .tick;
    let ends = gone.after(Ticks::new(GRACE));
    // Until the grace ends the orders stand, and the commanders count as online (D-160).
    let outputs = idle_until(&mut state, Tick::new(ends.get() - 1));
    assert!(outputs.iter().all(|o| o.announcements.is_empty()));
    assert!(matches!(
        state.fleet(one).unwrap().order,
        Some(FleetOrder::Patrol { .. })
    ));
    assert!(state.counts_as_online(CommanderId(1)));
    let out = idle(&mut state, 1).remove(0);
    assert_eq!(out.tick, ends);
    assert!(!state.counts_as_online(CommanderId(1)));
    assert_eq!(
        out.announcements,
        vec![
            Announcement {
                tick: ends,
                to: Recipients::Commanders(vec![CommanderId(1)]),
                kind: AnnouncementKind::OrdersLapsed {
                    commander: CommanderId(1),
                    fleets: vec![one],
                },
            },
            Announcement {
                tick: ends,
                to: Recipients::Commanders(vec![CommanderId(2)]),
                kind: AnnouncementKind::OrdersLapsed {
                    commander: CommanderId(2),
                    fleets: vec![two],
                },
            },
        ]
    );
    // In space: station-keeping where it is, the doctrine until A1 (D-162).
    let view = state.fleet(one).unwrap();
    assert_eq!(
        view.order,
        Some(FleetOrder::StationKeep { relative_to: None })
    );
    assert!(view.queued.is_empty());
    // Docked: it stays docked, without its queue.
    let view = state.fleet(two).unwrap();
    assert!(matches!(view.place, Place::Docked(_)));
    assert_eq!((view.order, view.queued.len()), (None, 0));
    // The convoy and the idle fleet are untouched.
    assert!(state.economy.convoys.contains_key(&three));
    assert_eq!(state.fleet(idle_fleet).unwrap().order, None);
    assert!(state.fleet_problems().is_empty());
}

#[test]
fn a_return_inside_the_grace_keeps_the_orders() {
    let (mut state, one, _, _) = lapse_scene();
    let gone = run(&mut state, vec![], vec![connection(1, 2, false)]).tick;
    idle_until(&mut state, gone.after(Ticks::new(GRACE / 2)));
    run(&mut state, vec![], vec![connection(1, 3, true)]);
    let outputs = idle(&mut state, GRACE);
    assert!(outputs.iter().all(|o| o.announcements.is_empty()));
    assert!(matches!(
        state.fleet(one).unwrap().order,
        Some(FleetOrder::Patrol { .. })
    ));
    // Only the window closes are pending: the grace deadline was cancelled.
    assert!(
        state
            .commanders
            .deadlines
            .pending()
            .all(|(_, _, kind)| kind == Deadline::WindowCloses)
    );
}

/// A system two lane hops from `from`.
fn two_hops_away(state: &UniverseState, from: SystemId) -> SystemId {
    let near = state.galaxy().neighbours(from);
    near.iter()
        .flat_map(|&n| state.galaxy().neighbours(n))
        .find(|s| *s != from && !near.contains(s))
        .expect("stage 1 has a system two jumps from home")
}

#[test]
fn a_fleet_in_lane_transit_finishes_its_hop_then_station_keeps() {
    let mut state = stage1(7);
    run(&mut state, vec![], vec![connection(1, 1, true)]);
    let (_, system) = home(&state, 1);
    let far = two_hops_away(&state, system);
    let fleet = spawn_fleet(
        &mut state,
        commander(1),
        system,
        Vec2::new(60_000.0, 0.0),
        &[ShipClass::Vx],
    );
    crate::testkit::send(
        &mut state,
        1,
        order(fleet, FleetOrder::Jump { system: far }),
    );
    // Put it at the route's first gate, so it departs after the spool-up.
    let gate = match &state.entities.get::<Fleet>(fleet.sim_id()).unwrap().active {
        Some(ActiveOrder {
            task: Task::Travel { hops, .. },
            ..
        }) => hops[0].from_gate,
        other => panic!("not travelling: {other:?}"),
    };
    let gate = state.galaxy().gate(gate).unwrap().position;
    let ships = state.fleet(fleet).unwrap().ships;
    for id in ships.iter().map(|s| s.sim_id()).chain([fleet.sim_id()]) {
        if let Some(m) = state.entities.get_mut::<Motion>(id) {
            m.position = gate;
        }
    }
    let mut departed = false;
    for _ in 0..200 {
        let out = idle(&mut state, 1).remove(0);
        if out
            .fleet_events
            .iter()
            .any(|e| matches!(e, FleetEvent::Departed { fleet: f, .. } if *f == fleet))
        {
            departed = true;
            break;
        }
    }
    assert!(departed);
    let hops_before = match &state.entities.get::<Fleet>(fleet.sim_id()).unwrap().active {
        Some(ActiveOrder {
            task: Task::Travel { hops, .. },
            ..
        }) => hops.len(),
        other => panic!("not travelling: {other:?}"),
    };
    assert!(hops_before >= 2);
    run(&mut state, vec![], vec![connection(1, 2, false)]);
    idle(&mut state, GRACE);
    let f = state.entities.get::<Fleet>(fleet.sim_id()).unwrap();
    assert!(
        matches!(f.place, Place::InTransit { .. }),
        "transit is uninterruptible"
    );
    match &f.active {
        Some(ActiveOrder {
            task: Task::Travel {
                hops, then_dock, ..
            },
            ..
        }) => assert_eq!((hops.len(), *then_dock), (1, None)),
        other => panic!("not travelling: {other:?}"),
    }
    assert_eq!(
        f.queue.iter().collect::<Vec<_>>(),
        vec![&FleetOrder::StationKeep { relative_to: None }]
    );
}

#[test]
fn snapshot_and_restore_inside_the_grace_change_nothing() {
    let (mut state, _, _, _) = lapse_scene();
    let gone = run(&mut state, vec![], vec![connection(1, 2, false)]).tick;
    idle_until(&mut state, gone.after(Ticks::new(GRACE / 2)));
    let mut restored = decode(&encode(&state).unwrap()).unwrap();
    assert_eq!(restored, state);
    let a = idle(&mut state, GRACE);
    let b = idle(&mut restored, GRACE);
    assert_eq!(a, b);
    assert!(
        a.iter().any(|o| !o.announcements.is_empty()),
        "the lapse happened"
    );
    assert_eq!(state_hash(&state).unwrap(), state_hash(&restored).unwrap());
}
