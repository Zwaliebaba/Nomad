//! Station combat: dome sectors, regeneration, breaches, damage to modules, turrets and the
//! station abilities (Design doc 15 §5, §12; D-053, D-056, D-100, D-140 to D-142, D-145 to
//! D-149, D-163; S5 plan §4.5–§4.8, S5b).
//!
//! It runs inside phase 7 of the tick, around ship combat ([`crate::combat::run`]):
//!
//! 1. **Upkeep** ([`upkeep`]): every holding whose core is complete projects a dome (doc 15
//!    §5.1, D-141). Each of its four sectors has a maximum of 25% of a full sector from the
//!    core plus 75% from that sector's own working pylon (D-140), kept up to date once a
//!    second in the holding's economy slot, and regenerates every tick from the power S4
//!    allocated it: allocated MW × 50 / 120 points per second (D-142). A new dome starts at
//!    its maximum. An open sector does not regenerate.
//! 2. **Hit tests** ([`Structures`]): a hostile projectile whose swept segment enters a dome's
//!    circle damages the sector whose arc contains the crossing point (D-046, D-100) and
//!    stops; outgoing fire and friendly fire pass (one-way, D-053, D-146). Where the sector is
//!    open it passes, and may hit the modules inside. Modules are circles at their slots
//!    (S4): peripheral ones (solar, harvester, extractor) can always be hit; inner ones only
//!    while a sector of their dome is open, which only S6's siege does (doc 15 §6); pylons
//!    never (D-053); the core of an outpost, which has no dome yet, always (D-054).
//! 3. **Turrets and FLAK SCREEN** fire ([`fire`]): see [`turrets`] and [`abilities`].
//! 4. **Damage** ([`apply`]) in `(target SimId, projectile id)` order (doc 15 §4). A sector
//!    reaching 0 under a hostile hit is a **breach**: [`DefenceEvent::SectorBreached`], once,
//!    if [`hooks::breach_may_occur`] allows it. While the dome is `immune` (S6's Reinforced)
//!    it takes no damage. A module at 0 hull is disabled, destroyed or (the core) reported
//!    down, as [`hooks::structure_outcome_at_zero`] decides: a peripheral module is destroyed
//!    inside its owner's window and disabled outside it, and raiders only disable (doc 12 §3,
//!    D-169, D-223). Every hit goes into the holding's damage [`ledger`] (D-057).
//! 5. **Settling** ([`settle`]), once a second per holding in its economy slot: each
//!    sector's regeneration demand for S4's allocation, the recovery of disabled structures
//!    (D-149, [`recovery`]) and pruning.
//!
//! **Hooks for S6** (S5 plan §4.5; S6 plan §4.4, §4.7): the breach event, the `immune` switch
//! ([`UniverseState::set_dome_immune`]), `open` ([`UniverseState::open_sector`]), sector
//! restoring for Held and capture, the damage ledger, and the two functions in [`hooks`],
//! whose rules S6b gives ([`crate::siege`]).
//!
//! **Power** comes from S4 ([`crate::economy`]): the allocation per sector for regeneration,
//! the powered flag per turret, and ability power through `request_ability_power` (D-128).
//! **Detection (S7):** turrets and FLAK SCREEN fire only at ships their holding's owner
//! detects, by the pictures of the last sensors pass.

mod abilities;
mod dome;
pub(crate) mod hooks;
pub(crate) mod ledger;
mod recovery;
mod structures;
mod turrets;

#[cfg(test)]
mod tests;

pub(crate) use abilities::{execute, validate};
pub(crate) use structures::{StructureHit, Structures, attack_aim, attack_edge_m, locate_module};

pub use ledger::{DomeDamage, LedgerView, Tally};

use std::collections::BTreeMap;

use nomad_types::Owner;
use serde::{Deserialize, Serialize};

