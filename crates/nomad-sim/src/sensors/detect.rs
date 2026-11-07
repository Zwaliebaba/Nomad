//! The detection pass (D-072, D-073, D-173 to D-176; S7 plan §4.1–§4.3).
//!
//! Brute force per system, which the plan measured as enough at MVP scale: for each observer
//! present in a system (a sensor there, or control of it) and each target there that is not
//! its own, stop at the first of its sensors in range. Systems in `SystemId` order, observers
//! in `Owner` order, targets in `SimId` order; the comparison is on squared distances, so no
//! square root is taken (D-049). Doc 16 §9 item 5's spatial grid and staggering remain the
//! path for the full 100-commander Universe.

use std::collections::{BTreeMap, BTreeSet};

use nomad_types::{CommanderId, ControlState, FleetId, HoldingId, Owner, ShipId, SystemId};

use super::{Picture, SystemPicture};
use crate::combat::ShipIndex;
use crate::geometry::Vec2;
use crate::state::UniverseState;

/// What a sensors pass costs (S7 plan §6 task 9).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DetectionStats {
    /// Sensors in space and at completed cores.
    pub sensors: u64,
    /// Ships in space and holdings.
    pub targets: u64,
    /// Distance comparisons made.
    pub checks: u64,
    /// Detections, summed over observers.
    pub detections: u64,
}

/// The smallest `Owner`, where a range over `(SystemId, Owner)` keys of one system starts.
const FIRST_OWNER: Owner = Owner::Commander(CommanderId(0));

/// One sensor: where it is and how far it reaches at signature 1.0, in whole metres.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Sensor {
    position: Vec2,
    range_m: u32,
}

/// Whether some sensor of `sensors` reaches a target of `signature_permille` at `at`:
/// `distance² ≤ (range × signature)²`. Ranges are whole metres and signatures thousandths, so
/// `range × signature` is exact and the boundary is bit-exact (D-049).
fn reaches(sensors: &[Sensor], at: Vec2, signature_permille: u32, checks: &mut u64) -> bool {
    sensors.iter().any(|s| {
        *checks += 1;
        let reach = f64::from(s.range_m) * f64::from(signature_permille) / 1_000.0;
        (at - s.position).length_squared() <= reach * reach
    })
}

/// Who covers what at the last pass, for D-178's clearing of stale fleet entries.
#[derive(Default)]
pub(crate) struct Coverage {
    sensors: BTreeMap<(SystemId, Owner), Vec<Sensor>>,
    controlled: BTreeMap<SystemId, Owner>,
}

impl Coverage {
    /// Whether `observer`'s picture covers `point` of `system` for a target of
    /// `signature_permille`: it controls the system, or one of its sensors reaches the point.
    pub(crate) fn covers(
        &self,
        observer: Owner,
        system: SystemId,
        point: Vec2,
        signature_permille: u32,
    ) -> bool {
        self.controlled.get(&system) == Some(&observer)
            || self
                .sensors
                .get(&(system, observer))
                .is_some_and(|sensors| reaches(sensors, point, signature_permille, &mut 0))
    }
}

/// What one pass found.
pub(crate) struct Detection {
    /// Each observer's new picture; observers that detect nothing are absent.
    pub(crate) pictures: BTreeMap<Owner, Picture>,
    pub(crate) coverage: Coverage,
    pub(crate) stats: DetectionStats,
}

/// A target of one system: a ship in space or a holding, with its signature (D-072, D-175).
struct Target {
    kind: TargetKind,
    owner: Owner,
    position: Vec2,
    signature_permille: u32,
}

enum TargetKind {
    Ship(ShipId, FleetId),
    Holding(HoldingId),
}

/// The targets and sensors of every system.
#[derive(Default)]
struct Scene {
    /// Per system: ships in `ShipId` order, then holdings in `HoldingId` order.
    targets: BTreeMap<SystemId, Vec<Target>>,
    sensors: BTreeMap<(SystemId, Owner), Vec<Sensor>>,
}

