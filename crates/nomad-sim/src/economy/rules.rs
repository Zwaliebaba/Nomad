//! The economy section of the rules file (D-104; doc 14 §6–§10, §12; D-126 to D-138).
//!
//! Amounts in the file are whole units (alloy, crystal, ore) and rates are per hour; the
//! simulation works in milli-units (D-049). Power is in MW in the file and kW in the
//! simulation, so allocation rounds in integers. Every number is a starting value tuned
//! against the pacing targets (D-070, D-137).

use std::collections::BTreeMap;

use nomad_types::{ModuleKind, ShipClass, Slot};
use serde::{Deserialize, Serialize};

use crate::rules::RulesError;

/// Milli-units per unit (D-049).
pub const MILLI: u64 = 1_000;
/// kW per MW.
pub const KW_PER_MW: u64 = 1_000;

/// The economy numbers of a season.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EconomyRules {
    /// Cores: supply, hull and deployment (doc 14 §9.3).
    pub core: CoreRules,
    /// The core kit item (doc 14 §9.3; doc 15 §8).
    pub core_kit: Cost,
    /// Every other module kind (doc 14 §9.3).
    pub modules: BTreeMap<ModuleKind, ModuleRules>,
    /// The ships a commander can build (doc 14 §9.4; doc 15 §12 command costs).
    pub ships: BTreeMap<ShipClass, ShipRules>,
    /// Harvesting, refining and extraction (doc 14 §6; D-126).
    pub production: ProductionRules,
    /// Power draws that are not a module's own (doc 14 §7; D-127, D-128).
    pub power: PowerRules,
    /// Pool cap and local buffers (doc 14 §4; D-025, D-064).
    pub storage: StorageRules,
    /// MULE cargo, convoys and claims (D-133, D-134, D-135).
    pub logistics: LogisticsRules,
    /// The command cap (doc 14 §8; D-129).
    pub command_cap: CommandCapRules,
    /// Repair (doc 14 §10; D-071).
    pub repair: RepairRules,
    /// The build queue (D-068).
    pub queue: QueueRules,
    /// The economic part of the start kit (doc 14 §12; D-112).
    pub start: StartRules,
    /// The slot template (D-130, D-131).
    pub slots: SlotTemplate,
}

/// What an item costs and how long it takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cost {
    /// Alloy.
    pub alloy: u64,
    /// Crystal.
    pub crystal: u64,
    /// Build time in seconds.
    pub build_s: u64,
}

/// Cores (doc 14 §9.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoreRules {
    /// A mother-station core's supply.
    pub mother_supply_mw: u64,
    /// Any other core's supply.
    pub supply_mw: u64,
    /// A mother-station core's hull.
    pub mother_hull: u32,
    /// Any other core's hull.
    pub hull: u32,
    /// Deployment of a core kit until the outpost becomes a holding (D-054).
    pub deploy_s: u64,
}

/// One module kind (doc 14 §9.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleRules {
    /// Cost and build time.
    pub cost: Cost,
    /// Power drawn while working (doc 14 §7). The dry dock's draw applies while it builds.
    pub draw_mw: u64,
    /// Power supplied (solar arrays).
    pub supply_mw: u64,
    /// Hull.
    pub hull: u32,
}

/// One buildable ship class (doc 14 §9.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShipRules {
    /// Cost and build time.
    pub cost: Cost,
    /// Command cost (doc 15 §12).
    pub command: u32,
}

/// Production (doc 14 §6; D-126).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductionRules {
    /// Ore per hour of a harvester on a standard field.
    pub harvester_ore_per_h: u64,
    /// Alloy per hour a refinery can make.
    pub refinery_alloy_per_h: u64,
    /// Crystal per hour of an extractor on a standard vein.
    pub extractor_crystal_per_h: u64,
    /// The ore buffer holds this many hours of harvesting.
    pub ore_buffer_h: u64,
}

/// Power draws that belong to no single module (doc 14 §7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PowerRules {
    /// Regeneration draw of a sector at full maximum and full rate (D-142 scales it).
    pub regen_mw_per_sector: u64,
}

