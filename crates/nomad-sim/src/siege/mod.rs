//! The siege lifecycle (Design doc 12 §3–§4; doc 15 §5.1, §6–§8; S6 plan §4.4–§4.8; S6b).
//!
//! A holding's core can only be lost inside its owner's exposure window, after at least 12 h
//! of notice, in a scheduled 60-minute siege that needs an online attacker (D-015). This
//! module makes that true, on top of S5b's dome ([`crate::defence`]) and S6a's commanders
//! ([`crate::commander`]).
//!
//! ```text
//! Nominal ──breach──► Reinforced{exit} ──exit, attacker online──► Siege{ends}
//!                         │                                         │      │
//!                         └──exit, no attacker online──► Held ◄─clock─┘      core 0
//!                                                        │                   ▼
//!                              protection 48 h ◄─────────┘                Fallen
//!                                                                 capture or emergency jump
//! ```
//!
//! - **Breach** ([`lifecycle`]; D-015, D-057, D-158, D-159): S5b reports a sector taken to
//!   0 by a hostile hit, only while the holding is breachable: Nominal and not protected
//!   ([`breachable`], the S5b hook `breach_may_occur`). The attackers are the commanders
//!   whose ships damaged the dome in the 10 minutes before (the ledger). The dome becomes
//!   immune, and the exit is the earliest stagger slot (the window's start plus 0, 1, 2 or
//!   3 h) at least 12 h after the breach and later than the owner's other pending exits.
//! - **The exit** (D-057, D-160): with no attacker counting as online (connected or inside
//!   the 120 s grace; nobody during catch-up), the holding is Held at once. Otherwise the
//!   breached sector opens, the siege tally starts, and the siege clock runs 60 minutes.
//! - **Held** (D-015, D-057): every sector is raised to at least 50%, nothing stays open or
//!   immune, and the holding cannot be breached for 48 h. Timers (phase 2) run before
//!   combat (phase 7), so core damage on the clock's last tick does not count.
//! - **Fallen** ([`capture`], [`jump`]; D-017, D-034, D-057, D-163 to D-168): the core
//!   reaches 0 during the siege. The commander with the most core damage captures, ties to
//!   the larger total and then to a draw from the season's random streams (D-229). A
//!   normal holding changes owner; a mother station emergency-jumps and leaves its site to
//!   the capturer as an outpost.
//! - **Protection** is one `breachable_from` tick per holding, public (D-170): 48 h after
//!   Held, 48 h for a capturer, 72 h after an emergency jump. Sectors can still be worn to
//!   0 during it, without a breach; the next hostile hit after it is one (D-159).
//! - **Outposts** under construction are destroyed when their core reaches 0, at any time
//!   (D-054).
//! - **Peripheral structures** (solar arrays, harvesters, extractors; D-138) at 0 hull are
//!   destroyed inside their owner's window and disabled outside it (doc 12 §3); a disabled
//!   one hit again inside the window is destroyed (D-169); NPC raiders only disable, at any
//!   time (D-223). [`peripheral_outcome`] is the rule S5b's `structure_outcome_at_zero` asks.
//! - **The window freeze** (D-156): [`defence_pending`] answers S6a's question.
//!
//! **Phases** (doc 11 §4.1): deadlines (exits, siege ends, emergency-jump arrivals) fire in
//! phase 2 after the Universe's timers and before the commanders' (a siege that ends as the
//! window closes is over before the window may move); breaches and cores down are handled in
//! phase 7, right after the combat that caused them. Deadlines are absolute ticks keyed by
//! `(tick, kind, holding or fleet)`, so they take no `SimId` (doc 11 §3).

mod capture;
mod grant;
mod jump;
mod lifecycle;
#[cfg(test)]
mod stakes;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use nomad_types::Owner;

use crate::announce::Recipients;
use crate::defence::DefenceEvent;
use crate::defence::hooks::StructureOutcome;
use crate::fleet::FleetId;
use crate::galaxy::SiteId;
use crate::holding::HoldingId;
use crate::id::CommanderId;
use crate::state::UniverseState;
use crate::step::TickOutputs;
use crate::time::Tick;

/// The siege part of the Universe state (S6b).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Sieges {
    /// Holdings with a defence pending or a protection. A holding without a record is
    /// Nominal and breachable.
    pub(crate) holdings: BTreeMap<HoldingId, HoldingSiege>,
    /// Fallen mother stations, on their way or waiting for a free site (D-164, D-166). Each
    /// keeps its holding's identity and lands as that holding again.
    pub(crate) stations: BTreeMap<HoldingId, StationJump>,
    /// Exits, siege ends and emergency-jump arrivals, by deadline (doc 11 §3).
    pub(crate) deadlines: BTreeSet<(Tick, Deadline)>,
}

/// One holding's defence state and protection.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct HoldingSiege {
    pub(crate) state: DefenceState,
    /// The holding cannot be breached before this tick (D-015, D-034, D-057, D-170).
    pub(crate) breachable_from: Tick,
}

