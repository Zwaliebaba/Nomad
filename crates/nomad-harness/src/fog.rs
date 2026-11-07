//! `--fog`: fog of war against ground truth, and what sensors cost (S7 plan §6 tasks 8 and 9,
//! §7 "`--fog`" and "Performance").
//!
//! 1. **The picture check.** A stage-1 Universe with scripted fleets of commanders 1 and 2
//!    in the standard system between their homes, and NPC raider groups that do not move,
//!    for a simulated day by default. Every fleet holds fire, so nothing is destroyed and the
//!    picture of the last pass still describes the state after the step. Every 4 simulated
//!    minutes, staggered, each fleet of commanders 1 and 2 gets a seeded order: a move, a
//!    patrol, a jump to a neighbouring system, or an Attack on a fleet its commander's
//!    `CommanderView` shows, which it chases without firing. After every sensors pass,
//!    each observer's picture, as its view shows it, is compared with a ground truth computed
//!    here by brute force from public state alone: every sensor against every target, range
//!    × signature, and system control (D-072, D-073, D-176). Memory and lane traffic are
//!    checked too: no fleet entry for a fleet in view or older than its expiry, no holding
//!    entry for a holding in view or one whose owner has changed, no departure kept past its
//!    expiry.
//! 2. **The cost** of the detection pass, timed on the run's own states (stage 1) and in a
//!    stage-2 stress layout: 600 ships of 10 owners fighting in one system, the worst case
//!    the plan names (S7 plan §4.8, §6 task 9). The step's tick time on pass ticks against
//!    other ticks gives the whole phase with its transitions.
//! 3. **Finding each other:** two fleets 40 km apart attack-move towards each other, so the
//!    first sighting, the cruise drop and the first hit come from real detection.
//!
//! S5a's time-to-kill scenarios run with real detection in `--combat`; their fleets start
//! 4 km apart, inside each other's sensor range, so their results are S5a's.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::time::Instant;

use nomad_sim::galaxy::{STAGE1_COMMANDERS, generate_stage1_with_rules};
use nomad_sim::scenario::{place_fleet, sense, set_rules_of_engagement};
use nomad_sim::{
    CombatEvent, Command, CommanderId, CommanderInput, CommanderInputKind, ControlState, FactionId,
    FleetEvent, FleetId, FleetOrder, HoldingId, Location, Owner, Place, Queueing, RngStreams,
    Rules, RulesOfEngagement, SeasonConfig, ShipClass, ShipId, StreamKey, SystemId, SystemKind,
    TICKS_PER_SECOND, TickInputs, UniverseState, Vec2, VisibilityEvent, step,
};

const COMMANDERS: [CommanderId; STAGE1_COMMANDERS] = [
    CommanderId(1),
    CommanderId(2),
    CommanderId(3),
    CommanderId(4),
];
const VX: ShipClass = ShipClass::Vx;
const LANCER: ShipClass = ShipClass::Lancer;
const WARDEN: ShipClass = ShipClass::Warden;
const MULE: ShipClass = ShipClass::Mule;
const RAIDER: ShipClass = ShipClass::RaiderStrikeCraft;
const HSTL: ShipClass = ShipClass::HeavyCruiser;

/// Each scripted fleet gets an order this often, staggered: long enough for a jump to leave.
const ORDER_INTERVAL_TICKS: u64 = 4 * 60 * TICKS_PER_SECOND;

/// Options of a `--fog` run.
pub struct FogRun {
    /// The season seed; the orders are seeded from it too.
    pub seed: u64,
    /// How many ticks the picture check runs.
    pub ticks: u64,
    /// The rules the Universe is created with.
    pub rules: Rules,
}

/// What the picture check counted.
#[derive(Default)]
struct Counts {
    passes: u64,
    comparisons: u64,
    mismatches: Vec<String>,
    sighted: u64,
    lost: u64,
    departures: u64,
    targets_lost: u64,
    applied: u64,
    rejected: BTreeMap<String, u64>,
    most_memory: usize,
    most_traffic: usize,
    detection_micros: Vec<f64>,
    checks_per_pass: Vec<u64>,
}

