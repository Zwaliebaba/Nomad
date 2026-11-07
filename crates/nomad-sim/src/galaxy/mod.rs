//! The season galaxy: sectors, systems, sites, gates and lanes (Design doc 10).
//!
//! A new galaxy is generated from the season seed at season start and stays fixed for the
//! season; its topology is fully charted from the start (D-044). The hierarchy is
//! sector → system → site, with gates at each system's edge joined by timed lanes
//! (doc 10 §2, §4; D-022, D-042, D-043).
//!
//! Every object has a typed identity wrapping a [`SimId`](crate::SimId), allocated from the
//! Universe's [`SimIdAllocator`]. The identities and the small kind enums live in
//! `nomad-types` and are re-exported here (D-086). Objects live in ordered maps keyed by
//! that identity, and child lists are kept in increasing identity order, so iteration and
//! the snapshot encoding are canonical (doc 11 §5, D-049).
//!
//! Positions are in the **system frame**: metres, 2D, origin at the star, +y = system
//! north (doc 10 §8; D-099 item 2: metres in the simulation, km only for documents and the
//! UI). Lane travel is a timed transit, not a distance (D-042).
//!
//! Sites carry their deposits and deposit values (doc 14 §5, D-066), which feed the balance
//! of home neighbourhoods (doc 10 §4.3 invariant 5).

mod deposit;
mod generate;
mod validate;

pub use deposit::{
    Deposit, DepositKind, DepositValue, MAX_DEPOSITS_PER_SITE, Richness, base_value, deposit_value,
    richness_per_mille,
};
pub use generate::{
    GenerationError, STAGE1_COMMANDERS, generate_stage1, generate_stage1_roster,
    generate_stage1_with_rules, stage1_sites,
};
pub use validate::{
    GalaxyReport, InvariantLimits, SectorReport, Violation, WorstTrip, allowed_gates,
    allowed_sites, validate,
};
// Identities, kinds and positions shared with the protocol and the client (D-086, doc 20 §3).
pub use nomad_types::{
    GateId, LaneId, LaneKind, PositionMetres, SectorId, SiteId, SystemId, SystemKind,
};

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::id::SimIdAllocator;
use crate::time::Ticks;

/// Whether commanders can claim sites in a system of `kind` and the system counts for
/// control (doc 10 §4.1, §6): every kind except raider havens.
pub const fn is_claimable(kind: SystemKind) -> bool {
    !matches!(kind, SystemKind::RaiderHaven)
}

/// A cluster of systems (doc 10 §2).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sector {
    /// Its systems, in identity order.
    pub systems: Vec<SystemId>,
}

/// A star system: the unit of system control (doc 10 §2, §6).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct System {
    /// The sector it belongs to.
    pub sector: SectorId,
    /// What kind of system it is.
    pub kind: SystemKind,
    /// Its sites, in identity order.
    pub sites: Vec<SiteId>,
    /// Its gates, in identity order.
    pub gates: Vec<GateId>,
}

/// A fixed location in a system. A site owned by a commander is a holding (doc 10 §5).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Site {
    /// The system it is in.
    pub system: SystemId,
    /// Where it is in the system frame.
    pub position: PositionMetres,
    /// Its 0–2 deposits (doc 14 §5, D-066).
    pub deposits: Vec<Deposit>,
}

impl Site {
    /// The total value of its deposits (D-066).
    pub fn value(&self) -> DepositValue {
        self.deposits.iter().copied().map(deposit_value).sum()
    }
}

/// Home neighbourhoods reach this many jumps from the home system (doc 10 §4.3 invariant 5).
pub const HOME_NEIGHBOURHOOD_JUMPS: u32 = 2;

/// A system's entry and exit point, at its edge (doc 10 §4.1).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Gate {
    /// The system it is in.
    pub system: SystemId,
    /// Where it is in the system frame.
    pub position: PositionMetres,
    /// The lanes that leave from it, in identity order.
    pub lanes: Vec<LaneId>,
}

