//! Breach, exit, siege, Held and the start of Fallen (doc 12 §4; doc 15 §6; D-015, D-033,
//! D-057, D-158 to D-161, D-163, D-229; S6 plan §4.4).

use nomad_types::Owner;

use super::{Deadline, DefenceState, HoldingSiege, capture, jump, owner_and_attackers};
use crate::announce::{Announcement, AnnouncementKind, HeldReason};
use crate::commander::{DEFAULT_WINDOW_START, counts_as_online};
use crate::holding::HoldingId;
use crate::id::CommanderId;
use crate::rng::{StreamKey, purpose};
use crate::state::UniverseState;
use crate::step::TickOutputs;
use crate::time::{Tick, Ticks};

const MILLIS_PER_DAY: i64 = 86_400_000;
/// How many days ahead an exit is looked for. A commander's pending exits fill 4 slots a day
/// (D-033), so this bounds the search far beyond any number of holdings.
const MAX_DAYS: i64 = 100_000;

/// A sector of `holding`, `sector`, reached 0 under a hostile hit on `now`: the holding is
/// Reinforced until its exit (D-015, D-057, D-158).
pub(super) fn breach(
    state: &mut UniverseState,
    now: Tick,
    holding: HoldingId,
    sector: u8,
    out: &mut TickOutputs,
) {
    // A second sector breached in the same tick finds the holding Reinforced already.
    if !super::breachable(state, holding, now) {
        return;
    }
    let Some(owner) = state.holdings.get(&holding).map(|h| h.owner) else {
        return;
    };
    // D-057: every commander whose ships damaged the dome in the 10 minutes before. NPC
    // raiders do no dome damage, so they never get credit (D-056).
    let credit = Ticks::from_secs(state.rules.sieges.breach_credit_s);
    let since = Tick::new(now.get().saturating_sub(credit.get()));
    let attackers: Vec<CommanderId> = state
        .dome_attackers(holding, since)
        .into_iter()
        .filter(|&c| c != owner)
        .collect();
    state.set_dome_immune(holding, true);
    let exit = exit_slot(state, owner, now);
    state.sieges.holdings.entry(holding).or_default().state = DefenceState::Reinforced {
        breached: now,
        sector,
        exit,
        attackers: attackers.clone(),
    };
    state
        .sieges
        .deadlines
        .insert((exit, Deadline::Exit(holding)));
    out.announcements.push(Announcement {
        tick: now,
        to: owner_and_attackers(owner, &attackers),
        kind: AnnouncementKind::Breached {
            holding,
            sector,
            exit_tick: exit,
            attackers,
        },
    });
}

/// The exit of a holding of `owner` breached on `breach` (D-158): the earliest stagger slot,
/// the window's start plus 0, 1, 2 or 3 × 60 min (D-033), that is at least 12 h after the
/// breach and later than the exits of the owner's earlier-breached pending holdings. The
/// window is the owner's as it is at the breach; the freeze keeps it there (D-156).
pub(super) fn exit_slot(state: &UniverseState, owner: CommanderId, breach: Tick) -> Tick {
    let rules = &state.rules.sieges;
    let earliest = breach.after(Ticks::from_minutes(u64::from(rules.notice_minutes)));
    let latest = state
        .sieges
        .holdings
        .iter()
        .filter(|(id, _)| state.holdings.get(id).is_some_and(|h| h.owner == owner))
        .filter_map(|(_, h)| match h.state {
            DefenceState::Reinforced { exit, .. } => Some(exit),
            DefenceState::Nominal | DefenceState::Siege { .. } => None,
        })
        .max();
    let start = state
        .commanders
        .records
        .get(&owner)
        .map_or(DEFAULT_WINDOW_START, |r| r.window.start);
    let clock = state.clock;
    let stagger_ms = i64::from(rules.stagger_minutes) * 60_000;
    let slots = i64::from(rules.slots_per_window());
    let earliest_ms = clock.universe_time_ms(earliest);
    // From the window that opened the day before, which may still have a slot to give.
    let mut day = earliest_ms - earliest_ms.rem_euclid(MILLIS_PER_DAY) - MILLIS_PER_DAY;
    for _ in 0..MAX_DAYS {
        let opens_ms = day + start.time_of_day_ms();
        for k in 0..slots {
            let slot = clock.first_tick_at_or_after(opens_ms + k * stagger_ms);
            if slot >= earliest && latest.is_none_or(|l| slot > l) {
                return slot;
            }
        }
        day += MILLIS_PER_DAY;
    }
    earliest
}

