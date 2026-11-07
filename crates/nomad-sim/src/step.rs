//! Advancing the Universe by one tick (Design doc 11 §4).

use nomad_types::{Command, Owner};

use crate::announce::Announcement;
use crate::combat::CombatEvent;
use crate::command::{CommandRejection, CommandSource, execute};
use crate::defence::DefenceEvent;
use crate::economy::EconomyEvent;
use crate::fleet::FleetEvent;
use crate::id::CommanderId;
use crate::input::{CommanderInput, CommanderInputKind, OperatorInput, TickInputs};
use crate::rng::{StreamKey, purpose};
use crate::sensors::VisibilityEvent;
use crate::state::{ScheduledEvent, UniverseState};
use crate::time::{Tick, Ticks};

/// What one tick produced.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TickOutputs {
    /// The tick that was computed.
    pub tick: Tick,
    /// Timers that fired.
    pub timers_fired: u32,
    /// Inputs that were invalid and had no effect, commands included.
    pub inputs_rejected: u32,
    /// The outcome of every command, in the order they were applied (doc 20 §5).
    pub commands: Vec<CommandResult>,
    /// What happened to fleets (S3 plan §4.4).
    pub fleet_events: Vec<FleetEvent>,
    /// What combat did: damage, destroyed ships and fleets (S5 plan §4.3).
    pub combat_events: Vec<CombatEvent>,
    /// What the economy did (S4 plan §4.8).
    pub economy_events: Vec<EconomyEvent>,
    /// What station combat did: sector and module damage, breaches, cores down (S5b).
    pub defence_events: Vec<DefenceEvent>,

    // S6: offline and siege rules.
    /// What commanders are told: window moves and lapsed orders (S6 plan §4.8), and control,
    /// standings, the near-win warning and the season's end (S8 plan §4.5).
    pub announcements: Vec<Announcement>,

    // S7: sensors and fog of war.
    /// What came into and went out of each observer's view, and observed departures
    /// (S7 plan §4.3, §4.5).
    pub visibility_events: Vec<VisibilityEvent>,

    // A1: the AI.
    /// What each commander's Deputy reports for the away report: deferred replacements
    /// (doc 12 §5.3; A1 plan §5.7, §5.8).
    pub deputy_events: Vec<(CommanderId, nomad_ai::DeputyEvent)>,
}

/// The outcome of one command input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandResult {
    /// Who sent it.
    pub commander: CommanderId,
    /// Its client sequence number.
    pub client_seq: u64,
    /// Applied on this tick, or rejected with the validator's reason (D-077).
    pub outcome: Result<(), CommandRejection>,
}