/// A holding's defence state (doc 12 §4; S6 plan §4.4). Held and Fallen are outcomes, not
/// states: Held returns to Nominal with a protection, Fallen to Nominal with a new owner or
/// no holding. Weakened is display only (a sector below 50%, [`crate::SectorView`]).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DefenceState {
    /// Breachable, unless protected.
    #[default]
    Nominal,
    /// Breached: the dome is immune until the exit, when the siege starts (D-015).
    Reinforced {
        /// When the sector reached 0.
        breached: Tick,
        /// The breached sector, which opens at the exit.
        sector: u8,
        /// The exit: the siege starts, or Held if no attacker is online (D-158, D-160).
        exit: Tick,
        /// The commanders with breach credit, in identity order (D-057).
        attackers: Vec<CommanderId>,
    },
    /// The siege: the breached sector is open and the clock runs (D-057).
    Siege {
        /// When the sector reached 0.
        breached: Tick,
        /// The open sector.
        sector: u8,
        /// When the siege started: the exit.
        started: Tick,
        /// When the clock runs out.
        ends: Tick,
        /// The commanders with breach credit (D-057).
        attackers: Vec<CommanderId>,
    },
}

impl DefenceState {
    /// Whether the holding is Reinforced or under siege: a defence is pending (D-156).
    pub const fn is_pending(&self) -> bool {
        !matches!(self, Self::Nominal)
    }

    /// The commanders with breach credit, if a defence is pending.
    pub fn attackers(&self) -> &[CommanderId] {
        match self {
            Self::Nominal => &[],
            Self::Reinforced { attackers, .. } | Self::Siege { attackers, .. } => attackers,
        }
    }
}

/// What falls due at a siege deadline, in this order within a tick: a siege's end before an
/// exit, and a station's arrival before the fleets that jumped with it, which dock at it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) enum Deadline {
    /// The siege clock runs out: Held (D-015).
    SiegeEnds(HoldingId),
    /// Reinforced ends: the siege starts, or Held (D-057, D-160).
    Exit(HoldingId),
    /// A fallen mother station's emergency jump arrives (D-166).
    StationArrives(HoldingId),
    /// A fleet's emergency jump arrives (D-166).
    FleetArrives(FleetId),
}

/// A fallen mother station between its fall and its landing (S6 plan §4.6).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct StationJump {
    pub(crate) owner: CommanderId,
    /// The site it fell on, now the capturer's outpost.
    pub(crate) from_site: SiteId,
    pub(crate) leg: StationLeg,
}

/// Where a fallen mother station is going (D-164 to D-166).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StationLeg {
    /// In its emergency jump to a free site, which is reserved for it, landing at `arrive`.
    ToSite {
        /// The site.
        site: SiteId,
        /// When it lands.
        arrive: Tick,
    },
    /// No site is free: in its emergency jump to the commander's nearest holding, where it
    /// waits for one (D-164).
    ToHolding {
        /// The holding it waits by.
        holding: HoldingId,
        /// When it arrives there.
        arrive: Tick,
    },
    /// Waiting by a holding, or where it fell (`None`) if the commander has none, until a
    /// site frees up (D-164).
    Waiting {
        /// The holding it waits by.
        by: Option<HoldingId>,
    },
}

/// A holding's siege state as seen from outside (S6 plan §4.8). The defence state with its
/// deadlines and attackers goes to the owner and the attackers (D-075); `breachable_from` is
/// public (D-170).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SiegeView {
    /// The holding.
    pub holding: HoldingId,
    /// Nominal, Reinforced or under siege, with the deadlines.
    pub state: DefenceState,
    /// The holding cannot be breached before this tick.
    pub breachable_from: Tick,
}

/// A fallen mother station in its emergency jump, as its owner sees it (S6 plan §4.6). It is
/// invisible to everyone else, like lane transit (D-166).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StationJumpView {
    /// The station: the holding it lands as again.
    pub holding: HoldingId,
    /// Whose it is. It stays their mother station for the command cap (S6 plan §4.6).
    pub owner: CommanderId,
    /// Where it fell.
    pub from_site: SiteId,
    /// Where it is going, or where it waits.
    pub leg: StationLeg,
}

/// Phase 2: the siege deadlines due on `now`, in `(tick, kind, id)` order.
pub(crate) fn fire_due(state: &mut UniverseState, now: Tick, out: &mut TickOutputs) {
    while let Some(&(tick, deadline)) = state.sieges.deadlines.first() {
        if tick > now {
            break;
        }
        state.sieges.deadlines.pop_first();
        match deadline {
            Deadline::SiegeEnds(holding) => lifecycle::siege_ends(state, now, holding, out),
            Deadline::Exit(holding) => lifecycle::exit(state, now, holding, out),
            Deadline::StationArrives(holding) => jump::station_arrives(state, now, holding, out),
            Deadline::FleetArrives(fleet) => jump::fleet_arrives(state, now, fleet, out),
        }
        out.timers_fired = out.timers_fired.saturating_add(1);
    }
}