use crate::combat::{ShipIndex, WeaponSlot};
use crate::economy::EconomyEvent;
use crate::fleet::{Fleet, FleetId, ShipId};
use crate::galaxy::SystemId;
use crate::geometry::Vec2;
use crate::holding::HoldingId;
use crate::id::{CommanderId, SimId};
use crate::sensors::Seen;
use crate::state::UniverseState;
use crate::time::Tick;

use self::ledger::Ledger;

/// The station-combat part of the Universe state (S5b).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Defence {
    /// Per holding, outposts included.
    pub(crate) holdings: BTreeMap<HoldingId, HoldingDefence>,
    /// Each turret module's weapon state, while it aims or reloads.
    pub(crate) turrets: BTreeMap<SimId, WeaponSlot>,
}

/// One holding's defence.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct HoldingDefence {
    /// Whether the dome is up: the core is complete. Sectors start at their maximum when it
    /// comes up.
    pub(crate) dome_up: bool,
    /// The four sectors, in pylon slot order (D-046: N, E, S, W with the template's pylons).
    pub(crate) sectors: [Sector; 4],
    /// S6's Reinforced: the dome takes no damage, but still blocks (doc 12 §4).
    pub(crate) immune: bool,
    /// STOP: every turret holds fire (doc 15 §5.3).
    pub(crate) turrets_hold: bool,
    /// SHLD SURGE may be used from this tick.
    pub(crate) surge_ready_at: Tick,
    /// FLAK SCREEN.
    pub(crate) flak_screen: FlakScreen,
    /// The last tick a hostile ship was inside the holding's ops area, while it has
    /// disabled structures (D-149).
    pub(crate) last_hostile: Option<Tick>,
    /// The damage ledger for S6 (D-057).
    pub(crate) ledger: Ledger,
    /// Ships that damaged one of the holding's structures (the dome, keyed by the holding's
    /// id, or a module), with the tick of their last hit, for *return fire* (D-145). Kept for
    /// the engagement window.
    pub(crate) struck_by: BTreeMap<SimId, BTreeMap<ShipId, Tick>>,
}

/// One dome sector.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Sector {
    /// Strength in milli-points (integer, D-052).
    pub(crate) strength: u64,
    /// The maximum in milli-points (D-140), as of the holding's last economy slot.
    pub(crate) max: u64,
    /// S6's siege: no regeneration and no blocking (doc 15 §5.1).
    pub(crate) open: bool,
    /// A breach was reported and the sector has not been back to its maximum since, so the
    /// event is sent once.
    pub(crate) breach_reported: bool,
    /// Regeneration below one milli-point, carried to the next tick so nothing is lost.
    pub(crate) carry: u64,
}

/// FLAK SCREEN state (D-148).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FlakScreen {
    /// It may be used from this tick.
    pub(crate) ready_at: Tick,
    /// While firing: the tick it started and the first tick it no longer fires.
    pub(crate) firing: Option<(Tick, Tick)>,
}

/// Something station combat produced during a tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefenceEvent {
    /// A sector took damage this tick from one owner's weapons, summed over the tick per
    /// attacking fleet.
    SectorDamaged {
        /// The holding.
        holding: HoldingId,
        /// The sector.
        sector: u8,
        /// Whose weapons.
        by: Owner,
        /// The fleet of the ship that fired, if a ship fired and still exists.
        by_fleet: Option<FleetId>,
        /// Strength lost, milli-points.
        amount_milli: u64,
    },
    /// A hostile hit took a sector to 0, or hit it at 0: a breach (doc 15 §5.1). Sent once
    /// until the sector is back at its maximum or restored, and only when
    /// [`hooks::breach_may_occur`] allows it. S6 acts on it (S6 plan §4.4).
    SectorBreached {
        /// The holding.
        holding: HoldingId,
        /// The sector.
        sector: u8,
        /// When.
        tick: Tick,
    },
    /// A module took damage this tick from one owner's weapons, summed over the tick: the
    /// `DamageApplied` of the S4/S5 interface (S5 plan §4.6), per attacking fleet.
    ModuleDamaged {
        /// The holding.
        holding: HoldingId,
        /// The module.
        module: SimId,
        /// Whose weapons.
        by: Owner,
        /// The fleet of the ship that fired, if a ship fired and still exists.
        by_fleet: Option<FleetId>,
        /// Hull points lost.
        amount: u32,
    },
    /// A holding's core reached 0 hull: during a siege the holding has fallen (doc 15 §6);
    /// an outpost under construction is destroyed (D-054). S6 acts on it.
    CoreDown {
        /// The holding.
        holding: HoldingId,
        /// Whose hit brought it to 0.
        by: Owner,
        /// When.
        tick: Tick,
    },
}

