//! What one evaluation knows, and the economic model it values steps with (A2 plan §4.2,
//! §4.4, §4.5; D-066, D-217, D-281).
//!
//! **Features** ([`Features`]) are read once per evaluation from the view. The topology is
//! small at stage 1 (about 10 systems and 60 sites), so the jump table is rebuilt each time
//! rather than cached in memory (A2 plan §4.2 allows a cache; nothing needs one yet).
//!
//! **Value** (D-217, D-281, the tuning study's note): a step's value is its effect on the
//! commander's output, not its nominal yield. Each remote holding's alloy output is scaled by
//! min(1, F ÷ the commander's alloy-producing holdings other than the mother station), and
//! its crystal output likewise by the crystal producers (D-281). So beyond about F producers,
//! a holding as rich as the others adds nothing, a richer one adds a little and a poorer one
//! lowers the total: those steps score zero or less and are not built (A2 plan §5.3: "a poor
//! site is better held for control without a harvester"). Values are milli-units per hour,
//! with crystal weighed 3× (D-066).

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use nomad_types::{
    BuildItem, CommanderId, DepositKind, FleetId, HoldingId, ModuleKind, ShipClass, SiteId,
    SiteKind, Slot, Stock, SystemId, SystemKind, Tick,
};

use super::view::{
    DepositInfo, EconomyNumbers, EconomyStatus, HoldingStatus, SiteInfo, StrategicView, SystemInfo,
};
use crate::view::{FleetPlace, OwnFleet};

/// Milli-units per unit.
const MILLI: u64 = 1_000;

/// One evaluation's reading of the view.
#[derive(Clone)]
pub(crate) struct Features {
    pub(crate) now: Tick,
    pub(crate) me: CommanderId,
    pub(crate) economy: EconomyStatus,
    /// Own holdings and outposts, in identity order.
    pub(crate) holdings: Vec<HoldingStatus>,
    /// Own fleets, in identity order.
    pub(crate) fleets: Vec<OwnFleet>,
    /// Own fleets on a convoy route.
    pub(crate) convoys: BTreeSet<FleetId>,
    pub(crate) sites: Vec<SiteInfo>,
    pub(crate) systems: Vec<SystemInfo>,
    pub(crate) numbers: EconomyNumbers,
    /// Jumps from the nearest raider haven, per system.
    pub(crate) haven_jumps: BTreeMap<SystemId, u32>,
    /// Hostile ships detected or remembered, per system.
    pub(crate) hostiles: BTreeMap<SystemId, u32>,
    /// The season's final tick.
    pub(crate) final_tick: Tick,
}

impl Features {
    pub(crate) fn read(view: &impl StrategicView, me: CommanderId) -> Option<Self> {
        let economy = view.economy()?;
        let holdings: Vec<HoldingStatus> = view
            .own_holdings()
            .iter()
            .filter_map(|h| view.holding_status(h.id))
            .collect();
        let fleets = view
            .own_fleet_ids()
            .into_iter()
            .filter_map(|id| view.own_fleet(id))
            .collect();
        let convoys = view.own_convoys().into_iter().map(|c| c.fleet).collect();
        let systems = view.systems();
        let havens: Vec<SystemId> = systems
            .iter()
            .filter(|s| s.kind == SystemKind::RaiderHaven)
            .map(|s| s.id)
            .collect();
        let haven_jumps = jumps_from(&systems, &havens);
        let hostiles = systems
            .iter()
            .map(|s| (s.id, view.hostile_ships_in(s.id)))
            .filter(|&(_, n)| n > 0)
            .collect();
        Some(Self {
            now: view.now(),
            me,
            economy,
            holdings,
            fleets,
            convoys,
            sites: view.sites(),
            systems,
            numbers: view.numbers(),
            haven_jumps,
            hostiles,
            final_tick: view.final_tick(),
        })
    }

    pub(crate) fn holding(&self, id: HoldingId) -> Option<&HoldingStatus> {
        self.holdings.iter().find(|h| h.id == id)
    }