/// Computes the next tick: applies `inputs`, fires due timers and advances the tick counter.
///
/// Phase order follows Design doc 11 §4.1. Phase 1 ends by bringing commanders' presence up
/// to date, phase 2 also fires siege deadlines (exits, siege ends, emergency-jump arrivals;
/// S6b) and then commander deadlines (window closes, grace ends; S6a), phase 5 (movement and
/// transit) runs ships and fleets (S3), phase 6 the sensors on every fifth tick and observed
/// departures on every tick (S7), phase 7 ship and station combat (S5a, S5b) and then the
/// breaches and cores down it caused (S6b), phase 8 the economy (S4) and phase 9 the activity
/// samples (S6a) and the standings, the early win and the season end (S8), then the inbox
/// (A1). Phase 1 applies due AI commands after the external inputs, and phases 3–4 run the
/// Strategists and the Deputies (A1; D-200, D-203). The rest of phase 9 arrives with its work
/// package (D-051). Once the season's result is fixed, `step` changes nothing (D-116).
pub fn step(state: &mut UniverseState, inputs: &TickInputs) -> TickOutputs {
    // The season is over (D-116): no tick follows the final one. Nothing changes, and every
    // input counts as rejected (S8 plan §4.4).
    if state.season_over() {
        let inputs = inputs
            .operator
            .len()
            .saturating_add(inputs.commanders.len());
        return TickOutputs {
            tick: state.tick,
            inputs_rejected: u32::try_from(inputs).unwrap_or(u32::MAX),
            ..TickOutputs::default()
        };
    }
    let now = state.tick.next();
    let mut out = TickOutputs {
        tick: now,
        ..TickOutputs::default()
    };

    // Phase 1: ingest inputs. Operator commands in journal order, then commander inputs in
    // (commander, client_seq) order, whatever order the network delivered them in.
    for op in &inputs.operator {
        if !apply_operator(state, now, op) {
            out.inputs_rejected = out.inputs_rejected.saturating_add(1);
        }
    }
    // Presence can change only through these inputs (S6a).
    let mut presence_inputs = !inputs.operator.is_empty();
    let mut commander_inputs: Vec<&CommanderInput> = inputs.commanders.iter().collect();
    commander_inputs.sort_by_key(|input| (input.commander, input.client_seq));
    for input in commander_inputs {
        let applied = match &input.kind {
            CommanderInputKind::Command(command) => {
                let outcome = apply_command(state, input.commander, command, &mut out);
                out.commands.push(CommandResult {
                    commander: input.commander,
                    client_seq: input.client_seq,
                    outcome,
                });
                outcome.is_ok()
            }
            CommanderInputKind::Connected | CommanderInputKind::Disconnected => {
                presence_inputs = true;
                apply_commander(state, input)
            }
        };
        if !applied {
            out.inputs_rejected = out.inputs_rejected.saturating_add(1);
        }
    }
    // Who is online after this tick's inputs (S6 plan §4.1).
    if presence_inputs {
        crate::commander::sync_presence(state, now);
    }
    // Due AI commands, after the external inputs, within each owner's rate (D-200, D-201).
    crate::ai::gate::apply_due(state, now, &mut out);

    // Phase 2: fire every timer due at or before this tick, in (deadline, SimId) order.
    while let Some((_id, _deadline, event)) = state.timers.pop_due(now) {
        fire(state, now, &event);
        out.timers_fired = out.timers_fired.saturating_add(1);
    }
    // Siege deadlines: exits, siege ends and emergency-jump arrivals (S6 plan §4.4, §4.6).
    // Before the commanders', so a siege that ends as its window closes is over by then.
    crate::siege::fire_due(state, now, &mut out);
    // Commander deadlines: window closes and grace ends (S6 plan §4.2, §4.3).
    crate::commander::fire_due(state, now, &mut out);

    // Phases 3–4: the Strategists of online AI commanders, then every Deputy (D-203). What
    // they issue waits in the outbox for a later tick (D-200).
    crate::ai::run(state, now, &mut out.deputy_events);

    // Phase 5: movement and the jump-transit queue (S3 plan §4.3, §4.4). On a sensors tick,
    // where fleets were before it moves them: what observers last saw (S7 plan §4.4).
    let anchors = crate::sensors::before_movement(state, now);
    crate::fleet::movement::run(state, now, &mut out.fleet_events);

    // Phase 6: sensors and visibility, 2 Hz (D-173), and departures seen (D-179).
    crate::sensors::run(
        state,
        now,
        anchors.as_ref(),
        &out.fleet_events,
        &mut out.visibility_events,
    );

    // Phase 7: ship combat (S5 plan §4.2–§4.4) and station combat (§4.5–§4.8).
    crate::combat::run(
        state,
        now,
        &mut out.combat_events,
        &mut out.defence_events,
        &mut out.economy_events,
    );
    // The breaches and cores down of this tick's combat (S6 plan §4.4–§4.6).
    crate::siege::after_combat(state, now, &mut out);
    // Phase 8: the economy, each holding once a second in its slot (S4 plan §4.1).
    crate::economy::run(state, now, &mut out.economy_events, &mut out.fleet_events);
    // Phase 9: activity samples, once a minute per commander (D-154).
    crate::commander::sample_activity(state, now);
    // Phase 9: the standings on a tick that changed holdings, then the early win and the
    // season's final tick (S8; D-191, D-192, D-195).
    crate::season::run(state, now, &mut out.announcements);
    // Direct control and overrides of fleets and holdings that are gone (A1 plan §5.2).
    crate::doctrine::prune(state);
    // What each owner may know of this tick, for its AI layers (D-202).
    crate::ai::inbox::collect(state, now, &out);

    state.tick = now;
    out
}

/// Returns `false` if the input was invalid.
fn apply_operator(state: &mut UniverseState, now: Tick, op: &OperatorInput) -> bool {
    match *op {
        OperatorInput::SpawnDiagnostics {
            count,
            mean_interval,
        } => {
            for _ in 0..count {
                schedule_diagnostic(state, now, mean_interval);
            }
            true
        }
        // Catch-up mode (doc 11 §7, D-047). A marker that does not change the mode is
        // rejected, so a server bug shows up in the outputs instead of passing silently.
        OperatorInput::CatchUpStart => !std::mem::replace(&mut state.catching_up, true),
        OperatorInput::CatchUpEnd => std::mem::replace(&mut state.catching_up, false),
        // AI presence until A5's schedules (S6 plan §4.1).
        OperatorInput::SetPresence { commander, online } => {
            crate::commander::set_presence(state, commander, online)
        }
    }
}