/// A timed, uninterruptible transit between two gates (doc 10 §4.2, D-042).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lane {
    /// One end.
    pub gate_a: GateId,
    /// The other end.
    pub gate_b: GateId,
    /// How long the transit takes, in either direction.
    pub transit: Ticks,
    /// Jump or trunk lane.
    pub kind: LaneKind,
}

impl Lane {
    /// The gate at the other end from `gate`, if `gate` is one of this lane's ends.
    pub fn other_end(&self, gate: GateId) -> Option<GateId> {
        if gate == self.gate_a {
            Some(self.gate_b)
        } else if gate == self.gate_b {
            Some(self.gate_a)
        } else {
            None
        }
    }
}

/// The topology of one season's galaxy (doc 10 §2–§5, D-044).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Galaxy {
    pub(crate) sectors: BTreeMap<SectorId, Sector>,
    pub(crate) systems: BTreeMap<SystemId, System>,
    pub(crate) sites: BTreeMap<SiteId, Site>,
    pub(crate) gates: BTreeMap<GateId, Gate>,
    pub(crate) lanes: BTreeMap<LaneId, Lane>,
}

impl Galaxy {
    /// Whether the galaxy has no systems, as in a Universe created without a generator.
    pub fn is_empty(&self) -> bool {
        self.systems.is_empty()
    }

    /// A sector by identity.
    pub fn sector(&self, id: SectorId) -> Option<&Sector> {
        self.sectors.get(&id)
    }

    /// A system by identity.
    pub fn system(&self, id: SystemId) -> Option<&System> {
        self.systems.get(&id)
    }

    /// A site by identity.
    pub fn site(&self, id: SiteId) -> Option<&Site> {
        self.sites.get(&id)
    }

    /// A gate by identity.
    pub fn gate(&self, id: GateId) -> Option<&Gate> {
        self.gates.get(&id)
    }

    /// A lane by identity.
    pub fn lane(&self, id: LaneId) -> Option<&Lane> {
        self.lanes.get(&id)
    }

    /// All sectors, in identity order.
    pub fn sectors(&self) -> impl Iterator<Item = (SectorId, &Sector)> {
        self.sectors.iter().map(|(&id, sector)| (id, sector))
    }

    /// All systems, in identity order.
    pub fn systems(&self) -> impl Iterator<Item = (SystemId, &System)> {
        self.systems.iter().map(|(&id, system)| (id, system))
    }

    /// All sites, in identity order.
    pub fn sites(&self) -> impl Iterator<Item = (SiteId, &Site)> {
        self.sites.iter().map(|(&id, site)| (id, site))
    }

    /// All gates, in identity order.
    pub fn gates(&self) -> impl Iterator<Item = (GateId, &Gate)> {
        self.gates.iter().map(|(&id, gate)| (id, gate))
    }

    /// All lanes, in identity order.
    pub fn lanes(&self) -> impl Iterator<Item = (LaneId, &Lane)> {
        self.lanes.iter().map(|(&id, lane)| (id, lane))
    }

    /// The systems at either end of a lane, if both gates exist.
    pub fn lane_systems(&self, lane: &Lane) -> Option<(SystemId, SystemId)> {
        let a = self.gates.get(&lane.gate_a)?.system;
        let b = self.gates.get(&lane.gate_b)?.system;
        Some((a, b))
    }

    /// Systems one lane away from `system`, through lanes of any kind. Excludes `system`
    /// itself.
    pub fn neighbours(&self, system: SystemId) -> BTreeSet<SystemId> {
        self.neighbours_filtered(system, |_| true)
    }

    /// Fewest jumps from `from` to every system it can reach, through lanes of any kind.
    /// Includes `from` at 0 jumps; unreachable systems are absent.
    pub fn jumps_from(&self, from: SystemId) -> BTreeMap<SystemId, u32> {
        self.jumps_from_filtered(from, |_| true)
    }