    pub(crate) fn site(&self, id: SiteId) -> Option<&SiteInfo> {
        self.sites.iter().find(|s| s.id == id)
    }

    pub(crate) fn system(&self, id: SystemId) -> Option<&SystemInfo> {
        self.systems.iter().find(|s| s.id == id)
    }

    /// Own holdings with a complete core: they can build (D-041).
    pub(crate) fn complete(&self) -> impl Iterator<Item = &HoldingStatus> {
        self.holdings.iter().filter(|h| h.complete)
    }

    /// The site's deposits.
    pub(crate) fn deposits(&self, site: SiteId) -> &[DepositInfo] {
        self.site(site).map_or(&[], |s| s.deposits.as_slice())
    }

    /// Jumps from each own system, per system.
    pub(crate) fn own_jumps(&self) -> BTreeMap<SystemId, u32> {
        let own: Vec<SystemId> = self.complete().map(|h| h.system).collect();
        jumps_from(&self.systems, &own)
    }

    /// Whether the commander has a depot in `system` (D-064): a holding there delivers to
    /// the pool.
    pub(crate) fn depot_in(&self, system: SystemId) -> bool {
        self.holdings
            .iter()
            .any(|h| h.system == system && h.complete && is_depot(h))
    }

    /// Whether crystal income is zero and nothing will change that soon: no extractor built
    /// or being built (A2 plan §4.4 "crystal first").
    pub(crate) fn crystal_starved(&self) -> bool {
        self.economy.income_last_hour.crystal == 0
            && !self.holdings.iter().any(|h| {
                h.modules.iter().any(|m| m.kind == ModuleKind::Extractor)
                    || h.queue.iter().any(|q| {
                        matches!(
                            q.item,
                            BuildItem::Module {
                                kind: ModuleKind::Extractor,
                                ..
                            }
                        )
                    })
            })
    }

    /// Hours left in the season.
    pub(crate) fn hours_left(&self) -> u64 {
        self.final_tick.since(self.now).get() / TICKS_PER_HOUR
    }

    /// Prices of queue items that wait to be paid, kits aside: they will take that much from
    /// the pool when they start (D-068).
    pub(crate) fn committed(&self) -> Stock {
        self.holdings
            .iter()
            .flat_map(|h| h.queue.iter())
            .filter(|q| !q.started && q.item != BuildItem::CoreKit)
            .fold(Stock::ZERO, |sum, q| sum.plus(q.price))
    }

    /// Command cost of ships queued and not started.
    pub(crate) fn command_queued(&self) -> u32 {
        self.holdings
            .iter()
            .flat_map(|h| h.queue.iter())
            .filter(|q| !q.started)
            .filter_map(|q| match q.item {
                BuildItem::Ship { class } => self.numbers.ship(class).map(|s| s.1),
                _ => None,
            })
            .sum()
    }

    /// The model of the commander's output now.
    pub(crate) fn output(&self) -> Output {
        let mut out = Output::default();
        for h in &self.holdings {
            if !h.complete {
                continue;
            }
            let (alloy, crystal) = nominal(&self.numbers, self.deposits(h.site), &h.modules, &[]);
            if h.mother {
                out.mother = out.mother.plus(Stock { alloy, crystal });
            } else {
                out.remote.insert(h.id, Stock { alloy, crystal });
            }
        }
        out
    }

    /// Whether the fleet is a lone, empty MULE fleet docked at `holding`, free for a plan.
    pub(crate) fn is_free_mule(
        &self,
        view: &impl StrategicView,
        fleet: &OwnFleet,
        busy: &BTreeSet<FleetId>,
    ) -> bool {
        !busy.contains(&fleet.id)
            && !self.convoys.contains(&fleet.id)
            && !fleet.busy
            && !fleet.ships.is_empty()
            && fleet.ships.iter().all(|&(ship, class)| {
                class == ShipClass::Mule
                    && view
                        .mule_cargo(ship)
                        .is_some_and(|c| c.stock.is_zero() && !c.core_kit)
            })
    }

