//! Counting and ranking the standings: a pure function of the holdings (S8 plan §4.1, §4.2;
//! D-041, D-066, D-193, D-194).

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

use nomad_types::{ControlState, DepositValue, Standing};
use serde::{Deserialize, Serialize};

use crate::galaxy::{Galaxy, SystemId};
use crate::holding::{self, Holding, HoldingId};
use crate::id::CommanderId;
use crate::time::Tick;

/// What one commander holds, before ranking (D-041).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Tally {
    /// Claimable systems controlled.
    pub(crate) systems: u32,
    /// Holdings with a completed core.
    pub(crate) holdings: u32,
    /// The deposit value at those holdings' sites (D-066).
    pub(crate) deposit_value: DepositValue,
}

/// Every system's control state and every commander's tally, counted from `holdings`. Each
/// commander in `commanders` gets a tally, holding nothing if need be; so does every owner of
/// a holding. Outposts count for nothing, and raider havens are not claimable (D-041, doc 10
/// §4.1).
pub(crate) fn tally(
    galaxy: &Galaxy,
    holdings: &BTreeMap<HoldingId, Holding>,
    commanders: &BTreeSet<CommanderId>,
) -> (
    BTreeMap<SystemId, ControlState>,
    BTreeMap<CommanderId, Tally>,
) {
    let control = holding::control_states(galaxy, holdings);
    let mut tallies: BTreeMap<CommanderId, Tally> = commanders
        .iter()
        .map(|&commander| (commander, Tally::default()))
        .collect();
    for h in holdings.values() {
        let entry = tallies.entry(h.owner).or_default();
        if holding::counts_for_control(h) {
            entry.holdings = entry.holdings.saturating_add(1);
            entry.deposit_value =
                entry.deposit_value + galaxy.site_value(h.site).unwrap_or(DepositValue::ZERO);
        }
    }
    for state in control.values() {
        if let ControlState::Controlled(owner) = *state {
            let entry = tallies.entry(owner).or_default();
            entry.systems = entry.systems.saturating_add(1);
        }
    }
    (control, tallies)
}

/// The order of D-041 and D-193: systems, holdings and deposit value, more first, then
/// *since*, earlier first. Equal keys share a rank (D-194).
fn key(s: &Standing) -> (Reverse<u32>, Reverse<u32>, Reverse<DepositValue>, Tick) {
    (
        Reverse(s.systems),
        Reverse(s.holdings),
        Reverse(s.deposit_value),
        s.since,
    )
}

/// Ranks `tallies` on tick `now`. A commander whose systems, holdings and deposit value are
/// the same as in `previous` keeps its *since*; any other gets `now` (D-193).
///
/// Commanders equal on all four keys share a rank (D-194). Within a shared rank the list is
/// in identity order only so that it is deterministic; that order decides nothing, and no
/// rule may read it as a tie-break (D-229).
pub(crate) fn rank(
    tallies: &BTreeMap<CommanderId, Tally>,
    previous: &Standings,
    now: Tick,
) -> Standings {
    let mut rows: Vec<Standing> = tallies
        .iter()
        .map(|(&commander, t)| {
            let since = previous
                .of(commander)
                .filter(|p| {
                    (p.systems, p.holdings, p.deposit_value)
                        == (t.systems, t.holdings, t.deposit_value)
                })
                .map_or(now, |p| p.since);
            Standing {
                commander,
                rank: 0,
                systems: t.systems,
                holdings: t.holdings,
                deposit_value: t.deposit_value,
                since,
            }
        })
        .collect();
    rows.sort_by(|a, b| key(a).cmp(&key(b)).then(a.commander.cmp(&b.commander)));
    let mut above = None;
    for (index, row) in rows.iter_mut().enumerate() {
        let k = key(row);
        row.rank = match above {
            Some((above_key, above_rank)) if above_key == k => above_rank,
            _ => u32::try_from(index + 1).unwrap_or(u32::MAX),
        };
        above = Some((k, row.rank));
    }
    Standings(rows)
}

/// The standings, ranked (D-041, D-193, D-194). Public (D-075).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Standings(Vec<Standing>);

impl Standings {
    /// Every standing, best first. Commanders who share a rank are in identity order, which
    /// carries no meaning (D-229).
    pub fn ranked(&self) -> &[Standing] {
        &self.0
    }

    /// Every standing, best first.
    pub fn iter(&self) -> impl Iterator<Item = &Standing> {
        self.0.iter()
    }

    /// The number of commanders with a standing.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether nobody has a standing yet.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The standing of `commander`, if it has one.
    pub fn of(&self, commander: CommanderId) -> Option<&Standing> {
        self.0.iter().find(|s| s.commander == commander)
    }

    /// The commanders ranked first: the standings leaders, in identity order.
    pub fn leaders(&self) -> impl Iterator<Item = &Standing> {
        self.0.iter().filter(|s| s.rank == 1)
    }

    /// The holdings of the standings leader, the comeback grant's reference (D-167): with
    /// joint leaders, the most any of them holds (S8 plan §4.7). 0 without standings.
    pub fn leader_holdings(&self) -> u32 {
        self.leaders().map(|s| s.holdings).max().unwrap_or(0)
    }
}
