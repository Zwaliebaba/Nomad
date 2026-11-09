//! Presence, the disconnect grace and the lapse of direct orders (Design doc 12 §5.3, PR-3;
//! doc 11 §6, §7; D-047, D-080, D-154, D-160 to D-162; S6 plan §4.1, §4.2).
//!
//! - **Online** means: connected (the journaled `Connected` and `Disconnected` inputs, which
//!   the server's sessions send, doc 11 §6), or for an AI scheduled online; during catch-up
//!   nobody (D-047). A `Connected` input for an AI commander, which only tests send, counts
//!   as online too.
//! - **An AI's schedule** (D-080, D-204) is its seeded weekly schedule, followed in phase 1
//!   at each of its changes ([`follow_schedules`]), so it needs no journal: replaying the
//!   inputs reproduces it. The harness's `SetPresence` overrides it for the rest of the
//!   season ("always online", "always offline"); an AI created without a schedule follows
//!   only `SetPresence`.
//! - Phase 1 brings presence up to date after the tick's inputs, so a disconnect and a
//!   reconnect in the same tick change nothing.
//! - **Going offline** starts the 120 s grace, a deadline (doc 12 §5.3). Coming back online
//!   before it ends cancels it.
//! - **When the grace ends** the commander's direct fleet orders lapse (D-162): every fleet
//!   under direct control (A1 plan §5.2) leaves it, and each of them with an order or a queue
//!   drops them and station-keeps where it is, until its doctrine role moves it (A1b). Orders
//!   the Deputy gave are doctrine and stand. A fleet that already station-keeps where it is, with nothing
//!   queued, is at that doctrine and has nothing to lose. A fleet in lane transit finishes its hop first (D-042) and
//!   station-keeps on arrival; a docked fleet stays docked. Convoy routes are doctrine and
//!   continue (doc 12 §5.2, D-162). Rules of engagement are a standing setting of the fleet,
//!   not an order, and stay (*Proposed*: D-162 does not say). The owner is told which fleets
//!   lapsed (the away report's "orders that lapsed", S6 plan §4.8).
//! - **Counts as online for sieges** (D-160): online, or offline for less than the grace;
//!   during catch-up nobody, so a siege that falls due then is Held (doc 11 §7, D-047).

use nomad_types::{CommanderKind, FleetOrder, Queueing};

use super::{Deadline, Presence};
use crate::announce::{Announcement, AnnouncementKind, Recipients};
use crate::fleet::{Fleet, FleetEvent, FleetId, Place};
use crate::id::CommanderId;
use crate::state::UniverseState;
use crate::time::{Tick, Ticks};

/// The disconnect grace (doc 12 §5.3).
fn grace(state: &UniverseState) -> Ticks {
    Ticks::from_secs(state.rules.sieges.disconnect_grace_s)
}

/// Whether `commander` is online by the inputs applied so far.
fn online_now(state: &UniverseState, commander: CommanderId) -> bool {
    let Some(record) = state.commanders.records.get(&commander) else {
        return false;
    };
    !state.catching_up
        && (state.connected.contains(&commander)
            || (record.kind == CommanderKind::Ai && record.scheduled_online))
}

/// Phase 1, after the inputs: every commander whose presence changed comes online or goes
/// offline on `now`.
pub(crate) fn sync_presence(state: &mut UniverseState, now: Tick) {
    let grace = grace(state);
    for commander in state.commander_ids() {
        let online = online_now(state, commander);
        let Some(record) = state.commanders.records.get_mut(&commander) else {
            continue;
        };
        match (record.presence, online) {
            (Presence::Offline { .. }, true) => {
                record.presence = Presence::Online { since: now };
                // Back before the grace ended: the orders stand.
                if let Some(until) = record.grace_until.take() {
                    state
                        .commanders
                        .deadlines
                        .cancel(until, commander, Deadline::GraceEnds);
                }
            }
            (Presence::Online { .. }, false) => {
                record.presence = Presence::Offline { since: now };
                let until = now.after(grace);
                record.grace_until = Some(until);
                state
                    .commanders
                    .deadlines
                    .schedule(until, commander, Deadline::GraceEnds);
            }
            _ => {}
        }
    }
}

/// Phase 1, before presence is brought up to date: every AI whose seeded schedule changes
/// on `now` follows it (D-080, D-204). Returns whether any did.
pub(crate) fn follow_schedules(state: &mut UniverseState, now: Tick) -> bool {
    let clock = state.clock;
    let mut changed = false;
    for record in state.commanders.records.values_mut() {
        if record.presence_overridden || record.next_schedule_change.is_none_or(|t| t > now) {
            continue;
        }
        let Some(schedule) = &record.schedule else {
            record.next_schedule_change = None;
            continue;
        };
        let at = clock.universe_time_ms(now);
        let online = schedule.session_at(at).is_some();
        record.next_schedule_change = schedule
            .next_change(at)
            .map(|ms| clock.first_tick_at_or_after(ms).max(now.next()));
        if record.scheduled_online != online {
            record.scheduled_online = online;
            changed = true;
        }
    }
    changed
}