    /// The holding a fleet is docked at.
    pub(crate) fn docked_at(fleet: &OwnFleet) -> Option<HoldingId> {
        match fleet.place {
            FleetPlace::Docked(h) => Some(h),
            FleetPlace::InSpace(_) | FleetPlace::Travelling => None,
        }
    }
}

/// Ticks per hour.
pub(crate) const TICKS_PER_HOUR: u64 = 36_000;

/// Whether a holding is a depot (D-064): the mother station or a completed storage module.
pub(crate) fn is_depot(h: &HoldingStatus) -> bool {
    h.mother
        || h.modules
            .iter()
            .any(|m| m.kind == ModuleKind::Storage && !m.building)
}

/// Jumps from the nearest of `from`, by breadth-first search over lanes.
pub(crate) fn jumps_from(systems: &[SystemInfo], from: &[SystemId]) -> BTreeMap<SystemId, u32> {
    let mut jumps = BTreeMap::new();
    let mut queue = VecDeque::new();
    for &s in from {
        if jumps.insert(s, 0).is_none() {
            queue.push_back(s);
        }
    }
    while let Some(s) = queue.pop_front() {
        let here = jumps.get(&s).copied().unwrap_or(0);
        let Some(system) = systems.iter().find(|x| x.id == s) else {
            continue;
        };
        for &n in &system.neighbours {
            if let std::collections::btree_map::Entry::Vacant(e) = jumps.entry(n) {
                e.insert(here + 1);
                queue.push_back(n);
            }
        }
    }
    jumps
}

/// The kind of a site, by its deposits (D-215).
pub(crate) fn site_kind(deposits: &[DepositInfo], mother: bool) -> SiteKind {
    if mother {
        return SiteKind::Mother;
    }
    let ore = deposits
        .iter()
        .filter(|d| d.kind == DepositKind::OreField)
        .count();
    let crystal = deposits
        .iter()
        .filter(|d| d.kind == DepositKind::CrystalVein)
        .count();
    SiteKind::from_deposits(ore, crystal)
}

/// Which kinds go in a slot, given the site's deposits (D-130, D-131): the validator's rule,
/// so the AI does not plan what cannot be queued.
pub(crate) fn fits(kind: ModuleKind, slot: Slot, deposits: &[DepositInfo]) -> bool {
    match (slot, kind) {
        (Slot::Pylon(_), ModuleKind::Pylon)
        | (Slot::Dock, ModuleKind::DryDock)
        | (Slot::Solar, ModuleKind::Solar)
        | (Slot::Inner(_), ModuleKind::Refinery | ModuleKind::Storage | ModuleKind::Turret) => true,
        (Slot::Deposit(i), ModuleKind::Harvester) => deposits
            .get(usize::from(i))
            .is_some_and(|d| d.kind == DepositKind::OreField),
        (Slot::Deposit(i), ModuleKind::Extractor) => deposits
            .get(usize::from(i))
            .is_some_and(|d| d.kind == DepositKind::CrystalVein),
        _ => false,
    }
}

/// A holding's nominal output per hour, milli-units, as the economy computes it: harvested
/// ore up to the refineries' capacity as alloy, and extracted crystal (doc 14 §6). Completed
/// modules count, and `extra` modules as if they were complete.
pub(crate) fn nominal(
    numbers: &EconomyNumbers,
    deposits: &[DepositInfo],
    modules: &[super::view::ModuleStatus],
    extra: &[(ModuleKind, Slot)],
) -> (u64, u64) {
    let built = modules
        .iter()
        .filter(|m| !m.building)
        .map(|m| (m.kind, m.slot));
    let (mut ore, mut refine, mut crystal) = (0_u64, 0_u64, 0_u64);
    for (kind, slot) in built.chain(extra.iter().copied()) {
        let richness = match slot {
            Slot::Deposit(i) => deposits
                .get(usize::from(i))
                .map_or(0, |d| u64::from(d.richness_permille)),
            _ => MILLI,
        };
        match kind {
            ModuleKind::Harvester => ore += numbers.harvester_ore_per_h * richness,
            ModuleKind::Refinery => refine += numbers.refinery_alloy_per_h * MILLI,
            ModuleKind::Extractor => crystal += numbers.extractor_crystal_per_h * richness,
            _ => {}
        }
    }
    (ore.min(refine), crystal)
}

