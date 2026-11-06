//! Holdings and system control (Design doc 10 §5–§6; D-041).
//!
//! A holding is a site owned by a commander. Only a holding whose core module is complete
//! counts for control; until then the claim is an outpost (doc 10 §5, RK-09). The core
//! module and claiming are in [`crate::economy`] (D-135); capture is S6's.
//!
//! Holding identities and control states are public (doc 16 §2), so [`HoldingId`] and
//! [`ControlState`] live in `nomad-types` (D-086, doc 20 §3) and are re-exported here. The
//! majority rule ([`majority_threshold`], [`control_from_counts`]) is a game rule and stays
//! in the simulation, so the client links no rules.

pub use nomad_types::{ControlState, HoldingId};

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::galaxy::{Galaxy, SiteId, SystemId, is_claimable};
use crate::id::{CommanderId, SimIdAllocator};

/// A site owned by a commander (doc 10 §5, D-041).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Holding {
    /// The site it occupies. At most one holding per site.
    pub site: SiteId,
    /// Who owns it. Ownership is public (D-044).
    pub owner: CommanderId,
    /// Whether the core module is complete. Until it is, the site is an outpost and does not
    /// count for control (D-041).
    pub core_complete: bool,
    /// Whether its core is a mother-station core, with 24 docking places (D-102) and the
    /// phase-1 station layout (S3 plan §4.3). Its modules, dry docks included, are economy
    /// entities ([`crate::economy`]).
    pub mother_station: bool,
}

/// The number of completed holdings needed to control a system with `site_count` sites: a
/// strict majority, ⌊n/2⌋+1 (D-041). 4 of 7, 4 of 6, 5 of 8.
pub const fn majority_threshold(site_count: usize) -> usize {
    site_count / 2 + 1
}

/// The control state of a system with `site_count` sites, given how many **completed**
/// holdings each commander owns there (D-041).
pub fn control_from_counts(
    site_count: usize,
    completed: &BTreeMap<CommanderId, usize>,
) -> ControlState {
    let threshold = majority_threshold(site_count);
    // At most one commander can hold a strict majority of a system's sites; iterating in
    // commander order keeps the answer defined even for inconsistent counts.
    if let Some((&owner, _)) = completed.iter().find(|&(_, &n)| n >= threshold) {
        return ControlState::Controlled(owner);
    }
    if completed.values().filter(|&&n| n > 0).count() >= 2 {
        ControlState::Contested
    } else {
        ControlState::Open
    }
}

/// Why a holding could not be placed.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum HoldingError {
    /// The site does not exist.
    #[error("site {0:?} does not exist")]
    UnknownSite(SiteId),
    /// The site already has a holding.
    #[error("site {site:?} already has holding {holding:?}")]
    SiteTaken {
        /// The site.
        site: SiteId,
        /// The holding already there.
        holding: HoldingId,
    },
    /// The site is in a system that cannot be claimed, such as a raider haven.
    #[error("site {0:?} is in a system that cannot be claimed")]
    NotClaimable(SiteId),
}

/// Adds `holding` if its site exists, can be claimed and is free.
pub(crate) fn insert(
    galaxy: &Galaxy,
    ids: &mut SimIdAllocator,
    holdings: &mut BTreeMap<HoldingId, Holding>,
    holding: Holding,
) -> Result<HoldingId, HoldingError> {
    let site = holding.site;
    let system = galaxy
        .site(site)
        .and_then(|s| galaxy.system(s.system))
        .ok_or(HoldingError::UnknownSite(site))?;
    if !is_claimable(system.kind) {
        return Err(HoldingError::NotClaimable(site));
    }
    if let Some((&existing, _)) = holdings.iter().find(|(_, h)| h.site == site) {
        return Err(HoldingError::SiteTaken {
            site,
            holding: existing,
        });
    }
    let id = HoldingId(ids.allocate());
    holdings.insert(id, holding);
    Ok(id)
}

/// The control state of every claimable system, in system order (D-041). Raider havens are
/// not claimable and are left out.
pub(crate) fn control_states(
    galaxy: &Galaxy,
    holdings: &BTreeMap<HoldingId, Holding>,
) -> BTreeMap<SystemId, ControlState> {
    let mut completed: BTreeMap<SystemId, BTreeMap<CommanderId, usize>> = BTreeMap::new();
    for holding in holdings.values().filter(|h| h.core_complete) {
        if let Some(site) = galaxy.site(holding.site) {
            *completed
                .entry(site.system)
                .or_default()
                .entry(holding.owner)
                .or_default() += 1;
        }
    }
    let none = BTreeMap::new();
    galaxy
        .systems()
        .filter(|(_, system)| is_claimable(system.kind))
        .map(|(id, system)| {
            let counts = completed.get(&id).unwrap_or(&none);
            (id, control_from_counts(system.sites.len(), counts))
        })
        .collect()
}