/// Returns `false` if the input was invalid.
fn apply_commander(state: &mut UniverseState, input: &CommanderInput) -> bool {
    match input.kind {
        CommanderInputKind::Connected => state.connected.insert(input.commander),
        CommanderInputKind::Disconnected => state.connected.remove(&input.commander),
        CommanderInputKind::Command(_) => false,
    }
}

/// Validates and carries out a commander's command (D-077).
fn apply_command(
    state: &mut UniverseState,
    commander: CommanderId,
    command: &Command,
    out: &mut TickOutputs,
) -> Result<(), CommandRejection> {
    execute(
        state,
        Owner::Commander(commander),
        CommandSource::Client,
        command,
        &mut out.fleet_events,
        &mut out.economy_events,
    )
}

fn fire(state: &mut UniverseState, now: Tick, event: &ScheduledEvent) {
    match *event {
        ScheduledEvent::Diagnostic { mean_interval } => {
            schedule_diagnostic(state, now, mean_interval);
        }
    }
}

/// Schedules a diagnostic timer 1 to `2 × mean_interval` ticks after `now`.
fn schedule_diagnostic(state: &mut UniverseState, now: Tick, mean_interval: Ticks) {
    let spread = mean_interval.get().saturating_mul(2);
    let delay = 1 + state
        .rng
        .below(StreamKey::global(purpose::DIAGNOSTICS), spread);
    state.timers.schedule(
        &mut state.ids,
        now.after(Ticks::new(delay)),
        ScheduledEvent::Diagnostic { mean_interval },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CommanderId, SeasonConfig};

    fn universe() -> UniverseState {
        UniverseState::new(SeasonConfig::new(1, 0))
    }

    #[test]
    fn tick_advances_by_one() {
        let mut state = universe();
        let out = step(&mut state, &TickInputs::default());
        assert_eq!(out.tick, Tick::new(1));
        assert_eq!(state.tick(), Tick::new(1));
    }

    #[test]
    fn connect_and_disconnect_are_tracked_and_duplicates_rejected() {
        let mut state = universe();
        let input = |seq, kind| CommanderInput {
            commander: CommanderId(3),
            client_seq: seq,
            kind,
        };
        let out = step(
            &mut state,
            &TickInputs {
                operator: vec![],
                commanders: vec![
                    input(2, CommanderInputKind::Connected),
                    input(1, CommanderInputKind::Connected),
                ],
            },
        );
        assert_eq!(out.inputs_rejected, 1);
        assert!(state.connected().contains(&CommanderId(3)));
        step(
            &mut state,
            &TickInputs {
                operator: vec![],
                commanders: vec![input(3, CommanderInputKind::Disconnected)],
            },
        );
        assert!(state.connected().is_empty());
    }

    #[test]
    fn catch_up_markers_set_and_clear_the_mode() {
        let mut state = universe();
        let operator = |ops: Vec<OperatorInput>| TickInputs {
            operator: ops,
            commanders: vec![],
        };
        let out = step(&mut state, &operator(vec![OperatorInput::CatchUpStart]));
        assert_eq!(out.inputs_rejected, 0);
        assert!(state.catching_up());
        // Starting twice is rejected and changes nothing.
        let out = step(&mut state, &operator(vec![OperatorInput::CatchUpStart]));
        assert_eq!(out.inputs_rejected, 1);
        assert!(state.catching_up());
        let out = step(&mut state, &operator(vec![OperatorInput::CatchUpEnd]));
        assert_eq!(out.inputs_rejected, 0);
        assert!(!state.catching_up());
        let out = step(&mut state, &operator(vec![OperatorInput::CatchUpEnd]));
        assert_eq!(out.inputs_rejected, 1);
        assert!(!state.catching_up());
    }

    #[test]
    fn diagnostics_reschedule_themselves() {
        let mut state = universe();
        step(
            &mut state,
            &TickInputs {
                operator: vec![OperatorInput::SpawnDiagnostics {
                    count: 50,
                    mean_interval: Ticks::new(5),
                }],
                commanders: vec![],
            },
        );
        let mut fired = 0;
        for _ in 0..100 {
            fired += step(&mut state, &TickInputs::default()).timers_fired;
        }
        assert_eq!(state.pending_timers(), 50);
        assert!(fired > 50 * 5, "each timer fires many times in 100 ticks");
    }
}