/// Per observer and system: the ships and holdings it should detect.
type Truth = BTreeMap<Owner, BTreeMap<SystemId, (BTreeSet<ShipId>, BTreeSet<HoldingId>)>>;

/// The ground truth of detection, by brute force over public state (D-072, D-073, D-176).
fn ground_truth(state: &UniverseState) -> Truth {
    let rules = state.rules();
    let sensors_rules = &rules.sensors;
    // (owner, system, position, range at signature 1.0)
    let mut sensors: Vec<(Owner, SystemId, Vec2, u32)> = Vec::new();
    // (ship or holding, owner, system, position, signature)
    let mut ships: Vec<(ShipId, Owner, SystemId, Vec2, u32)> = Vec::new();
    let mut holdings: Vec<(HoldingId, Owner, SystemId, Vec2, u32)> = Vec::new();
    for id in state.ship_ids() {
        let Some(ship) = state.ship(id) else { continue };
        let Place::InSystem(system) = ship.place else {
            continue;
        };
        let at = Vec2::from(ship.position);
        let class = rules.class(ship.class);
        if let Some(range) = sensors_rules.range_m(&class.sensor) {
            sensors.push((ship.owner, system, at, range));
        }
        let size = rules.combat.class(ship.class).size;
        ships.push((
            id,
            ship.owner,
            system,
            at,
            sensors_rules.ship_signature(size),
        ));
    }
    for (id, holding) in state.holdings() {
        let Some(site) = state.galaxy().site(holding.site) else {
            continue;
        };
        let owner = Owner::Commander(holding.owner);
        let at = Vec2::from(site.position);
        if holding.core_complete {
            sensors.push((owner, site.system, at, sensors_rules.holding_core_range_m));
        }
        holdings.push((
            id,
            owner,
            site.system,
            at,
            sensors_rules.structure_signature_permille,
        ));
    }
    let controllers: BTreeMap<SystemId, Owner> = state
        .control_states()
        .into_iter()
        .filter_map(|(system, c)| match c {
            ControlState::Controlled(id) => Some((system, Owner::Commander(id))),
            _ => None,
        })
        .collect();
    let observers: BTreeSet<Owner> = sensors
        .iter()
        .map(|s| s.0)
        .chain(controllers.values().copied())
        .collect();
    let detects = |observer: Owner, system: SystemId, at: Vec2, signature: u32| {
        controllers.get(&system) == Some(&observer)
            || sensors.iter().any(|&(o, s, p, range)| {
                let reach = f64::from(range) * f64::from(signature) / 1_000.0;
                o == observer && s == system && (at - p).length_squared() <= reach * reach
            })
    };
    let mut truth = Truth::new();
    for &observer in &observers {
        for &(id, owner, system, at, signature) in &ships {
            if owner != observer && detects(observer, system, at, signature) {
                let entry = truth
                    .entry(observer)
                    .or_default()
                    .entry(system)
                    .or_default();
                entry.0.insert(id);
            }
        }
        for &(id, owner, system, at, signature) in &holdings {
            if owner != observer && detects(observer, system, at, signature) {
                let entry = truth
                    .entry(observer)
                    .or_default()
                    .entry(system)
                    .or_default();
                entry.1.insert(id);
            }
        }
    }
    truth
}