/// The operator input `SetPresence` (S6 plan §4.1): the harness overrides an AI commander's
/// schedule, from now on, with online or offline (D-204). Returns `false`, rejecting the
/// input, for a human or unknown commander, or if nothing changes.
pub(crate) fn set_presence(
    state: &mut UniverseState,
    commander: CommanderId,
    online: bool,
) -> bool {
    match state.commanders.records.get_mut(&commander) {
        Some(record)
            if record.kind == CommanderKind::Ai
                && (record.scheduled_online != online || !record.presence_overridden) =>
        {
            let changed = record.scheduled_online != online || record.schedule.is_some();
            record.scheduled_online = online;
            record.presence_overridden = true;
            record.next_schedule_change = None;
            changed
        }
        _ => false,
    }
}

/// The session times of `commander`'s schedule at `now`, for its Strategist (D-214): when
/// the open session ends, when the next one starts, and its longest session in ticks. None are known while `SetPresence`
/// overrides the schedule or without one.
pub(crate) fn session_times(
    state: &UniverseState,
    commander: CommanderId,
    now: Tick,
) -> (Option<Tick>, Option<Tick>, Option<u64>) {
    let Some(record) = state.commanders.records.get(&commander) else {
        return (None, None, None);
    };
    let Some(schedule) = record
        .schedule
        .as_ref()
        .filter(|_| !record.presence_overridden)
    else {
        return (None, None, None);
    };
    let longest = schedule
        .weekday
        .iter()
        .chain(&schedule.weekend)
        .map(|s| Ticks::from_minutes(u64::from(s.length_min)).get())
        .max();
    let clock = state.clock;
    let at = clock.universe_time_ms(now);
    let ends = schedule
        .session_at(at)
        .map(|(_, end)| clock.first_tick_at_or_after(end));
    let next = schedule
        .next_session(at)
        .map(|(start, _)| clock.first_tick_at_or_after(start));
    (ends, next, longest)
}

/// Whether `commander` counts as online for siege rules on `now`: online, or offline for less
/// than the grace; nobody during catch-up (D-160, D-047).
pub(crate) fn counts_as_online(state: &UniverseState, commander: CommanderId, now: Tick) -> bool {
    if state.catching_up {
        return false;
    }
    state
        .commanders
        .records
        .get(&commander)
        .is_some_and(|record| match record.presence {
            Presence::Online { .. } => true,
            // A commander who never was online has no grace.
            Presence::Offline { .. } => record.grace_until.is_some_and(|until| now < until),
        })
}

/// The grace of `commander` ends on `now` and they are still offline (a return would have
/// cancelled the deadline): their direct fleet orders lapse (D-162).
pub(super) fn grace_ends(
    state: &mut UniverseState,
    now: Tick,
    commander: CommanderId,
    fleet_events: &mut Vec<FleetEvent>,
    announcements: &mut Vec<Announcement>,
) {
    let Some(record) = state.commanders.records.get_mut(&commander) else {
        return;
    };
    record.grace_until = None;
    if record.presence.is_online() {
        return;
    }
    // The fleets under direct control return to doctrine (D-162, RK-10); those with orders
    // lose them.
    let mut lapsed = Vec::new();
    for id in crate::doctrine::lapse_all(state, commander) {
        // Convoy routes are doctrine, not direct orders (doc 12 §5.2, D-162).
        if state.economy.convoys.contains_key(&id) {
            continue;
        }
        let Some((place, has_orders)) = state.entities.with::<Fleet, _>(id.sim_id(), |f| {
            let at_doctrine = f
                .active
                .as_ref()
                .is_none_or(|active| active.order == FleetOrder::StationKeep { relative_to: None });
            (f.place, !at_doctrine || !f.queue.is_empty())
        }) else {
            continue;
        };
        if !has_orders {
            continue;
        }
        lapse(state, id, place, fleet_events);
        lapsed.push(id);
    }
    if !lapsed.is_empty() {
        announcements.push(Announcement {
            tick: now,
            to: Recipients::Commanders(vec![commander]),
            kind: AnnouncementKind::OrdersLapsed {
                commander,
                fleets: lapsed,
            },
        });
    }
}

/// Drops `fleet`'s direct orders: it station-keeps where it is, or where its lane hop ends;
/// docked, it stays docked (D-162).
fn lapse(state: &mut UniverseState, fleet: FleetId, place: Place, events: &mut Vec<FleetEvent>) {
    match place {
        Place::InSystem(_) | Place::InTransit { .. } => crate::fleet::orders::give(
            state,
            fleet,
            FleetOrder::StationKeep { relative_to: None },
            Queueing::Replace,
            events,
        ),
        // An emergency jump cannot be interrupted, and it has no orders (S6b).
        Place::Docked(_) | Place::EmergencyJump { .. } => {
            if let Some(f) = state.entities.get_mut::<Fleet>(fleet.sim_id()) {
                f.active = None;
                f.queue.clear();
            }
        }
    }
}