/// Pool cap and buffers (doc 14 §4, §9.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageRules {
    /// Pool cap from the mother station, alloy.
    pub base_alloy: u64,
    /// Pool cap from the mother station, crystal.
    pub base_crystal: u64,
    /// Added by each storage module, alloy.
    pub rack_alloy: u64,
    /// Added by each storage module, crystal.
    pub rack_crystal: u64,
    /// A remote holding's buffer holds this many hours of its production.
    pub buffer_h: u64,
}

/// MULEs, convoys and claims (D-133, D-134, D-135).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogisticsRules {
    /// Cargo units per MULE (1 alloy or 1 crystal = 1 unit; a core kit fills the hold).
    pub mule_units: u64,
    /// A convoy is at a stop when its anchor is this close to the holding's launch point.
    pub stop_radius_m: f64,
    /// A core kit can be deployed this close to the site.
    pub deploy_range_m: f64,
    /// Longest convoy route.
    pub max_route_stops: usize,
}

/// The command cap (D-129).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandCapRules {
    /// From the mother station.
    pub base: u32,
    /// Per other completed holding.
    pub per_holding: u32,
    /// At most.
    pub max: u32,
}

/// Repair (D-071).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepairRules {
    /// Free repair, in per cent of hull per minute.
    pub free_percent_per_min: u32,
    /// Free repair waits this long after the last damage.
    pub damage_grace_s: u64,
    /// The REPAIR ability's alloy use per second.
    pub ability_alloy_per_s: u64,
    /// The REPAIR ability's hull restored per second.
    pub ability_hull_per_s: u32,
}

/// The build queue (D-068).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueueRules {
    /// Most items in one holding's queue.
    pub max_items: usize,
}

/// The economic part of the start kit (D-112).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartRules {
    /// Starting alloy.
    pub alloy: u64,
    /// Starting crystal.
    pub crystal: u64,
    /// The mother station's modules besides its core, complete at the start.
    pub modules: Vec<StartModule>,
}

/// One start-kit module.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartModule {
    /// The module.
    pub kind: ModuleKind,
    /// Its slot.
    pub slot: Slot,
}

/// The slot template (D-130, D-131), in grid voxels with the origin at the core.
///
/// *Proposed* positions within the agreed template: D-130 fixes the slot counts and which
/// slots lie inside the dome; the exact positions follow the art pass that brings the
/// phase-1 scene into line (D-119).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SlotTemplate {
    /// Every core's dome radius (D-141).
    pub dome_radius_voxels: i32,
    /// The four pylon slots, one per shield sector, in sector order.
    pub pylons: [[i32; 2]; 4],
    /// Standard inner slots; the mother station has `mother_inner` of them, other holdings
    /// `other_inner`, the first ones in this list.
    pub inner: Vec<[i32; 2]>,
    /// Standard inner slots of a mother station.
    pub mother_inner: usize,
    /// Standard inner slots of any other holding.
    pub other_inner: usize,
    /// The dock slot.
    pub dock: [i32; 2],
    /// The solar slot.
    pub solar: [i32; 2],
    /// Deposit slots, by deposit index.
    pub deposits: [[i32; 2]; 2],
    /// Hit radius per slot type in voxels, for S5 hit tests: core, pylon, inner, dock,
    /// solar, deposit.
    pub radius_voxels: SlotRadii,
}

/// Hit radius per slot type, in voxels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SlotRadii {
    /// The core.
    pub core: i32,
    /// A pylon.
    pub pylon: i32,
    /// A standard inner module.
    pub inner: i32,
    /// The dry dock.
    pub dock: i32,
    /// The solar array.
    pub solar: i32,
    /// A deposit module.
    pub deposit: i32,
}

impl EconomyRules {
    /// The rules of a module kind other than the core.
    pub fn module(&self, kind: ModuleKind) -> &ModuleRules {
        self.modules
            .get(&kind)
            .expect("invariant: checked rules have every non-core module kind")
    }

    /// The rules of a buildable ship class, or `None` for NPC classes.
    pub fn ship(&self, class: ShipClass) -> Option<&ShipRules> {
        self.ships.get(&class)
    }