/// The commander's output by holding, nominal (before D-217).
#[derive(Clone, Debug, Default)]
pub(crate) struct Output {
    pub(crate) mother: Stock,
    pub(crate) remote: BTreeMap<HoldingId, Stock>,
}

impl Output {
    /// The commander's output per hour after D-217 and D-281, milli-units.
    pub(crate) fn scaled(&self, f_permille: u32) -> Stock {
        let (mut alloy, mut n_alloy, mut crystal, mut n_crystal) = (0_u64, 0_u64, 0_u64, 0_u64);
        for out in self.remote.values() {
            if out.alloy > 0 {
                alloy += out.alloy;
                n_alloy += 1;
            }
            if out.crystal > 0 {
                crystal += out.crystal;
                n_crystal += 1;
            }
        }
        self.mother.plus(Stock {
            alloy: diminish(alloy, n_alloy, f_permille),
            crystal: diminish(crystal, n_crystal, f_permille),
        })
    }

    /// The change in scaled output, as weighed value per hour, if `holding` (remote, or new
    /// with `None`) produced `to` instead of what it does now.
    pub(crate) fn marginal(
        &self,
        holding: Option<HoldingId>,
        mother: bool,
        to: Stock,
        f_permille: u32,
        crystal_weight: u64,
    ) -> i64 {
        let before = self.scaled(f_permille);
        let mut after = self.clone();
        if mother {
            after.mother = to;
        } else {
            let key = holding.unwrap_or(HoldingId(nomad_types::SimId::from_raw(u64::MAX)));
            after.remote.insert(key, to);
        }
        let after = after.scaled(f_permille);
        let delta = |a: u64, b: u64| {
            i64::try_from(a).unwrap_or(i64::MAX) - i64::try_from(b).unwrap_or(i64::MAX)
        };
        delta(after.alloy, before.alloy)
            + delta(after.crystal, before.crystal) * i64::try_from(crystal_weight).unwrap_or(3)
    }
}

/// `total` scaled by min(1, F ÷ n) (D-217), rounded down.
fn diminish(total: u64, n: u64, f_permille: u32) -> u64 {
    if n == 0 {
        return 0;
    }
    let f = u128::from(f_permille);
    let n = u128::from(n) * u128::from(MILLI);
    if f >= n {
        return total;
    }
    u64::try_from(u128::from(total) * f / n).unwrap_or(u64::MAX)
}

/// A stock as one weighed amount, milli-units (D-066: crystal 3×).
pub(crate) fn weighed(stock: Stock, crystal_weight: u64) -> u64 {
    stock
        .alloy
        .saturating_add(stock.crystal.saturating_mul(crystal_weight))
}

/// The score of `value` per hour for `cost`: per million of the cost returned per hour.
pub(crate) fn return_score(value: i64, cost: u64) -> i64 {
    if value <= 0 || cost == 0 {
        return 0;
    }
    let score = i128::from(value) * 1_000_000 / i128::from(cost);
    i64::try_from(score).unwrap_or(i64::MAX)
}