/// The exit of `holding` falls due on `now`: the siege starts if an attacker counts as
/// online, otherwise the holding is Held at once (D-057, D-160; during catch-up nobody is
/// online, D-047).
pub(super) fn exit(
    state: &mut UniverseState,
    now: Tick,
    holding: HoldingId,
    out: &mut TickOutputs,
) {
    let Some(HoldingSiege {
        state:
            DefenceState::Reinforced {
                breached,
                sector,
                exit,
                attackers,
            },
        ..
    }) = state.sieges.holdings.get(&holding).cloned()
    else {
        return;
    };
    if exit > now {
        return;
    }
    if !attackers.iter().any(|&a| counts_as_online(state, a, now)) {
        held(state, now, holding, HeldReason::NoAttackerOnline, out);
        return;
    }
    // Only the breached sector opens; the others behave normally (D-057, doc 15 §6).
    if !state.open_sector(holding, sector) {
        held(state, now, holding, HeldReason::Clock, out);
        return;
    }
    state.set_dome_immune(holding, false);
    state.reset_siege_tally(holding, now);
    let ends = now.after(Ticks::from_minutes(u64::from(
        state.rules.sieges.siege_minutes,
    )));
    let Some(owner) = state.holdings.get(&holding).map(|h| h.owner) else {
        return;
    };
    state.sieges.holdings.entry(holding).or_default().state = DefenceState::Siege {
        breached,
        sector,
        started: now,
        ends,
        attackers: attackers.clone(),
    };
    state
        .sieges
        .deadlines
        .insert((ends, Deadline::SiegeEnds(holding)));
    out.announcements.push(Announcement {
        tick: now,
        to: owner_and_attackers(owner, &attackers),
        kind: AnnouncementKind::SiegeStarted {
            holding,
            end_tick: ends,
            open_sector: sector,
        },
    });
}

/// The siege clock of `holding` runs out on `now` with the core standing: Held (D-015).
pub(super) fn siege_ends(
    state: &mut UniverseState,
    now: Tick,
    holding: HoldingId,
    out: &mut TickOutputs,
) {
    if matches!(
        state.sieges.holdings.get(&holding).map(|h| &h.state),
        Some(DefenceState::Siege { ends, .. }) if *ends <= now
    ) {
        held(state, now, holding, HeldReason::Clock, out);
    }
}

/// `holding` held (D-015, D-057): every sector is raised to at least 50%, nothing stays open
/// or immune, and it cannot be breached for 48 h.
pub(super) fn held(
    state: &mut UniverseState,
    now: Tick,
    holding: HoldingId,
    reason: HeldReason,
    out: &mut TickOutputs,
) {
    let Some(record) = state.sieges.holdings.get(&holding).cloned() else {
        return;
    };
    if let DefenceState::Siege { ends, .. } = record.state {
        state
            .sieges
            .deadlines
            .remove(&(ends, Deadline::SiegeEnds(holding)));
    }
    let rules = &state.rules.sieges;
    let (percent, protection) = (
        rules.held_restore_percent,
        Ticks::from_hours(u64::from(rules.held_protection_hours)),
    );
    state.close_sectors(holding);
    state.set_dome_immune(holding, false);
    state.restore_sectors(holding, percent);
    let breachable_from = now.after(protection);
    state.sieges.holdings.insert(
        holding,
        HoldingSiege {
            state: DefenceState::Nominal,
            breachable_from,
        },
    );
    let Some(owner) = state.holdings.get(&holding).map(|h| h.owner) else {
        return;
    };
    out.announcements.push(Announcement {
        tick: now,
        to: owner_and_attackers(owner, record.state.attackers()),
        kind: AnnouncementKind::Held {
            holding,
            reason,
            breachable_from,
        },
    });
}

/// The core of `holding` reached 0 under `by`'s hit on `now`. An outpost under construction
/// is destroyed, at any time (D-054). A holding under siege has fallen (D-057): captured, or
/// a mother station emergency-jumps (D-017, D-034). Outside a siege a completed holding's
/// core cannot be hit (doc 15 §6), and S5b leaves it at 0.
pub(super) fn core_down(
    state: &mut UniverseState,
    now: Tick,
    holding: HoldingId,
    by: Owner,
    out: &mut TickOutputs,
) {
    let Some(h) = state.holdings.get(&holding).cloned() else {
        return;
    };
    if !h.core_complete {
        capture::destroy_outpost(state, now, holding, out);
        return;
    }
    if !matches!(
        state.sieges.holdings.get(&holding).map(|r| &r.state),
        Some(DefenceState::Siege { .. })
    ) {
        return;
    }
    let Some(capturer) = capturer(state, holding, h.owner, by) else {
        held(state, now, holding, HeldReason::Clock, out);
        return;
    };
    if h.mother_station {
        jump::mother_falls(state, now, holding, capturer, out);
    } else {
        capture::capture(state, now, holding, capturer, out);
    }
}

/// Who captures `holding` (D-057, D-163, D-229): of every commander hostile to `owner` in the
/// siege tally, the most core damage; then the larger total damage to the holding; then a
/// draw from the season's random stream for this holding. `by`, the commander whose hit
/// brought the core down, if the tally is somehow empty.
pub(super) fn capturer(
    state: &mut UniverseState,
    holding: HoldingId,
    owner: CommanderId,
    by: Owner,
) -> Option<CommanderId> {
    let tally = state
        .damage_ledger(holding)
        .map(|l| l.tally)
        .unwrap_or_default();
    let best = tally
        .iter()
        .filter(|(c, _)| *c != owner)
        .map(|(_, t)| (t.core_milli, t.total_milli))
        .max();
    let tied: Vec<CommanderId> = tally
        .iter()
        .filter(|(c, t)| *c != owner && Some((t.core_milli, t.total_milli)) == best)
        .map(|&(c, _)| c)
        .collect();
    match tied.as_slice() {
        [] => by.commander().filter(|&c| c != owner),
        [one] => Some(*one),
        _ => {
            let key = StreamKey {
                scope: holding.sim_id().get(),
                purpose: purpose::SIEGE_TIES,
            };
            let pick = state.rng.index(key, tied.len());
            tied.get(pick).copied()
        }
    }
}