    /// Checks every value; called by [`crate::rules::Rules::check`].
    pub(crate) fn check(&self) -> Result<(), RulesError> {
        self.check_tables()?;
        self.check_values()
    }

    /// Module and ship tables.
    fn check_tables(&self) -> Result<(), RulesError> {
        const NON_CORE: [ModuleKind; 8] = [
            ModuleKind::Pylon,
            ModuleKind::Solar,
            ModuleKind::Harvester,
            ModuleKind::Refinery,
            ModuleKind::Extractor,
            ModuleKind::Storage,
            ModuleKind::Turret,
            ModuleKind::DryDock,
        ];
        let err = |field: String, reason: &'static str| RulesError::Invalid { field, reason };
        if self.modules.contains_key(&ModuleKind::Core) {
            return Err(err(
                "economy.modules.Core".into(),
                "cores are set in economy.core",
            ));
        }
        for kind in NON_CORE {
            let m = self
                .modules
                .get(&kind)
                .ok_or_else(|| err(format!("economy.modules.{kind:?}"), "is missing"))?;
            if m.hull == 0 {
                return Err(err(format!("economy.modules.{kind:?}.hull"), "must be > 0"));
            }
            if m.cost.build_s == 0 {
                return Err(err(
                    format!("economy.modules.{kind:?}.cost.build_s"),
                    "must be > 0",
                ));
            }
        }
        if self.ships.is_empty() {
            return Err(err("economy.ships".into(), "is empty"));
        }
        for (class, ship) in &self.ships {
            if ship.cost.build_s == 0 || ship.command == 0 {
                return Err(err(
                    format!("economy.ships.{class:?}"),
                    "needs a build time and a command cost",
                ));
            }
        }
        Ok(())
    }

    /// Every other value.
    fn check_values(&self) -> Result<(), RulesError> {
        let err = |field: String, reason: &'static str| RulesError::Invalid { field, reason };
        let c = &self.core;
        if c.mother_hull == 0 || c.hull == 0 || c.deploy_s == 0 {
            return Err(err(
                "economy.core".into(),
                "needs hull and a deployment time",
            ));
        }
        if self.core_kit.build_s == 0 {
            return Err(err("economy.core_kit.build_s".into(), "must be > 0"));
        }
        let p = &self.production;
        if p.ore_buffer_h == 0 {
            return Err(err("economy.production.ore_buffer_h".into(), "must be > 0"));
        }
        if self.storage.buffer_h == 0 {
            return Err(err("economy.storage.buffer_h".into(), "must be > 0"));
        }
        let l = &self.logistics;
        if l.mule_units == 0 || l.max_route_stops == 0 {
            return Err(err(
                "economy.logistics".into(),
                "needs MULE cargo and route stops",
            ));
        }
        for (field, value) in [
            ("stop_radius_m", l.stop_radius_m),
            ("deploy_range_m", l.deploy_range_m),
        ] {
            if !(value.is_finite() && value > 0.0) {
                return Err(err(format!("economy.logistics.{field}"), "must be > 0"));
            }
        }
        let cap = &self.command_cap;
        if cap.base == 0 || cap.max < cap.base {
            return Err(err(
                "economy.command_cap".into(),
                "needs a base no larger than the maximum",
            ));
        }
        if self.queue.max_items == 0 {
            return Err(err("economy.queue.max_items".into(), "must be > 0"));
        }
        let t = &self.slots;
        if t.dome_radius_voxels <= 0 {
            return Err(err(
                "economy.slots.dome_radius_voxels".into(),
                "must be > 0",
            ));
        }
        if t.mother_inner > t.inner.len() || t.other_inner > t.inner.len() {
            return Err(err(
                "economy.slots.inner".into(),
                "has fewer positions than a holding's inner slots",
            ));
        }
        if t.inner.len() > usize::from(u8::MAX) {
            return Err(err("economy.slots.inner".into(), "has too many positions"));
        }
        for start in &self.start.modules {
            if start.kind == ModuleKind::Core {
                return Err(err(
                    "economy.start.modules".into(),
                    "the core comes with the station",
                ));
            }
        }
        Ok(())
    }
}