/// Compares every observer's view with the ground truth.
fn check_pictures(state: &UniverseState, observers: &BTreeSet<Owner>, counts: &mut Counts) {
    let truth = ground_truth(state);
    let mut everyone = observers.clone();
    everyone.extend(truth.keys().copied());
    let systems: Vec<SystemId> = state.galaxy().systems().map(|(id, _)| id).collect();
    let now = state.tick();
    let memory_limit = nomad_sim::Ticks::from_secs(
        state.rules().sensors.fleet_memory_s + state.rules().sensors.memory_sweep_s,
    );
    let mut mismatches = Vec::new();
    let mut fail = |what: String| mismatches.push(format!("tick {}: {what}", now.get()));
    let mut comparisons = 0;
    let (mut most_memory, mut most_traffic) = (0, 0);
    for &observer in &everyone {
        let view = state.view(observer);
        let mine = truth.get(&observer);
        for &system in &systems {
            comparisons += 1;
            let (ships, holdings) = mine
                .and_then(|m| m.get(&system))
                .cloned()
                .unwrap_or_default();
            let seen: BTreeSet<ShipId> = view.sensed_ship_ids_in(system).iter().copied().collect();
            if seen != ships {
                fail(format!(
                    "{observer:?} in {system:?} sees ships {seen:?}, ground truth {ships:?}"
                ));
            }
            let seen: BTreeSet<HoldingId> = view
                .sensed_holdings_in(system)
                .iter()
                .map(|h| h.holding)
                .collect();
            if seen != holdings {
                fail(format!(
                    "{observer:?} in {system:?} sees holdings {seen:?}, ground truth {holdings:?}"
                ));
            }
            // A sensed fleet lists only detected ships (D-174), and nothing of its own.
            for fleet in view.sensed_fleets_in(system) {
                if fleet.owner == observer || fleet.ships.iter().any(|s| !ships.contains(&s.id)) {
                    fail(format!(
                        "{observer:?} sees undetected ships of {:?}",
                        fleet.id
                    ));
                }
            }
        }
        for entry in view.last_known_fleets() {
            if view.sensed_fleet(entry.fleet).is_some() {
                fail(format!(
                    "{observer:?} remembers {:?}, which it sees",
                    entry.fleet
                ));
            }
            if entry.seen_at >= now || now.since(entry.seen_at) > memory_limit {
                fail(format!(
                    "{observer:?}'s entry for {:?} is out of date",
                    entry.fleet
                ));
            }
        }
        for entry in view.last_known_holdings() {
            let holding = entry.holding.holding;
            if view.sensed_holding(holding).is_some()
                || view.holding(holding).map(|h| h.owner) != Some(entry.holding.owner)
            {
                fail(format!("{observer:?}'s entry for {holding:?} is stale"));
            }
        }
        let after = nomad_sim::Ticks::from_secs(state.rules().sensors.lane_traffic_after_arrival_s);
        for record in view.lane_traffic() {
            if now >= record.arrives.after(after) {
                fail(format!(
                    "{observer:?} keeps an expired departure of {:?}",
                    record.fleet
                ));
            }
        }
        most_memory = most_memory.max(view.last_known_fleets().count());
        most_traffic = most_traffic.max(view.lane_traffic().count());
    }
    counts.mismatches.extend(mismatches);
    counts.comparisons += comparisons;
    counts.most_memory = counts.most_memory.max(most_memory);
    counts.most_traffic = counts.most_traffic.max(most_traffic);
}

/// Where commanders 1 and 2 meet: their home systems and mother stations, the system next to
/// both, and in it the ends of the lanes from each home, a little inside the system.
struct Front {
    home1: (SystemId, Vec2),
    home2: (SystemId, Vec2),
    between: SystemId,
    gate1: Vec2,
    gate2: Vec2,
}

fn front(state: &UniverseState) -> Result<Front, String> {
    let home = |state: &UniverseState, n: usize| -> Option<(SystemId, Vec2)> {
        let (_, h) = state
            .holdings()
            .find(|(_, h)| h.owner == COMMANDERS[n] && h.mother_station)?;
        let site = state.galaxy().site(h.site)?;
        Some((site.system, site.position.into()))
    };
    let (home1, at1) = home(state, 0).ok_or("no mother station")?;
    let (home2, at2) = home(state, 1).ok_or("no mother station")?;
    let between = state
        .galaxy()
        .neighbours(home1)
        .intersection(&state.galaxy().neighbours(home2))
        .copied()
        .next()
        .ok_or("commanders 1 and 2 share no neighbour")?;
    // Where a lane from `towards` ends in `between`, a little inside the system.
    let gate_from = |state: &UniverseState, towards: SystemId| -> Option<Vec2> {
        let galaxy = state.galaxy();
        galaxy.system(between)?.gates.iter().find_map(|&gate| {
            let g = galaxy.gate(gate)?;
            g.lanes.iter().find_map(|&lane| {
                let far = galaxy.lane(lane)?.other_end(gate)?;
                (galaxy.gate(far)?.system == towards).then(|| {
                    let at = Vec2::from(g.position);
                    at - at.normalized().unwrap_or(Vec2::ZERO) * 6_000.0
                })
            })
        })
    };
    Ok(Front {
        home1: (home1, at1),
        home2: (home2, at2),
        between,
        gate1: gate_from(state, home1).ok_or("no gate towards home 1")?,
        gate2: gate_from(state, home2).ok_or("no gate towards home 2")?,
    })
}