/// Phase 7, after combat: this tick's breaches and cores down, in the order S5b reported
/// them (doc 15 §4's damage order).
pub(crate) fn after_combat(state: &mut UniverseState, now: Tick, out: &mut TickOutputs) {
    let events: Vec<DefenceEvent> = out
        .defence_events
        .iter()
        .filter(|e| {
            matches!(
                e,
                DefenceEvent::SectorBreached { .. } | DefenceEvent::CoreDown { .. }
            )
        })
        .copied()
        .collect();
    for event in events {
        match event {
            DefenceEvent::SectorBreached {
                holding, sector, ..
            } => lifecycle::breach(state, now, holding, sector, out),
            DefenceEvent::CoreDown { holding, by, .. } => {
                lifecycle::core_down(state, now, holding, by, out);
            }
            DefenceEvent::SectorDamaged { .. } | DefenceEvent::ModuleDamaged { .. } => {}
        }
    }
}

/// Whether a hostile hit that leaves a sector of `holding` at 0 on `now` is a breach: the
/// holding is Nominal and its protection is over (D-015, D-159). The body of S5b's
/// `breach_may_occur` hook.
pub(crate) fn breachable(state: &UniverseState, holding: HoldingId, now: Tick) -> bool {
    state
        .sieges
        .holdings
        .get(&holding)
        .is_none_or(|h| h.state == DefenceState::Nominal && now >= h.breachable_from)
}

/// What becomes of a peripheral structure of `holding` at 0 hull under a hit of `by` on `now`
/// (doc 12 §3; D-138, D-169, D-223): destroyed inside the owner's exposure window, disabled
/// outside it; NPC raiders only disable.
pub(crate) fn peripheral_outcome(
    state: &UniverseState,
    holding: HoldingId,
    by: Owner,
    now: Tick,
) -> StructureOutcome {
    if matches!(by, Owner::Npc(_)) {
        return StructureOutcome::Disabled;
    }
    let inside = state
        .holdings
        .get(&holding)
        .and_then(|h| state.exposure_window(h.owner))
        .is_some_and(|window| window.is_open(now));
    if inside {
        StructureOutcome::Destroyed
    } else {
        StructureOutcome::Disabled
    }
}

/// Whether any of `commander`'s holdings is Reinforced or under siege, which freezes their
/// exposure window (D-156). S6a's `defence_pending` asks this.
pub(crate) fn defence_pending(state: &UniverseState, commander: CommanderId) -> bool {
    state.sieges.holdings.iter().any(|(id, h)| {
        h.state.is_pending() && state.holdings.get(id).is_some_and(|h| h.owner == commander)
    })
}

/// Whether a fallen mother station is jumping to `site`, which is then reserved for it: a
/// core kit cannot be deployed there and no other station chooses it (D-164).
pub(crate) fn site_reserved(state: &UniverseState, site: SiteId) -> bool {
    state
        .sieges
        .stations
        .values()
        .any(|j| matches!(j.leg, StationLeg::ToSite { site: s, .. } if s == site))
}

/// Whether `fleet`'s emergency jump has its arrival scheduled for `arrive`, for the fleet
/// consistency check.
pub(crate) fn jump_pending(state: &UniverseState, fleet: FleetId, arrive: Tick) -> bool {
    state
        .sieges
        .deadlines
        .contains(&(arrive, Deadline::FleetArrives(fleet)))
}

/// Whether `commander`'s fallen mother station is between its fall and its landing: it stays
/// their mother station for the command cap and the pool's base cap meanwhile (S6 plan §4.6;
/// *Derived* for the pool: its base storage is the station's).
pub(crate) fn mother_in_transit(state: &UniverseState, commander: CommanderId) -> bool {
    state.sieges.stations.values().any(|j| j.owner == commander)
}

/// The owner and the attackers with breach credit, in identity order: who hears of a breach,
/// a siege and Held (D-075).
fn owner_and_attackers(owner: CommanderId, attackers: &[CommanderId]) -> Recipients {
    let mut to: BTreeSet<CommanderId> = attackers.iter().copied().collect();
    to.insert(owner);
    Recipients::Commanders(to.into_iter().collect())
}

impl UniverseState {
    /// `holding`'s siege state, or `None` if there is no such holding (S6 plan §4.8).
    pub fn siege(&self, holding: HoldingId) -> Option<SiegeView> {
        self.holdings.get(&holding)?;
        let record = self
            .sieges
            .holdings
            .get(&holding)
            .cloned()
            .unwrap_or_default();
        Some(SiegeView {
            holding,
            state: record.state,
            breachable_from: record.breachable_from,
        })
    }

    /// The tick from which `holding` can be breached: public (D-170). `None` if there is no
    /// such holding.
    pub fn breachable_from(&self, holding: HoldingId) -> Option<Tick> {
        self.holdings.get(&holding)?;
        Some(
            self.sieges
                .holdings
                .get(&holding)
                .map_or(Tick::ZERO, |h| h.breachable_from),
        )
    }

    /// Fallen mother stations between their fall and their landing, in identity order.
    /// Private to each owner (D-166).
    pub fn station_jumps(&self) -> Vec<StationJumpView> {
        self.sieges
            .stations
            .iter()
            .map(|(&holding, j)| StationJumpView {
                holding,
                owner: j.owner,
                from_site: j.from_site,
                leg: j.leg,
            })
            .collect()
    }
}