/// The control state of one system, or `None` if it does not exist or cannot be claimed.
pub(crate) fn control_state(
    galaxy: &Galaxy,
    holdings: &BTreeMap<HoldingId, Holding>,
    system: SystemId,
) -> Option<ControlState> {
    let sys = galaxy.system(system).filter(|s| is_claimable(s.kind))?;
    let mut completed: BTreeMap<CommanderId, usize> = BTreeMap::new();
    for holding in holdings.values().filter(|h| h.core_complete) {
        if galaxy
            .site(holding.site)
            .is_some_and(|s| s.system == system)
        {
            *completed.entry(holding.owner).or_default() += 1;
        }
    }
    Some(control_from_counts(sys.sites.len(), &completed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::galaxy::SystemKind;
    use crate::geometry::polar;
    use crate::{SeasonConfig, UniverseState};

    const YOU: CommanderId = CommanderId(1);
    const HOSTILE: CommanderId = CommanderId(2);
    const OTHER: CommanderId = CommanderId(3);

    /// One sector with a 7-site claimable system and a raider haven with 2 sites.
    fn kestrel() -> (UniverseState, Vec<SiteId>, Vec<SiteId>) {
        let mut state = UniverseState::new(SeasonConfig {
            seed: 1,
            season_start_unix_ms: 0,
        });
        let ids: &mut SimIdAllocator = &mut state.ids;
        let galaxy = &mut state.galaxy;
        let sector = galaxy.add_sector(ids);
        let system = galaxy.add_system(ids, sector, SystemKind::Standard);
        let haven = galaxy.add_system(ids, sector, SystemKind::RaiderHaven);
        let mut place =
            |system, i: u32| galaxy.add_site(ids, system, polar(100_000.0, f64::from(i)));
        let sites = (0..7).map(|i| place(system, i)).collect();
        let haven_sites = (0..2).map(|i| place(haven, i)).collect();
        (state, sites, haven_sites)
    }

    fn counts(pairs: &[(CommanderId, usize)]) -> BTreeMap<CommanderId, usize> {
        pairs.iter().copied().collect()
    }

    #[test]
    fn majority_is_strict() {
        assert_eq!(majority_threshold(7), 4);
        assert_eq!(majority_threshold(6), 4);
        assert_eq!(majority_threshold(8), 5);
        assert_eq!(majority_threshold(9), 5);
    }

    #[test]
    fn control_from_counts_covers_every_state() {
        // Phase-1 Kestrel-7: "YOU 3 · HSTL 2 · FREE 2" is CONTESTED (doc 10 §6).
        assert_eq!(
            control_from_counts(7, &counts(&[(YOU, 3), (HOSTILE, 2)])),
            ControlState::Contested
        );
        assert_eq!(
            control_from_counts(7, &counts(&[(YOU, 4), (HOSTILE, 2)])),
            ControlState::Controlled(YOU)
        );
        // Half is not a majority.
        assert_eq!(
            control_from_counts(6, &counts(&[(YOU, 3), (HOSTILE, 3)])),
            ControlState::Contested
        );
        assert_eq!(
            control_from_counts(6, &counts(&[(YOU, 3)])),
            ControlState::Open
        );
        assert_eq!(control_from_counts(7, &counts(&[])), ControlState::Open);
        assert_eq!(
            control_from_counts(7, &counts(&[(YOU, 0), (HOSTILE, 1)])),
            ControlState::Open
        );
    }

    #[test]
    fn phase1_example_is_contested_and_outposts_do_not_count() {
        let (mut state, sites, _) = kestrel();
        for &site in &sites[..3] {
            state.insert_holding(site, YOU, true).unwrap();
        }
        for &site in &sites[3..5] {
            state.insert_holding(site, HOSTILE, true).unwrap();
        }
        let system = state.galaxy().site(sites[0]).unwrap().system;
        assert_eq!(state.control_state(system), Some(ControlState::Contested));
        assert_eq!(
            state.control_states().get(&system),
            Some(&ControlState::Contested)
        );

        // A fourth site as an outpost does not give control (D-041) ...
        let outpost = state.insert_holding(sites[5], YOU, false).unwrap();
        assert_eq!(state.control_state(system), Some(ControlState::Contested));
        // ... completing its core does.
        state.holdings.get_mut(&outpost).unwrap().core_complete = true;
        assert_eq!(
            state.control_state(system),
            Some(ControlState::Controlled(YOU))
        );
    }

    #[test]
    fn single_owner_without_majority_is_open() {
        let (mut state, sites, _) = kestrel();
        state.insert_holding(sites[0], OTHER, true).unwrap();
        state.insert_holding(sites[1], HOSTILE, false).unwrap();
        let system = state.galaxy().site(sites[0]).unwrap().system;
        assert_eq!(state.control_state(system), Some(ControlState::Open));
    }

    #[test]
    fn havens_are_not_claimable_and_have_no_control_state() {
        let (mut state, sites, haven_sites) = kestrel();
        assert_eq!(
            state.insert_holding(haven_sites[0], YOU, true),
            Err(HoldingError::NotClaimable(haven_sites[0]))
        );
        let haven = state.galaxy().site(haven_sites[0]).unwrap().system;
        assert_eq!(state.control_state(haven), None);
        let all = state.control_states();
        assert_eq!(all.len(), 1);
        assert!(!all.contains_key(&haven));
        let first = state.insert_holding(sites[0], YOU, true).unwrap();
        assert_eq!(
            state.insert_holding(sites[0], HOSTILE, true),
            Err(HoldingError::SiteTaken {
                site: sites[0],
                holding: first
            })
        );
    }
}