/// The scripted fleets: commanders 1 and 2 in the standard system between their homes, with
/// a scout at home and a picket at the other's gate; raiders there and in the hub.
fn set_up(seed: u64, rules: &Rules) -> Result<(UniverseState, Vec<FleetId>), String> {
    let mut state =
        generate_stage1_with_rules(SeasonConfig::new(seed, 0), COMMANDERS, rules.clone())
            .map_err(|e| e.to_string())?;
    let Front {
        home1: (home1, at1),
        home2: (home2, at2),
        between,
        gate1,
        gate2,
    } = front(&state)?;
    let middle = (gate1 + gate2) * 0.5;
    let c = |n: usize| Owner::Commander(COMMANDERS[n]);
    let layout: [(Owner, SystemId, Vec2, &[ShipClass]); 8] = [
        (c(0), between, gate1, &[VX, VX]),
        (
            c(0),
            between,
            middle + Vec2::new(-9_000.0, 2_000.0),
            &[WARDEN, LANCER, LANCER],
        ),
        (
            c(0),
            between,
            middle + Vec2::new(-6_000.0, -8_000.0),
            &[LANCER],
        ),
        (c(0), home1, at1 + Vec2::new(12_000.0, 0.0), &[MULE]),
        (c(1), between, gate2, &[VX]),
        (
            c(1),
            between,
            middle + Vec2::new(8_000.0, 3_000.0),
            &[LANCER, LANCER, VX],
        ),
        (
            c(1),
            between,
            middle + Vec2::new(10_000.0, -5_000.0),
            &[WARDEN],
        ),
        (c(1), home2, at2 + Vec2::new(-12_000.0, 0.0), &[VX]),
    ];
    let mut fleets = Vec::new();
    for (owner, system, at, classes) in layout {
        fleets.push(place_fleet(&mut state, owner, system, at, 0.0, classes));
    }
    // Pickets that do not move, each watching the other's gate home (doc 16 §4).
    place_fleet(&mut state, c(0), between, gate2 * 1.04, 0.0, &[WARDEN]);
    place_fleet(&mut state, c(1), between, gate1 * 1.04, 0.0, &[WARDEN]);
    let hub = state
        .galaxy()
        .systems()
        .find(|(_, s)| s.kind == SystemKind::Hub)
        .map(|(id, _)| id)
        .ok_or("no hub")?;
    let npc = Owner::Npc(FactionId(1));
    let raiders = middle + Vec2::new(0.0, 14_000.0);
    place_fleet(
        &mut state,
        npc,
        between,
        raiders,
        0.0,
        &[RAIDER, RAIDER, RAIDER],
    );
    place_fleet(
        &mut state,
        npc,
        hub,
        Vec2::new(-20_000.0, 10_000.0),
        0.0,
        &[HSTL],
    );
    // Seeing, not fighting: every fleet holds fire, the docked start kits included.
    for fleet in state.fleet_ids() {
        set_rules_of_engagement(&mut state, fleet, RulesOfEngagement::HoldFire);
    }
    sense(&mut state);
    Ok((state, fleets))
}