/// One sector as seen from outside.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SectorView {
    /// Strength, milli-points.
    pub strength_milli: u64,
    /// Maximum, milli-points (D-140).
    pub max_milli: u64,
    /// Below half its maximum (phase 1 "weak").
    pub weak: bool,
    /// Open in a siege: no blocking, no regeneration.
    pub open: bool,
    /// A breach was reported and it has not been back to its maximum since.
    pub breached: bool,
}

/// A holding's defence as seen from outside (S5b; the phase-1 shield ring, "TURRETS 4 ·
/// 2 FIRING" and the ability buttons). Deadlines are ticks, not countdowns (doc 11 §3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DefenceView {
    /// The holding.
    pub holding: HoldingId,
    /// Whether it projects a dome: its core is complete.
    pub dome: bool,
    /// The sectors, in pylon slot order.
    pub sectors: [SectorView; 4],
    /// Reinforced: the dome takes no damage (S6).
    pub immune: bool,
    /// STOP is set.
    pub turrets_hold: bool,
    /// Completed turrets that are not disabled.
    pub turrets: u32,
    /// Of those, how many have their power.
    pub turrets_powered: u32,
    /// Of those, how many have a target.
    pub turrets_firing: u32,
    /// SHLD SURGE may be used from this tick.
    pub surge_ready_at: Tick,
    /// FLAK SCREEN may be used from this tick.
    pub flak_screen_ready_at: Tick,
    /// While FLAK SCREEN fires: the first tick it no longer does.
    pub flak_screen_until: Option<Tick>,
}

/// Station-combat upkeep at the start of phase 7: in each holding's economy slot its dome
/// comes up or goes and its maxima follow the pylons; every tick, closed sectors below their
/// maximum regenerate from their allocated power (D-140 to D-142).
pub(crate) fn upkeep(state: &mut UniverseState, now: Tick) {
    state
        .defence
        .holdings
        .retain(|id, _| state.holdings.contains_key(id));
    for holding in due(state, now) {
        dome::refresh(state, holding);
    }
    dome::regenerate(state);
}

/// The holdings whose economy slot is this tick (doc 11 §4.1).
fn due(state: &UniverseState, now: Tick) -> Vec<HoldingId> {
    let slot = now.get() % crate::time::TICKS_PER_SECOND;
    state
        .holdings
        .keys()
        .copied()
        .filter(|&h| crate::economy::economy_slot(h) == slot)
        .collect()
}

/// Turrets and FLAK SCREEN fire, holding by holding, after the ships (S5 plan §4.8).
pub(crate) fn fire(state: &mut UniverseState, now: Tick, index: &ShipIndex, seen: &Seen) {
    turrets::run(state, now, index, seen);
    abilities::flak_screens(state, now, index, seen);
}

/// Applies the tick's hits on sectors and modules in `(target SimId, projectile id)` order
/// (doc 15 §4): see [`structures::apply`].
pub(crate) fn apply(
    state: &mut UniverseState,
    now: Tick,
    hits: Vec<StructureHit>,
    events: &mut Vec<DefenceEvent>,
    economy_events: &mut Vec<EconomyEvent>,
) {
    structures::apply(state, now, hits, events, economy_events);
}