    /// The total deposit value of a site (D-066).
    pub fn site_value(&self, site: SiteId) -> Option<DepositValue> {
        self.sites.get(&site).map(Site::value)
    }

    /// The total deposit value of a system's sites (D-066).
    pub fn system_value(&self, system: SystemId) -> Option<DepositValue> {
        let system = self.systems.get(&system)?;
        Some(
            system
                .sites
                .iter()
                .filter_map(|&s| self.site_value(s))
                .sum(),
        )
    }

    /// `system` and every system within `max_jumps` of it, through lanes of any kind.
    pub fn neighbourhood(&self, system: SystemId, max_jumps: u32) -> BTreeSet<SystemId> {
        self.jumps_from(system)
            .into_iter()
            .filter(|&(_, jumps)| jumps <= max_jumps)
            .map(|(id, _)| id)
            .collect()
    }

    /// The total deposit value of [`Self::neighbourhood`] (D-066).
    pub fn neighbourhood_value(&self, system: SystemId, max_jumps: u32) -> Option<DepositValue> {
        self.systems.contains_key(&system).then(|| {
            self.neighbourhood(system, max_jumps)
                .into_iter()
                .filter_map(|id| self.system_value(id))
                .sum()
        })
    }

    /// The neighbourhood value of every home system, for invariant 5 (doc 10 §4.3): all
    /// systems within [`HOME_NEIGHBOURHOOD_JUMPS`] of the home, the home included.
    pub fn home_neighbourhood_values(&self) -> BTreeMap<SystemId, DepositValue> {
        self.systems()
            .filter(|(_, s)| s.kind == SystemKind::Home)
            .filter_map(|(id, _)| {
                Some((id, self.neighbourhood_value(id, HOME_NEIGHBOURHOOD_JUMPS)?))
            })
            .collect()
    }

    /// Like [`Self::neighbours`], using only lanes accepted by `keep`.
    pub(crate) fn neighbours_filtered(
        &self,
        system: SystemId,
        keep: impl Fn(&Lane) -> bool,
    ) -> BTreeSet<SystemId> {
        let Some(sys) = self.systems.get(&system) else {
            return BTreeSet::new();
        };
        let mut out = BTreeSet::new();
        for gate_id in &sys.gates {
            let Some(gate) = self.gates.get(gate_id) else {
                continue;
            };
            for lane in gate.lanes.iter().filter_map(|id| self.lanes.get(id)) {
                if !keep(lane) {
                    continue;
                }
                let far = lane
                    .other_end(*gate_id)
                    .and_then(|g| self.gates.get(&g))
                    .map(|g| g.system);
                if let Some(far) = far.filter(|&far| far != system) {
                    out.insert(far);
                }
            }
        }
        out
    }

    /// Like [`Self::jumps_from`], using only lanes accepted by `keep`.
    pub(crate) fn jumps_from_filtered(
        &self,
        from: SystemId,
        keep: impl Fn(&Lane) -> bool,
    ) -> BTreeMap<SystemId, u32> {
        let mut jumps = BTreeMap::new();
        if !self.systems.contains_key(&from) {
            return jumps;
        }
        jumps.insert(from, 0);
        let mut queue = VecDeque::from([from]);
        while let Some(system) = queue.pop_front() {
            let next = jumps.get(&system).copied().unwrap_or(0).saturating_add(1);
            for far in self.neighbours_filtered(system, &keep) {
                if let std::collections::btree_map::Entry::Vacant(entry) = jumps.entry(far) {
                    entry.insert(next);
                    queue.push_back(far);
                }
            }
        }
        jumps
    }

    // Construction. Only the generator and tests build galaxies; the topology is fixed for
    // the season once generated (D-044).

    pub(crate) fn add_sector(&mut self, ids: &mut SimIdAllocator) -> SectorId {
        let id = SectorId(ids.allocate());
        self.sectors.insert(
            id,
            Sector {
                systems: Vec::new(),
            },
        );
        id
    }

