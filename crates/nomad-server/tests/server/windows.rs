//! Commanders through the server (S6a): the human's window from `create`, `SetExposureWindow`
//! from a session, presence from the server's own connection inputs, and `SetPresence`
//! refused (S6 plan §4.1, §4.3; D-157, D-047).

use nomad_server::{CreateParams, FakeClock, Input, ServerEvent, SubmitError, create};
use nomad_sim::{
    Command, CommandRejection, CommanderId, CommanderInput, CommanderInputKind, CommanderKind,
    OperatorInput, Presence, QuarterHour, Tick, Ticks,
};

use crate::common::{BUILD, T0, TestDir, run_live_until, snapshot_state, start_live};

fn input(seq: u64, kind: CommanderInputKind) -> Input {
    Input::Commander(CommanderInput {
        commander: CommanderId(1),
        client_seq: seq,
        kind,
    })
}

#[test]
fn the_human_window_is_set_at_create_and_chosen_through_the_server() {
    let dir = TestDir::new("windows");
    let clock = FakeClock::new(T0);
    let params = CreateParams {
        seed: Some(5),
        window_start: QuarterHour::at(6, 30).expect("a quarter hour"),
        ..CreateParams::default()
    };
    let created = create(&dir.data(), &params, &clock, BUILD).expect("create");

    // The roster: commander 1 is the human with the given window; the AI commanders take
    // theirs from their seeded schedules (D-157, D-204).
    let state = snapshot_state(&created.dir, 0);
    let human = state.commander(CommanderId(1)).expect("the human");
    assert_eq!(human.kind, CommanderKind::Human);
    assert_eq!(human.window.start, params.window_start);
    for n in 2..=4 {
        let ai = state.commander(CommanderId(n)).expect("an AI commander");
        let schedule = state.schedule(CommanderId(n)).expect("a scheduled AI");
        assert_eq!(ai.kind, CommanderKind::Ai);
        assert_eq!(ai.window.start.index(), schedule.window_start(240));
    }

    // A session connects and chooses 21:00; an invalid start is refused by the validator.
    let server = start_live(&created.dir, &clock);
    let events = server.subscribe();
    server
        .submit(input(1, CommanderInputKind::Connected))
        .expect("connect");
    run_live_until(&server, &clock, 10);
    let start = QuarterHour::at(21, 0).expect("a quarter hour");
    let invalid: QuarterHour = postcard::from_bytes(&[99]).expect("decodes");
    for (seq, start) in [(2, start), (3, invalid)] {
        server
            .submit(input(
                seq,
                CommanderInputKind::Command(Command::SetExposureWindow { start }),
            ))
            .expect("command");
    }
    run_live_until(&server, &clock, 20);
    let outcomes: Vec<_> = events
        .try_iter()
        .filter_map(|event| match event {
            ServerEvent::Tick(out) => Some(out.commands),
            _ => None,
        })
        .flatten()
        .map(|result| (result.client_seq, result.outcome))
        .collect();
    assert_eq!(
        outcomes,
        vec![(2, Ok(())), (3, Err(CommandRejection::InvalidWindow))]
    );
    // A production server never takes the harness's presence input (S6 plan §4.1).
    assert_eq!(
        server.submit(Input::Operator(OperatorInput::SetPresence {
            commander: CommanderId(2),
            online: true,
        })),
        Err(SubmitError::Reserved)
    );
    let stopped = server.shutdown().expect("clean shutdown").tick;
    let state = snapshot_state(&created.dir, stopped.get());
    let human = state.commander(CommanderId(1)).expect("the human");
    assert_eq!(human.chosen_window, Some(start));
    assert_eq!(
        human.presence,
        Presence::Online {
            since: Tick::new(1)
        }
    );

    // A restart: the commander restored as connected has no session, so the first tick
    // disconnects them (N3), and their grace starts then.
    let server = start_live(&created.dir, &clock);
    run_live_until(&server, &clock, stopped.get() + 5);
    let stopped_again = server.shutdown().expect("clean shutdown").tick;
    let state = snapshot_state(&created.dir, stopped_again.get());
    let human = state.commander(CommanderId(1)).expect("the human");
    let offline = stopped.next();
    assert_eq!(human.presence, Presence::Offline { since: offline });
    assert_eq!(
        human.grace_until,
        Some(offline.after(Ticks::from_secs(120)))
    );
    assert!(state.connected().is_empty());
}