/// What an unaffordable `price` should hold back from `free` (A2 plan §4.1 "reserving each
/// plan's next costs"). *Proposed:* per resource, what the price will still need from the
/// pool when its scarcest resource arrives at the current income: the scarce resource is
/// held entirely, and of another only what income will not have refilled by then. So alloy
/// that would otherwise overflow while a kit waits for crystal can still be spent.
pub(crate) fn reservation(price: Stock, free: Stock, income: Stock) -> Stock {
    let wait = |need: u64, have: u64, rate: u64| -> Option<u64> {
        if have >= need {
            Some(0)
        } else if rate == 0 {
            None
        } else {
            // Seconds until the pool has it.
            Some((need - have).saturating_mul(3_600).div_ceil(rate))
        }
    };
    let waits = [
        wait(price.alloy, free.alloy, income.alloy),
        wait(price.crystal, free.crystal, income.crystal),
    ];
    let Some(longest) = waits.iter().try_fold(0_u64, |acc, w| w.map(|w| acc.max(w))) else {
        // It never comes at this income: hold what there is of what it needs.
        return Stock {
            alloy: price.alloy.min(free.alloy),
            crystal: price.crystal.min(free.crystal),
        };
    };
    let held = |need: u64, have: u64, rate: u64| -> u64 {
        let refilled = rate.saturating_mul(longest) / 3_600;
        need.saturating_sub(refilled).min(have)
    };
    Stock {
        alloy: held(price.alloy, free.alloy, income.alloy),
        crystal: held(price.crystal, free.crystal, income.crystal),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diminishing_yield_bounds_remote_output_per_resource() {
        let f = 2_500;
        let mut out = Output {
            mother: Stock::units(600, 0),
            remote: BTreeMap::new(),
        };
        let ore = Stock::units(600, 0);
        for i in 0..2 {
            out.remote
                .insert(HoldingId(nomad_types::SimId::from_raw(i)), ore);
        }
        // Two producers under F: a third of the same yield still adds 600 A/h... scaled.
        let third = out.marginal(None, false, ore, f, 3);
        assert_eq!(third, 1_500_000 - 1_200_000);
        for i in 2..4 {
            out.remote
                .insert(HoldingId(nomad_types::SimId::from_raw(i)), ore);
        }
        // Four producers above F: a fifth of the same yield adds nothing, a poorer one less.
        assert_eq!(out.marginal(None, false, ore, f, 3), 0);
        assert!(out.marginal(None, false, Stock::units(300, 0), f, 3) < 0);
        // A crystal holding does not count against the alloy producers (D-281).
        assert_eq!(
            out.marginal(None, false, Stock::units(0, 60), f, 3),
            3 * 60_000
        );
    }

    #[test]
    fn a_reservation_holds_only_what_will_still_be_needed() {
        // A kit waits for crystal: 7,500 A + 625 C with 8,000 A and 325 C, at 600 A/h and
        // 60 C/h. Crystal arrives in 5 h, when income has refilled 3,000 A.
        let r = reservation(
            Stock::units(7_500, 625),
            Stock::units(8_000, 325),
            Stock::units(600, 60),
        );
        assert_eq!(r, Stock::units(4_500, 325));
        // Affordable: everything is held.
        let r = reservation(Stock::units(100, 0), Stock::units(500, 0), Stock::ZERO);
        assert_eq!(r, Stock::units(100, 0));
        // Never at this income: what there is.
        let r = reservation(Stock::units(100, 50), Stock::units(500, 10), Stock::ZERO);
        assert_eq!(r, Stock::units(100, 10));
    }

    #[test]
    fn site_kinds_follow_the_deposits() {
        let ore = DepositInfo {
            kind: DepositKind::OreField,
            richness_permille: 1_000,
        };
        let crystal = DepositInfo {
            kind: DepositKind::CrystalVein,
            richness_permille: 1_000,
        };
        assert_eq!(site_kind(&[], false), SiteKind::Empty);
        assert_eq!(site_kind(&[ore], false), SiteKind::Ore);
        assert_eq!(site_kind(&[ore, ore], false), SiteKind::TwoOre);
        assert_eq!(site_kind(&[crystal], false), SiteKind::Crystal);
        assert_eq!(site_kind(&[crystal, ore], false), SiteKind::OreAndCrystal);
        assert_eq!(site_kind(&[ore], true), SiteKind::Mother);
        assert!(fits(ModuleKind::Extractor, Slot::Deposit(0), &[crystal]));
        assert!(!fits(ModuleKind::Harvester, Slot::Deposit(0), &[crystal]));
        assert!(!fits(ModuleKind::Harvester, Slot::Deposit(1), &[ore]));
    }
}