    pub(crate) fn add_system(
        &mut self,
        ids: &mut SimIdAllocator,
        sector: SectorId,
        kind: SystemKind,
    ) -> SystemId {
        let id = SystemId(ids.allocate());
        self.sectors
            .get_mut(&sector)
            .expect("invariant: a system is added to an existing sector")
            .systems
            .push(id);
        self.systems.insert(
            id,
            System {
                sector,
                kind,
                sites: Vec::new(),
                gates: Vec::new(),
            },
        );
        id
    }

    pub(crate) fn add_site(
        &mut self,
        ids: &mut SimIdAllocator,
        system: SystemId,
        position: PositionMetres,
    ) -> SiteId {
        let id = SiteId(ids.allocate());
        self.systems
            .get_mut(&system)
            .expect("invariant: a site is added to an existing system")
            .sites
            .push(id);
        self.sites.insert(
            id,
            Site {
                system,
                position,
                deposits: Vec::new(),
            },
        );
        id
    }

    pub(crate) fn add_gate(
        &mut self,
        ids: &mut SimIdAllocator,
        system: SystemId,
        position: PositionMetres,
    ) -> GateId {
        let id = GateId(ids.allocate());
        self.systems
            .get_mut(&system)
            .expect("invariant: a gate is added to an existing system")
            .gates
            .push(id);
        self.gates.insert(
            id,
            Gate {
                system,
                position,
                lanes: Vec::new(),
            },
        );
        id
    }

    pub(crate) fn set_deposits(&mut self, site: SiteId, deposits: Vec<Deposit>) {
        self.sites
            .get_mut(&site)
            .expect("invariant: deposits are set on an existing site")
            .deposits = deposits;
    }

    pub(crate) fn add_lane(
        &mut self,
        ids: &mut SimIdAllocator,
        gate_a: GateId,
        gate_b: GateId,
        transit: Ticks,
        kind: LaneKind,
    ) -> LaneId {
        let id = LaneId(ids.allocate());
        for gate in [gate_a, gate_b] {
            let lanes = &mut self
                .gates
                .get_mut(&gate)
                .expect("invariant: a lane joins existing gates")
                .lanes;
            if !lanes.contains(&id) {
                lanes.push(id);
            }
        }
        self.lanes.insert(
            id,
            Lane {
                gate_a,
                gate_b,
                transit,
                kind,
            },
        );
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_links_both_directions_and_jumps_follow_lanes() {
        let mut ids = SimIdAllocator::default();
        let mut g = Galaxy::default();
        let sector = g.add_sector(&mut ids);
        let a = g.add_system(&mut ids, sector, SystemKind::Home);
        let b = g.add_system(&mut ids, sector, SystemKind::Standard);
        let c = g.add_system(&mut ids, sector, SystemKind::Hub);
        let site = g.add_site(&mut ids, a, PositionMetres::ORIGIN);
        let ga = g.add_gate(&mut ids, a, PositionMetres::ORIGIN);
        let gb1 = g.add_gate(&mut ids, b, PositionMetres::ORIGIN);
        let gb2 = g.add_gate(&mut ids, b, PositionMetres::ORIGIN);
        let gc = g.add_gate(&mut ids, c, PositionMetres::ORIGIN);
        let ab = g.add_lane(&mut ids, ga, gb1, Ticks::from_minutes(50), LaneKind::Jump);
        g.add_lane(&mut ids, gb2, gc, Ticks::from_minutes(50), LaneKind::Jump);

        assert_eq!(g.sector(sector).unwrap().systems, vec![a, b, c]);
        assert_eq!(g.system(a).unwrap().sites, vec![site]);
        assert_eq!(g.gate(ga).unwrap().lanes, vec![ab]);
        assert_eq!(g.lane(ab).unwrap().other_end(gb1), Some(ga));
        assert_eq!(g.neighbours(b), BTreeSet::from([a, c]));
        let jumps = g.jumps_from(a);
        assert_eq!(jumps.get(&c), Some(&2));
        assert_eq!(jumps.get(&a), Some(&0));
    }
}
