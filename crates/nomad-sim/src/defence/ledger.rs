//! The damage ledger for S6 (D-057, D-163; S5 plan §4.5).
//!
//! Per holding:
//!
//! - **Dome damage per commander,** with the tick of their last hit, so S6 can find the
//!   attackers of the 10 minutes before a breach. A commander's entry is dropped once their
//!   last hit is older than `defence.ledger_window_s` (D-057's 10 minutes), so the points
//!   count the current run of attacks.
//! - **A tally per commander** of core damage and of total damage to the holding (dome
//!   sectors and modules, the core included), for the capture rule: the most core damage
//!   captures, ties go to the larger total, then the lower `CommanderId`. S6 starts it afresh
//!   when a siege starts ([`crate::UniverseState::reset_siege_tally`]).
//!
//! Only commanders are recorded: NPC raiders do no damage to domes or to modules inside the
//! dome, so they never take part in breaches or sieges (D-056, D-163). Amounts are
//! milli-points (a sector's unit); hull points count 1,000 each, so a module's hull point and
//! a sector's point weigh the same in the total.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::economy::rules::MILLI;
use crate::id::CommanderId;
use crate::time::{Tick, Ticks};

/// A holding's ledger.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Ledger {
    pub(crate) dome: BTreeMap<CommanderId, DomeDamage>,
    pub(crate) tally_since: Tick,
    pub(crate) tally: BTreeMap<CommanderId, Tally>,
}

/// One commander's dome damage.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DomeDamage {
    /// Their first hit of the current run.
    pub first: Tick,
    /// Their last hit.
    pub last: Tick,
    /// Sector strength they removed in this run, milli-points.
    pub milli: u64,
}

/// One commander's damage since the tally started.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tally {
    /// Damage to the core, milli-points (hull points × 1,000).
    pub core_milli: u64,
    /// Damage to the holding: dome sectors and every module, milli-points.
    pub total_milli: u64,
}

/// A holding's ledger as seen from outside.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LedgerView {
    /// Dome damage per commander, in commander order.
    pub dome: Vec<(CommanderId, DomeDamage)>,
    /// Since when the tally counts.
    pub tally_since: Tick,
    /// The tally per commander, in commander order.
    pub tally: Vec<(CommanderId, Tally)>,
}

impl Ledger {
    /// `commander` removed `milli` of sector strength at `now`.
    pub(crate) fn dome_hit(&mut self, commander: CommanderId, milli: u64, now: Tick) {
        let entry = self.dome.entry(commander).or_insert(DomeDamage {
            first: now,
            last: now,
            milli: 0,
        });
        entry.last = now;
        entry.milli = entry.milli.saturating_add(milli);
        let tally = self.tally_mut(commander);
        tally.total_milli = tally.total_milli.saturating_add(milli);
    }

    /// `commander` removed `hull` hull points of a module; `core` if it was the core.
    pub(crate) fn module_hit(&mut self, commander: CommanderId, hull: u32, core: bool) {
        let milli = u64::from(hull).saturating_mul(MILLI);
        let tally = self.tally_mut(commander);
        tally.total_milli = tally.total_milli.saturating_add(milli);
        if core {
            tally.core_milli = tally.core_milli.saturating_add(milli);
        }
    }

    fn tally_mut(&mut self, commander: CommanderId) -> &mut Tally {
        self.tally.entry(commander).or_default()
    }

    /// Commanders with a dome hit at or after `since`.
    pub(crate) fn dome_attackers(&self, since: Tick) -> Vec<CommanderId> {
        self.dome
            .iter()
            .filter(|(_, d)| d.last >= since)
            .map(|(&c, _)| c)
            .collect()
    }

    /// Starts the tally afresh at `now`.
    pub(crate) fn reset_tally(&mut self, now: Tick) {
        self.tally.clear();
        self.tally_since = now;
    }

    /// Drops dome entries whose last hit is `window` or more before `now`.
    pub(crate) fn prune(&mut self, now: Tick, window: Ticks) {
        self.dome.retain(|_, d| d.last.after(window) > now);
    }

    pub(crate) fn view(&self) -> LedgerView {
        LedgerView {
            dome: self.dome.iter().map(|(&c, &d)| (c, d)).collect(),
            tally_since: self.tally_since,
            tally: self.tally.iter().map(|(&c, &t)| (c, t)).collect(),
        }
    }
}