/// The end of phase 7, for each holding in its economy slot: regeneration demand for S4's
/// allocation later in the tick, recovery of disabled structures (D-149) and pruning of old
/// records.
pub(crate) fn settle(
    state: &mut UniverseState,
    now: Tick,
    index: &ShipIndex,
    economy_events: &mut Vec<EconomyEvent>,
) {
    let window = crate::combat::engagement_window(state);
    let ledger_window = crate::time::Ticks::from_secs(state.rules.defence.ledger_window_s);
    for holding in due(state, now) {
        dome::set_demand(state, holding);
        recovery::run(state, now, index, holding, economy_events);
        if let Some(d) = state.defence.holdings.get_mut(&holding) {
            for ships in d.struck_by.values_mut() {
                ships.retain(|_, &mut t| t.after(window) > now);
            }
            d.struck_by.retain(|_, ships| !ships.is_empty());
            d.ledger.prune(now, ledger_window);
        }
    }
}

/// Ships that damaged one of `fleet`'s commander's structures in `system` in the last 30 s,
/// where the structure lies within the fleet's weapon range of `anchor` (D-145): a fleet on
/// *return fire* may engage them. The fleet's range is its shortest class weapon range
/// (D-115), measured to the structure's edge.
pub(crate) fn structure_attackers(
    state: &UniverseState,
    fleet: &Fleet,
    system: SystemId,
    anchor: Vec2,
    now: Tick,
) -> std::collections::BTreeSet<ShipId> {
    let window = crate::combat::engagement_window(state);
    let mut ships = std::collections::BTreeSet::new();
    let Owner::Commander(commander) = fleet.owner else {
        return ships;
    };
    let Some(range) = fleet
        .members
        .keys()
        .filter_map(|s| state.entities.get::<crate::fleet::Ship>(s.sim_id()))
        .filter_map(|s| state.rules.class(s.class).weapon_range_m)
        .reduce(f64::min)
    else {
        return ships;
    };
    for (&holding, d) in &state.defence.holdings {
        if d.struck_by.is_empty()
            || state.holdings.get(&holding).map(|h| h.owner) != Some(commander)
        {
            continue;
        }
        for (&structure, attackers) in &d.struck_by {
            let Some((at, centre, radius)) = structures::circle_of(state, holding, structure)
            else {
                continue;
            };
            if at == system && (centre - anchor).length() - radius <= range {
                // Records are pruned once a second; the window itself is exact.
                ships.extend(
                    attackers
                        .iter()
                        .filter(|&(_, &t)| t.after(window) > now)
                        .map(|(&ship, _)| ship),
                );
            }
        }
    }
    ships
}

impl UniverseState {
    /// A holding's defence: dome sectors, switches, turrets and abilities (S5b).
    pub fn defence(&self, holding: HoldingId) -> Option<DefenceView> {
        self.holdings.get(&holding)?;
        let d = self.defence.holdings.get(&holding);
        let up = dome::has_dome(self, holding);
        // A dome not yet refreshed by the combat phase stands at its maximum.
        let maxima = match d.filter(|d| d.dome_up) {
            Some(d) => d.sectors.map(|s| s.max),
            None if up => dome::maxima(self, holding),
            None => [0; 4],
        };
        let weak = self.rules.defence.weak_below_percent;
        let mut sectors = [SectorView::default(); 4];
        for (i, view) in sectors.iter_mut().enumerate() {
            let max = maxima[i];
            let sector = d.filter(|d| d.dome_up).map(|d| d.sectors[i]);
            let strength = sector.map_or(max, |s| s.strength);
            *view = SectorView {
                strength_milli: strength,
                max_milli: max,
                weak: up && u128::from(strength) * 100 < u128::from(max) * u128::from(weak),
                open: sector.is_some_and(|s| s.open),
                breached: sector.is_some_and(|s| s.breach_reported),
            };
        }
        let (turrets, turrets_powered) = turrets::counts(self, holding);
        let turrets_firing = turrets::firing(self, holding);
        Some(DefenceView {
            holding,
            dome: up,
            sectors,
            immune: d.is_some_and(|d| d.immune),
            turrets_hold: d.is_some_and(|d| d.turrets_hold),
            turrets,
            turrets_powered,
            turrets_firing,
            surge_ready_at: d.map_or(Tick::ZERO, |d| d.surge_ready_at),
            flak_screen_ready_at: d.map_or(Tick::ZERO, |d| d.flak_screen.ready_at),
            flak_screen_until: d.and_then(|d| d.flak_screen.firing.map(|(_, until)| until)),
        })
    }