/// A seeded order for `fleet`, from what its commander's view shows.
fn order_for(state: &UniverseState, rng: &mut RngStreams, fleet: FleetId) -> Option<Command> {
    let key = StreamKey::global(2);
    let view = state.fleet(fleet)?;
    let Place::InSystem(system) = view.place else {
        return None;
    };
    let commander = state.view(view.owner);
    // Near a site or a gate of the system: sites for meeting, gates for leaving.
    let anchors: Vec<Vec2> = state
        .galaxy()
        .system(system)
        .map(|s| {
            let sites = s
                .sites
                .iter()
                .filter_map(|&id| state.galaxy().site(id).map(|s| s.position.into()));
            let gates = s
                .gates
                .iter()
                .filter_map(|&id| state.galaxy().gate(id).map(|g| g.position.into()));
            sites.chain(gates).collect()
        })
        .unwrap_or_default();
    let point = |rng: &mut RngStreams| {
        let radius = 3_000.0 + rng.unit_f64(key) * 17_000.0;
        let angle = rng.unit_f64(key) * std::f64::consts::TAU;
        let base = anchors
            .get(rng.index(key, anchors.len()))
            .copied()
            .unwrap_or(Vec2::ZERO);
        Location {
            system,
            position: (base + Vec2::from_angle(angle) * radius).into(),
        }
    };
    // Where hostiles are, or were last seen, by what the commander knows.
    let leads: Vec<Vec2> = commander
        .sensed_fleets_in(system)
        .iter()
        .map(|f| Vec2::from(f.position))
        .chain(
            commander
                .last_known_fleets_in(system)
                .map(|f| Vec2::from(f.position)),
        )
        .collect();
    let hunt = |rng: &mut RngStreams| match leads.get(rng.index(key, leads.len().max(1))) {
        Some(&lead) => Location {
            system,
            position: (lead
                + Vec2::from_angle(rng.unit_f64(key) * std::f64::consts::TAU) * 2_000.0)
                .into(),
        },
        None => point(rng),
    };
    let order = match rng.below(key, 100) {
        0..20 => FleetOrder::Move { to: point(rng) },
        20..40 => FleetOrder::Move { to: hunt(rng) },
        40..48 => FleetOrder::Patrol {
            route: vec![point(rng), hunt(rng)],
        },
        48..62 => {
            let next: Vec<SystemId> = state.galaxy().neighbours(system).into_iter().collect();
            FleetOrder::Jump {
                system: *next.get(rng.index(key, next.len()))?,
            }
        }
        _ => {
            let targets: Vec<FleetId> = commander
                .sensed_fleets_in(system)
                .into_iter()
                .map(|f| f.id)
                .collect();
            match targets.get(rng.index(key, targets.len().max(1))) {
                Some(target) => FleetOrder::Attack {
                    target: target.sim_id(),
                },
                None => FleetOrder::Move { to: hunt(rng) },
            }
        }
    };
    Some(Command::Fleet {
        fleet,
        order,
        queue: Queueing::Replace,
    })
}

/// Runs the picture check and returns what it counted.
fn picture_check(args: &FogRun) -> Result<(Counts, u64), String> {
    let (mut state, fleets) = set_up(args.seed, &args.rules)?;
    let mut rng = RngStreams::new(args.seed ^ 0x0066_6f67); // "fog"
    let mut counts = Counts::default();
    let observers: BTreeSet<Owner> = COMMANDERS
        .iter()
        .map(|&c| Owner::Commander(c))
        .chain([Owner::Npc(FactionId(1))])
        .collect();
    for t in 0..args.ticks {
        let mut commanders = Vec::new();
        for (i, &fleet) in fleets.iter().enumerate() {
            let slot = i as u64 * ORDER_INTERVAL_TICKS / fleets.len() as u64;
            if !(t + slot).is_multiple_of(ORDER_INTERVAL_TICKS) {
                continue;
            }
            let Some(owner) = state.fleet(fleet).and_then(|f| f.owner.commander()) else {
                continue;
            };
            if let Some(command) = order_for(&state, &mut rng, fleet) {
                commanders.push(CommanderInput {
                    commander: owner,
                    client_seq: t * 16 + i as u64,
                    kind: CommanderInputKind::Command(command),
                });
            }
        }
        let out = step(
            &mut state,
            &TickInputs {
                operator: vec![],
                commanders,
            },
        );
        for result in &out.commands {
            match result.outcome {
                Ok(()) => counts.applied += 1,
                Err(reason) => *counts.rejected.entry(format!("{reason:?}")).or_default() += 1,
            }
        }
        for event in &out.visibility_events {
            match event {
                VisibilityEvent::FleetSighted { .. } | VisibilityEvent::HoldingSighted { .. } => {
                    counts.sighted += 1;
                }
                VisibilityEvent::FleetLost { .. } | VisibilityEvent::HoldingLost { .. } => {
                    counts.lost += 1;
                }
                VisibilityEvent::DepartureObserved { .. } => counts.departures += 1,
            }
        }
        counts.targets_lost += out
            .fleet_events
            .iter()
            .filter(|e| matches!(e, FleetEvent::TargetLost { .. }))
            .count() as u64;
        if out
            .combat_events
            .iter()
            .any(|e| matches!(e, CombatEvent::ShipDestroyed { .. }))
        {
            return Err("a ship was destroyed although every fleet holds fire".to_owned());
        }
        if nomad_sim::sensors::is_pass_tick(out.tick) {
            counts.passes += 1;
            check_pictures(&state, &observers, &mut counts);
            let timed = Instant::now();
            let stats = state.sensors_detection_stats();
            counts
                .detection_micros
                .push(timed.elapsed().as_secs_f64() * 1e6);
            counts.checks_per_pass.push(stats.checks);
        }
    }
    Ok((counts, args.ticks))
}

