//! Advancing the Universe by one tick (Design doc 11 §4).

use crate::input::{CommanderInput, CommanderInputKind, OperatorInput, TickInputs};
use crate::rng::{StreamKey, purpose};
use crate::state::{ScheduledEvent, UniverseState};
use crate::time::{Tick, Ticks};

/// What one tick produced.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TickOutputs {
    /// The tick that was computed.
    pub tick: Tick,
    /// Timers that fired.
    pub timers_fired: u32,
    /// Inputs that were invalid and had no effect.
    pub inputs_rejected: u32,
}

/// Computes the next tick: applies `inputs`, fires due timers and advances the tick counter.
///
/// Phase order follows Design doc 11 §4.1. Phases 3–9 (AI, Deputy, movement, sensors, combat,
/// economy, derived state) arrive with Design docs 13–16; there are no game rules yet (D-051).
pub fn step(state: &mut UniverseState, inputs: &TickInputs) -> TickOutputs {
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
    let mut commander_inputs: Vec<&CommanderInput> = inputs.commanders.iter().collect();
    commander_inputs.sort_by_key(|input| (input.commander, input.client_seq));
    for input in commander_inputs {
        if !apply_commander(state, input) {
            out.inputs_rejected = out.inputs_rejected.saturating_add(1);
        }
    }

    // Phase 2: fire every timer due at or before this tick, in (deadline, SimId) order.
    while let Some((_id, _deadline, event)) = state.timers.pop_due(now) {
        fire(state, now, &event);
        out.timers_fired = out.timers_fired.saturating_add(1);
    }

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
    }
}

/// Returns `false` if the input was invalid.
fn apply_commander(state: &mut UniverseState, input: &CommanderInput) -> bool {
    match input.kind {
        CommanderInputKind::Connected => state.connected.insert(input.commander),
        CommanderInputKind::Disconnected => state.connected.remove(&input.commander),
    }
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
        UniverseState::new(SeasonConfig {
            seed: 1,
            season_start_unix_ms: 0,
        })
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