    /// The commanders whose ships damaged `holding`'s dome at or after `since`: for S6, the
    /// attackers of the 10 minutes before a breach (D-057). In commander order.
    pub fn dome_attackers(&self, holding: HoldingId, since: Tick) -> Vec<CommanderId> {
        self.defence
            .holdings
            .get(&holding)
            .map_or_else(Vec::new, |d| d.ledger.dome_attackers(since))
    }

    /// `holding`'s damage ledger (D-057).
    pub fn damage_ledger(&self, holding: HoldingId) -> Option<LedgerView> {
        self.defence.holdings.get(&holding).map(|d| d.ledger.view())
    }

    /// S6's Reinforced switch: while `immune`, the dome takes no damage but still blocks
    /// hostile fire (doc 12 §4; S5 plan §4.5).
    pub(crate) fn set_dome_immune(&mut self, holding: HoldingId, immune: bool) {
        if self.holdings.contains_key(&holding) {
            self.defence.holdings.entry(holding).or_default().immune = immune;
        }
    }

    /// S6's siege: `sector` is open, so it neither blocks nor regenerates, and hostile fire
    /// through it can hit the modules inside (doc 15 §5.1, §6). Returns whether there is
    /// such a sector at a holding with a dome.
    pub(crate) fn open_sector(&mut self, holding: HoldingId, sector: u8) -> bool {
        if usize::from(sector) >= 4 || !dome::has_dome(self, holding) {
            return false;
        }
        dome::refresh(self, holding);
        let Some(d) = self.defence.holdings.get_mut(&holding) else {
            return false;
        };
        d.sectors[usize::from(sector)].open = true;
        true
    }

    /// Closes every open sector of `holding` (S6: the siege is over).
    pub(crate) fn close_sectors(&mut self, holding: HoldingId) {
        if let Some(d) = self.defence.holdings.get_mut(&holding) {
            for sector in &mut d.sectors {
                sector.open = false;
            }
        }
    }

    /// Raises every sector of `holding` to at least `percent` of its maximum (S6: Held
    /// restores 50%, a capture 25%; D-057) and re-arms the breach event. *Derived:* "restores"
    /// raises a sector and never lowers one; a sector that regenerated while the dome was
    /// immune keeps its strength (D-227).
    pub(crate) fn restore_sectors(&mut self, holding: HoldingId, percent: u64) {
        if !dome::has_dome(self, holding) {
            return;
        }
        dome::refresh(self, holding);
        let Some(d) = self.defence.holdings.get_mut(&holding) else {
            return;
        };
        for sector in &mut d.sectors {
            let max = sector.max;
            let restored =
                u64::try_from(u128::from(max) * u128::from(percent.min(100)) / 100).unwrap_or(max);
            sector.strength = sector.strength.max(restored).min(max);
            sector.breach_reported = false;
            sector.carry = 0;
        }
    }

    /// Starts `holding`'s siege tally afresh at `now`: core damage and total damage per
    /// commander from then on (S6: at the siege's start; D-057).
    pub(crate) fn reset_siege_tally(&mut self, holding: HoldingId, now: Tick) {
        if self.holdings.contains_key(&holding) {
            self.defence
                .holdings
                .entry(holding)
                .or_default()
                .ledger
                .reset_tally(now);
        }
    }
}