/// Mean and 99th percentile of `micros`.
fn mean_p99(micros: &mut [f64]) -> (f64, f64) {
    micros.sort_by(f64::total_cmp);
    let mean = micros.iter().sum::<f64>() / micros.len().max(1) as f64;
    let p99 = micros
        .get(micros.len().saturating_sub(1) * 99 / 100)
        .copied()
        .unwrap_or(0.0);
    (mean, p99)
}

/// What the stage-2 stress layout measured.
struct Stress {
    ships: usize,
    detection: (f64, f64),
    checks: u64,
    pass_ticks: (f64, f64),
    other_ticks: (f64, f64),
}

/// 600 ships of 10 owners attack-moving into one another in the hub (S7 plan §6 task 9).
fn stress(seed: u64, rules: &Rules) -> Result<Stress, String> {
    let mut state =
        generate_stage1_with_rules(SeasonConfig::new(seed, 0), COMMANDERS, rules.clone())
            .map_err(|e| e.to_string())?;
    let hub = state
        .galaxy()
        .systems()
        .find(|(_, s)| s.kind == SystemKind::Hub)
        .map(|(id, _)| id)
        .ok_or("no hub")?;
    let mut orders = Vec::new();
    let mut ships = 0;
    for n in 1..=10_u32 {
        let owner = Owner::Commander(CommanderId(n));
        let angle = f64::from(n) * std::f64::consts::TAU / 10.0;
        for f in 0..6_u32 {
            let centre = Vec2::from_angle(angle) * 12_000.0
                + Vec2::from_angle(angle + std::f64::consts::FRAC_PI_2)
                    * (f64::from(f) - 2.5)
                    * 1_500.0;
            let classes = [[VX; 4].as_slice(), &[LANCER; 4], &[WARDEN; 2]].concat();
            ships += classes.len();
            let fleet = place_fleet(
                &mut state,
                owner,
                hub,
                centre,
                angle + std::f64::consts::PI,
                &classes,
            );
            orders.push(CommanderInput {
                commander: CommanderId(n),
                client_seq: u64::from(f),
                kind: CommanderInputKind::Command(Command::Fleet {
                    fleet,
                    order: FleetOrder::AttackMove {
                        to: Location {
                            system: hub,
                            position: (Vec2::from_angle(angle) * -2_000.0).into(),
                        },
                    },
                    queue: Queueing::Replace,
                }),
            });
        }
    }
    sense(&mut state);
    let mut detection = Vec::new();
    let mut checks = 0;
    let (mut pass_ticks, mut other_ticks) = (Vec::new(), Vec::new());
    for t in 0..3_000_u64 {
        let inputs = TickInputs {
            operator: vec![],
            commanders: if t == 0 {
                std::mem::take(&mut orders)
            } else {
                vec![]
            },
        };
        let timed = Instant::now();
        let out = step(&mut state, &inputs);
        let micros = timed.elapsed().as_secs_f64() * 1e6;
        if out.commands.iter().any(|c| c.outcome.is_err()) {
            return Err(format!("a stress order was rejected: {:?}", out.commands));
        }
        if nomad_sim::sensors::is_pass_tick(out.tick) {
            pass_ticks.push(micros);
            let timed = Instant::now();
            let stats = state.sensors_detection_stats();
            detection.push(timed.elapsed().as_secs_f64() * 1e6);
            checks = checks.max(stats.checks);
        } else {
            other_ticks.push(micros);
        }
    }
    Ok(Stress {
        ships,
        detection: mean_p99(&mut detection),
        checks,
        pass_ticks: mean_p99(&mut pass_ticks),
        other_ticks: mean_p99(&mut other_ticks),
    })
}