/// Collects targets and sensors (D-073, D-176).
fn scene(state: &UniverseState) -> Scene {
    let rules = &state.rules.sensors;
    let index = ShipIndex::build(state);
    let mut scene = Scene::default();
    for (&system, ships) in &index.systems {
        let targets = scene.targets.entry(system).or_default();
        for entry in &ships.entries {
            let size = state.rules.combat.class(entry.class).size;
            targets.push(Target {
                kind: TargetKind::Ship(entry.id, entry.fleet),
                owner: entry.owner,
                position: entry.position,
                signature_permille: rules.ship_signature(size),
            });
            // Shrouded arrivals see (D-176); docked ships and fleets in transit are not in
            // the index.
            let kind = &state.rules.class(entry.class).sensor;
            if let Some(range_m) = rules.range_m(kind) {
                scene
                    .sensors
                    .entry((system, entry.owner))
                    .or_default()
                    .push(Sensor {
                        position: entry.position,
                        range_m,
                    });
            }
        }
    }
    for (&id, h) in &state.holdings {
        let Some(site) = state.galaxy.site(h.site) else {
            continue;
        };
        let owner = Owner::Commander(h.owner);
        let position = Vec2::from(site.position);
        scene.targets.entry(site.system).or_default().push(Target {
            kind: TargetKind::Holding(id),
            owner,
            position,
            signature_permille: rules.structure_signature_permille,
        });
        // An outpost under construction has no core yet (D-176).
        if h.core_complete {
            scene
                .sensors
                .entry((site.system, owner))
                .or_default()
                .push(Sensor {
                    position,
                    range_m: rules.holding_core_range_m,
                });
        }
    }
    scene
}

/// What `observer` detects among `targets` with its `sensors` there, or everything if it
/// controls the system (`full`).
fn system_picture(
    observer: Owner,
    full: bool,
    sensors: &[Sensor],
    targets: &[Target],
    checks: &mut u64,
) -> SystemPicture {
    let mut picture = SystemPicture::default();
    for target in targets {
        if target.owner == observer
            || !(full || reaches(sensors, target.position, target.signature_permille, checks))
        {
            continue;
        }
        match target.kind {
            TargetKind::Ship(ship, fleet) => {
                picture.ships.push(ship);
                picture.fleets.push(fleet);
            }
            TargetKind::Holding(holding) => picture.holdings.push(holding),
        }
    }
    picture.fleets.sort_unstable();
    picture.fleets.dedup();
    picture
}

/// Runs the detection of one pass on the current state.
pub(crate) fn detect(state: &UniverseState) -> Detection {
    let Scene { targets, sensors } = scene(state);
    let mut stats = DetectionStats {
        sensors: sensors.values().map(|s| s.len() as u64).sum(),
        targets: targets.values().map(|t| t.len() as u64).sum(),
        ..DetectionStats::default()
    };
    // System control covers the whole system, gate approaches included (D-041, D-073).
    let controlled: BTreeMap<SystemId, Owner> = state
        .control_states()
        .into_iter()
        .filter_map(|(system, control)| match control {
            ControlState::Controlled(commander) => Some((system, Owner::Commander(commander))),
            ControlState::Contested | ControlState::Open => None,
        })
        .collect();
    let mut pictures: BTreeMap<Owner, Picture> = BTreeMap::new();
    for (&system, here) in &targets {
        let controller = controlled.get(&system).copied();
        // Every owner with a sensor here: the smallest `Owner` is commander 0, so the range
        // starts at this system's first key.
        let mut observers: BTreeSet<Owner> = sensors
            .range((system, FIRST_OWNER)..)
            .take_while(|((s, _), _)| *s == system)
            .map(|((_, owner), _)| *owner)
            .collect();
        observers.extend(controller);
        for observer in observers {
            let own = sensors
                .get(&(system, observer))
                .map_or(&[][..], Vec::as_slice);
            let full = controller == Some(observer);
            let picture = system_picture(observer, full, own, here, &mut stats.checks);
            if !picture.is_empty() {
                stats.detections += (picture.ships.len() + picture.holdings.len()) as u64;
                pictures
                    .entry(observer)
                    .or_default()
                    .systems
                    .insert(system, picture);
            }
        }
    }
    Detection {
        pictures,
        coverage: Coverage {
            sensors,
            controlled,
        },
        stats,
    }
}