/// What finding each other measured, in seconds from the orders.
struct Approach {
    /// The first sighting of either fleet.
    sighting: Option<f64>,
    /// The first hit on either.
    hit: Option<f64>,
    /// When one fleet was gone.
    end: Option<f64>,
}

/// Two fleets 40 km apart in open space attack-move towards each other.
fn approach(
    seed: u64,
    rules: &Rules,
    a: &[ShipClass],
    b: &[ShipClass],
) -> Result<Approach, String> {
    let mut state =
        generate_stage1_with_rules(SeasonConfig::new(seed, 0), COMMANDERS, rules.clone())
            .map_err(|e| e.to_string())?;
    let hub = state
        .galaxy()
        .systems()
        .find(|(_, s)| s.kind == SystemKind::Hub)
        .map(|(id, _)| id)
        .ok_or("no hub")?;
    let centre: Vec2 = state
        .galaxy()
        .system(hub)
        .and_then(|s| s.sites.first())
        .and_then(|&site| state.galaxy().site(site))
        .map(|s| s.position.into())
        .ok_or("the hub has no site")?;
    let (west, east) = (
        centre - Vec2::new(20_000.0, 0.0),
        centre + Vec2::new(20_000.0, 0.0),
    );
    let owner = |n| Owner::Commander(CommanderId(n));
    let fa = place_fleet(&mut state, owner(1), hub, west, 0.0, a);
    let fb = place_fleet(&mut state, owner(2), hub, east, std::f64::consts::PI, b);
    sense(&mut state);
    let go = |n: u32, fleet, to: Vec2| CommanderInput {
        commander: CommanderId(n),
        client_seq: 1,
        kind: CommanderInputKind::Command(Command::Fleet {
            fleet,
            order: FleetOrder::AttackMove {
                to: Location {
                    system: hub,
                    position: to.into(),
                },
            },
            queue: Queueing::Replace,
        }),
    };
    let mut first = TickInputs {
        operator: vec![],
        commanders: vec![go(1, fa, east), go(2, fb, west)],
    };
    let secs = |t: u64| t as f64 / TICKS_PER_SECOND as f64;
    let mut result = Approach {
        sighting: None,
        hit: None,
        end: None,
    };
    for t in 1..=12_000_u64 {
        let out = step(&mut state, &std::mem::take(&mut first));
        if out.commands.iter().any(|c| c.outcome.is_err()) {
            return Err(format!("an attack-move was rejected: {:?}", out.commands));
        }
        if result.sighting.is_none()
            && out.visibility_events.iter().any(|e| {
                matches!(e, VisibilityEvent::FleetSighted { fleet, .. } if *fleet == fa || *fleet == fb)
            })
        {
            result.sighting = Some(secs(t));
        }
        if result.hit.is_none()
            && out
                .combat_events
                .iter()
                .any(|e| matches!(e, CombatEvent::ShipDamaged { .. }))
        {
            result.hit = Some(secs(t));
        }
        if state.fleet(fa).is_none() || state.fleet(fb).is_none() {
            result.end = Some(secs(t));
            break;
        }
    }
    Ok(result)
}

/// Runs `--fog` and writes the report.
pub fn run(out: &mut impl Write, args: &FogRun) -> Result<(), String> {
    let w = |out: &mut dyn Write, line: String| writeln!(out, "{line}").map_err(|e| e.to_string());
    let started = Instant::now();
    let (mut counts, ticks) = picture_check(args)?;
    let elapsed = started.elapsed().as_secs_f64();
    w(
        out,
        format!(
            "fog | seed {} | {:.1} simulated hours in {elapsed:.1} s | {} passes checked, {} \
             picture comparisons, {} mismatches",
            args.seed,
            ticks as f64 / TICKS_PER_SECOND as f64 / 3_600.0,
            counts.passes,
            counts.comparisons,
            counts.mismatches.len(),
        ),
    )?;
    w(
        out,
        format!(
            "  {} sightings, {} losses, {} departures seen, {} Attacks ended at a lost target \
             (D-180) | most fleets remembered {}, most departures kept {} | commands {} \
             applied, rejected {:?}",
            counts.sighted,
            counts.lost,
            counts.departures,
            counts.targets_lost,
            counts.most_memory,
            counts.most_traffic,
            counts.applied,
            counts.rejected,
        ),
    )?;
    for mismatch in counts.mismatches.iter().take(10) {
        w(out, format!("  MISMATCH {mismatch}"))?;
    }
    let (mean, p99) = mean_p99(&mut counts.detection_micros);
    let checks = counts.checks_per_pass.iter().copied().max().unwrap_or(0);
    w(
        out,
        format!(
            "sensors cost, stage 1 (this run) | detection pass mean {mean:.1} µs, p99 {p99:.1} \
             µs | at most {checks} distance checks a pass"
        ),
    )?;
    let s = stress(args.seed, &args.rules)?;
    w(
        out,
        format!(
            "sensors cost, stage 2 stress ({} ships of 10 owners in one system, fighting) | \
             detection pass mean {:.1} µs, p99 {:.1} µs, at most {} checks | ticks with a pass \
             mean {:.0} µs, p99 {:.0} µs; other ticks mean {:.0} µs, p99 {:.0} µs",
            s.ships,
            s.detection.0,
            s.detection.1,
            s.checks,
            s.pass_ticks.0,
            s.pass_ticks.1,
            s.other_ticks.0,
            s.other_ticks.1,
        ),
    )?;
    for (name, a, b) in [
        ("6 VX v 6 VX", [VX; 6].to_vec(), [VX; 6].to_vec()),
        (
            "6 LANCER v 6 WARDEN",
            [LANCER; 6].to_vec(),
            [WARDEN; 6].to_vec(),
        ),
    ] {
        let r = approach(args.seed, &args.rules, &a, &b)?;
        let show = |s: Option<f64>| s.map_or_else(|| "never".to_owned(), |s| format!("{s:.1} s"));
        w(
            out,
            format!(
                "finding each other, {name:<20} 40 km apart, attack-moving | first sighting \
                 {} | first hit {} | one side gone {}",
                show(r.sighting),
                show(r.hit),
                show(r.end),
            ),
        )?;
    }
    if counts.mismatches.is_empty() {
        Ok(())
    } else {
        Err(format!("{} picture mismatches", counts.mismatches.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ten_simulated_minutes_match_ground_truth() {
        let args = FogRun {
            seed: 3,
            ticks: 6_000,
            rules: Rules::embedded(),
        };
        let (counts, _) = picture_check(&args).unwrap();
        assert!(counts.mismatches.is_empty(), "{:?}", counts.mismatches);
        assert_eq!(counts.passes, 1_200);
        assert!(
            counts.sighted > 0 && counts.applied > 0,
            "something happens"
        );
    }

    #[test]
    fn the_brute_force_truth_sees_what_the_rules_say() {
        let (state, _) = set_up(3, &Rules::embedded()).unwrap();
        let truth = ground_truth(&state);
        // Nobody detects its own ships or holdings: they are known in full anyway.
        for (observer, systems) in &truth {
            for (ships, holdings) in systems.values() {
                for ship in ships {
                    assert_ne!(state.ship(*ship).map(|s| s.owner), Some(*observer));
                }
                for holding in holdings {
                    assert_ne!(
                        state.holding(*holding).map(|h| Owner::Commander(h.owner)),
                        Some(*observer)
                    );
                }
            }
        }
    }
}
