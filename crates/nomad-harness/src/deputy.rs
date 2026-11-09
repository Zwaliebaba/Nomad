//! `--deputy`: the Deputy's scenarios (A1 plan §9) and its budget (D-203, A1 plan §4.5).
//!
//! Each scenario runs on the stage-1 sector from scripted inputs, with commander 1 (C1) as
//! the defender, and checks what §9 asks; the report lists each check and the timeline.
//!
//! 1. **Raid:** four VX of C2 attack-move on C1's mother station from 30 km. Alert within one
//!    sensors pass, one Deputy slot and the latency of the zone crossing; the alert profile;
//!    the garrison's launch; priority targeting; no pursuit beyond the leash (plus a weapon's
//!    range, where the fleet stands to fire); retreat below the threshold, if it comes to
//!    that; docking after the alert ends.
//! 2. **Abilities:** D-058's mid-size fleet on the east sector: SHLD SURGE fires below the
//!    Balanced 30%; eight light hostiles within 3 km bring FLAK SCREEN, seven do not.
//! 3. **Convoys:** the start kit's MULEs run a convoy to a second holding; hostiles near the
//!    stop evacuate it and it resumes 60 s after they left; a contested gate on its way to a
//!    holding in another system reroutes or holds it, and it never spools up there.
//! 4. **Rebuilding:** "keep the garrison at 3 VX": losses are queued within the 24 h budget
//!    and deferred beyond it; a solar array destroyed inside C1's exposure window (D-169) is
//!    rebuilt in its slot.
//! 5. **Direct control** (RK-10): a fleet under a direct order is never overridden; it
//!    retreats below the threshold only with the opt-in; `Release` and the D-162 lapse return
//!    it to its role.
//! 6. **Catch-up:** during a catch-up (everyone offline, D-047, D-080) the Deputy defends.
//!
//! `--deputy bench` measures the Deputy's cost (D-203): stage 1 in catch-up, with a raid on a
//! mother station every 30 simulated minutes, timing every commander's Deputy evaluation
//! directly on each tick's state (less the copying of its memory) and the whole tick, and
//! reporting mean and p99, the share of a 500× tick (0.2 ms), and the catch-up speed. Build
//! with `--release` for the D-203 numbers.

use std::io::Write;
use std::time::Instant;

use nomad_sim::galaxy::{STAGE1_COMMANDERS, generate_stage1_with_rules};
use nomad_sim::scenario::{place_fleet, place_holding, sense};
use nomad_sim::{
    Command, CommandOutcome, CommanderId, CommanderInput, CommanderInputKind, DefenceEvent,
    DeferReason, DeputyEvent, DoctrineCommand, EconomyCommand, FleetEvent, FleetId, FleetOrder,
    HoldingId, HoldingOrder, InboxEvent, ModuleKind, OperatorInput, Owner, Place, PowerProfile,
    Queueing, Rebuild, ReplacementEntry, Rules, SeasonConfig, ShipClass, Slot, StationAbility,
    SystemId, TICKS_PER_SECOND, Tick, TickInputs, TickOutputs, Ticks, UniverseState, Vec2, step,
};

const COMMANDERS: [CommanderId; STAGE1_COMMANDERS] = [
    CommanderId(1),
    CommanderId(2),
    CommanderId(3),
    CommanderId(4),
];
const C1: CommanderId = CommanderId(1);
const C2: CommanderId = CommanderId(2);
const SECOND: u64 = TICKS_PER_SECOND;

/// A `--deputy` run.
pub struct DeputyRun {
    /// The season seed.
    pub seed: u64,
    /// The rules (D-104).
    pub rules: Rules,
}

/// One scenario's Universe, with C1's mother station and its two start-kit fleets (D-286).
struct World {
    state: UniverseState,
    mother: HoldingId,
    system: SystemId,
    centre: Vec2,
    garrison: FleetId,
    reserve: FleetId,
    seq: u64,
    pending: Vec<CommanderInput>,
    operator: Vec<OperatorInput>,
    /// The timeline, for the report.
    log: Vec<(Tick, String)>,
    /// C1's AI commands that applied, by tick, from its inbox (D-202), kept past the inbox's
    /// 60 s.
    outcomes: Vec<(Tick, Command)>,
}

impl World {
    fn new(seed: u64, rules: &Rules) -> Result<Self, String> {
        let state =
            generate_stage1_with_rules(SeasonConfig::new(seed, 0), COMMANDERS, rules.clone())
                .map_err(|e| e.to_string())?;
        let (mother, site) = state
            .holdings()
            .find(|(_, h)| h.owner == C1 && h.mother_station)
            .map(|(id, h)| (id, h.site))
            .ok_or("no mother station")?;
        let site = state.galaxy().site(site).ok_or("no site")?;
        let (system, centre) = (site.system, Vec2::from(site.position));
        let kit = |class: ShipClass| {
            state.fleet_ids().into_iter().find(|&f| {
                state.fleet(f).is_some_and(|v| {
                    v.owner == Owner::Commander(C1)
                        && v.ships
                            .iter()
                            .all(|&s| state.ship(s).is_some_and(|s| s.class == class))
                })
            })
        };
        let garrison = kit(ShipClass::Vx).ok_or("no garrison")?;
        let reserve = kit(ShipClass::Mule).ok_or("no reserve")?;
        Ok(Self {
            state,
            mother,
            system,
            centre,
            garrison,
            reserve,
            seq: 0,
            pending: Vec::new(),
            operator: Vec::new(),
            log: Vec::new(),
            outcomes: Vec::new(),
        })
    }

    /// Queues `command` from commander `by` for the next tick.
    fn send(&mut self, by: CommanderId, command: Command) {
        self.seq += 1;
        self.pending.push(CommanderInput {
            commander: by,
            client_seq: self.seq,
            kind: CommanderInputKind::Command(command),
        });
    }

    fn order(&mut self, by: CommanderId, fleet: FleetId, order: FleetOrder, retreat: bool) {
        self.send(
            by,
            Command::Fleet {
                fleet,
                order,
                queue: Queueing::Replace,
                retreat,
            },
        );
    }

    /// One tick with the queued inputs; logs the Deputy's events.
    fn tick(&mut self) -> TickOutputs {
        let inputs = TickInputs {
            operator: std::mem::take(&mut self.operator),
            commanders: std::mem::take(&mut self.pending),
        };
        let out = step(&mut self.state, &inputs);
        for (who, event) in &out.deputy_events {
            self.log.push((out.tick, format!("C{} {event:?}", who.0)));
        }
        for entry in self.state.inbox(Owner::Commander(C1)) {
            if let InboxEvent::Outcome {
                command,
                outcome: CommandOutcome::Applied,
                ..
            } = &entry.event
                && entry.tick == out.tick
            {
                self.outcomes.push((entry.tick, command.clone()));
            }
        }
        out
    }

    /// Runs `ticks` ticks, returning every output.
    fn run(&mut self, ticks: u64) -> Vec<TickOutputs> {
        (0..ticks).map(|_| self.tick()).collect()
    }

    /// Places a C2 fleet `distance_m` from the mother station's centre, `angle_deg` from east,
    /// detected at once.
    fn hostiles(&mut self, classes: &[ShipClass], distance_m: f64, angle_deg: f64) -> FleetId {
        let direction = Vec2::from_angle(angle_deg.to_radians());
        let fleet = place_fleet(
            &mut self.state,
            Owner::Commander(C2),
            self.system,
            self.centre + direction * distance_m,
            (direction * -1.0).angle(),
            classes,
        );
        sense(&mut self.state);
        fleet
    }

    fn distance_m(&self, fleet: FleetId) -> Option<f64> {
        let view = self.state.fleet(fleet)?;
        matches!(view.place, Place::InSystem(_))
            .then(|| (Vec2::from(view.position) - self.centre).length())
    }

    /// The first tick an applied Deputy command of C1 matched `want`, by its inbox (D-202).
    fn applied(&self, since: Tick, want: impl Fn(&Command) -> bool) -> Option<Tick> {
        self.outcomes
            .iter()
            .find(|(tick, command)| *tick >= since && want(command))
            .map(|(tick, _)| *tick)
    }

    fn at(&self, offset: Vec2) -> nomad_sim::Location {
        nomad_sim::Location {
            system: self.system,
            position: (self.centre + offset).into(),
        }
    }
}

/// The first tick one of C1's Deputy events matched `want`.
fn deputy_event(outs: &[TickOutputs], want: impl Fn(&DeputyEvent) -> bool) -> Option<Tick> {
    outs.iter().find_map(|o| {
        o.deputy_events
            .iter()
            .any(|(who, e)| *who == C1 && want(e))
            .then_some(o.tick)
    })
}

fn check(ok: bool, what: String, checks: &mut Vec<(bool, String)>) {
    checks.push((ok, what));
}

fn secs(ticks: u64) -> f64 {
    ticks as f64 / TICKS_PER_SECOND as f64
}

type Checks = Vec<(bool, String)>;

/// Scenario 1: a raid on the mother station.
#[expect(
    clippy::too_many_lines,
    reason = "one scenario script with its checks, read top to bottom"
)]
fn raid(seed: u64, rules: &Rules, checks: &mut Checks) -> Result<World, String> {
    let mut w = World::new(seed, rules)?;
    let raiders = w.hostiles(&[ShipClass::Vx; 4], 30_000.0, 0.0);
    // Towards the station, stopping 3 km out of its centre (the dome is an obstacle).
    let target = w.at(Vec2::new(3_000.0, 0.0));
    w.order(C2, raiders, FleetOrder::AttackMove { to: target }, false);
    let zone = f64::from(
        rules
            .ai
            .preset(nomad_sim::DoctrinePreset::Balanced)
            .holding
            .zone_radius_m,
    );
    let leash = f64::from(rules.ai.preset(nomad_sim::DoctrinePreset::Balanced).leash_m);
    let range = rules
        .combat
        .class(ShipClass::Vx)
        .longest_range_m()
        .unwrap_or(0.0);
    let (mut crossed, mut farthest, mut attacked, mut retreated) = (None, 0.0_f64, false, false);
    let mut outs = Vec::new();
    for t in 0..15 * 60 * SECOND {
        // Three minutes in, C2 calls its raiders home.
        if t == 180 * SECOND && w.state.fleet(raiders).is_some() {
            w.order(C2, raiders, FleetOrder::Retreat { to: None }, false);
        }
        let out = w.tick();
        let raider_ships = w.state.fleet(raiders).map(|f| f.ships).unwrap_or_default();
        let in_zone = raider_ships.iter().any(|&s| {
            w.state
                .ship(s)
                .is_some_and(|v| (Vec2::from(v.position) - w.centre).length() <= zone)
        });
        if in_zone && crossed.is_none() {
            crossed = Some(out.tick);
        }
        if let Some(d) = w.distance_m(w.garrison) {
            farthest = farthest.max(d);
        }
        if let Some(view) = w.state.fleet(w.garrison) {
            attacked |= matches!(view.order, Some(FleetOrder::Attack { target })
                if raider_ships.iter().any(|s| s.sim_id() == target));
            retreated |= matches!(view.order, Some(FleetOrder::Retreat { .. }));
        }
        outs.push(out);
        let ended = deputy_event(&outs, |e| matches!(e, DeputyEvent::AlertEnded { .. }));
        let docked = ended.is_some_and(|end| {
            outs.iter().any(|o| {
                o.tick >= end
                    && o.fleet_events.iter().any(|e| {
                        matches!(e, FleetEvent::Docked { fleet, holding } if *fleet == w.garrison && *holding == w.mother)
                    })
            })
        });
        if docked || (ended.is_some() && w.state.fleet(w.garrison).is_none()) {
            break;
        }
    }
    let started = deputy_event(
        &outs,
        |e| matches!(e, DeputyEvent::AlertStarted { holding } if *holding == w.mother),
    );
    let latency = 3; // a human's Deputy (D-201)
    let bound = 5 + rules.ai.deputy_interval_ticks + latency;
    match (crossed, started) {
        (Some(c), Some(s)) => check(
            s.get() <= c.get() + bound,
            format!(
                "alert {:.1} s after the zone crossing (≤ {:.1} s: a sensors pass, a Deputy slot, the latency)",
                secs(s.get().saturating_sub(c.get())),
                secs(bound)
            ),
            checks,
        ),
        _ => check(
            false,
            format!("zone crossing {crossed:?}, alert {started:?}"),
            checks,
        ),
    }
    let since = started.unwrap_or(Tick::ZERO);
    let profile = w.applied(since, |c| {
        matches!(
            c,
            Command::Economy(EconomyCommand::SetPowerProfile {
                profile: PowerProfile::ShieldsFirst,
                ..
            })
        )
    });
    check(
        profile.is_some(),
        format!("the alert power profile (ShieldsFirst) at {profile:?}"),
        checks,
    );
    let launched = outs.iter().find_map(|o| {
        o.fleet_events
            .iter()
            .any(|e| matches!(e, FleetEvent::Launched { fleet, .. } if *fleet == w.garrison))
            .then_some(o.tick)
    });
    check(
        launched.is_some(),
        format!("the garrison launched at {launched:?}"),
        checks,
    );
    let reserve_docked = outs.iter().all(|o| {
        !o.fleet_events
            .iter()
            .any(|e| matches!(e, FleetEvent::Launched { fleet, .. } if *fleet == w.reserve))
    });
    check(
        reserve_docked,
        "the MULE reserve stayed docked".to_owned(),
        checks,
    );
    check(
        attacked,
        "the garrison attacked a raider by explicit order".to_owned(),
        checks,
    );
    check(
        farthest <= leash + range + 2_000.0,
        format!(
            "the garrison went at most {:.1} km out (leash {:.0} km + weapon range {:.1} km, 2 km for latency)",
            farthest / 1_000.0,
            leash / 1_000.0,
            range / 1_000.0
        ),
        checks,
    );
    let ended = deputy_event(&outs, |e| matches!(e, DeputyEvent::AlertEnded { .. }));
    check(
        ended.is_some(),
        format!("the alert ended at {ended:?}"),
        checks,
    );
    let docked = w
        .state
        .fleet(w.garrison)
        .is_none_or(|f| f.place == Place::Docked(w.mother));
    check(
        docked,
        "the garrison docked again after the alert (or was lost)".to_owned(),
        checks,
    );
    w.log
        .push((Tick::ZERO, format!("retreat ordered: {retreated}")));
    Ok(w)
}

/// Scenario 2: SHLD SURGE and FLAK SCREEN on their triggers.
fn abilities(seed: u64, rules: &Rules, checks: &mut Checks) -> Result<World, String> {
    let mut w = World::new(seed, rules)?;
    // The garrison stays docked: this is about the station's abilities.
    let preset = *rules.ai.preset(nomad_sim::DoctrinePreset::Balanced);
    for fleet in [w.garrison, w.reserve] {
        let holding = w.mother;
        w.send(
            C1,
            Command::Doctrine(DoctrineCommand::SetFleet {
                fleet,
                doctrine: Some(nomad_sim::FleetDoctrine {
                    role: Some(nomad_sim::FleetRole::Reserve { holding }),
                    ..preset.fleet()
                }),
            }),
        );
    }
    let attackers = [vec![ShipClass::Lancer; 10], vec![ShipClass::Warden; 4]].concat();
    let fleet = w.hostiles(&attackers, 6_000.0, 0.0);
    let holding = w.mother;
    w.order(
        C2,
        fleet,
        FleetOrder::Attack {
            target: holding.sim_id(),
        },
        false,
    );
    let east = 1_usize;
    let mut share_before = 1_000_u64;
    let mut surged = None;
    for _ in 0..20 * 60 * SECOND {
        let out = w.tick();
        let now = out.tick;
        if w.applied(now, |c| {
            matches!(
                c,
                Command::Holding {
                    order: HoldingOrder::Ability {
                        ability: StationAbility::ShieldSurge { .. }
                    },
                    ..
                }
            )
        })
        .is_some()
        {
            surged = Some((now, share_before));
            break;
        }
        if out
            .defence_events
            .iter()
            .any(|e| matches!(e, DefenceEvent::SectorBreached { .. }))
        {
            break;
        }
        if let Some(d) = w.state.defence(w.mother) {
            let s = d.sectors[east];
            share_before = s.strength_milli * 1_000 / s.max_milli.max(1);
        }
    }
    let threshold = u64::from(preset.holding.surge_below_permille);
    match surged {
        Some((t, share)) => check(
            share < threshold && share + 30 >= threshold,
            format!(
                "SHLD SURGE at {:.1}% of the sector ({}% trigger), at {t:?}",
                share as f64 / 10.0,
                threshold / 10
            ),
            checks,
        ),
        None => check(false, "SHLD SURGE never fired".to_owned(), checks),
    }
    // FLAK SCREEN: eight light hostiles within 3 km of the dome centre, then seven.
    for (count, expect) in [(8_usize, true), (7, false)] {
        let mut f = World::new(seed, rules)?;
        f.hostiles(&vec![ShipClass::Vx; count], 2_500.0, 90.0);
        let outs = f.run(15 * SECOND);
        let _ = outs;
        let flak = f.applied(Tick::ZERO, |c| {
            matches!(
                c,
                Command::Holding {
                    order: HoldingOrder::Ability {
                        ability: StationAbility::FlakScreen
                    },
                    ..
                }
            )
        });
        check(
            flak.is_some() == expect,
            format!(
                "{count} light hostiles within 3 km: FLAK SCREEN {}",
                if flak.is_some() { "fired" } else { "held" }
            ),
            checks,
        );
    }
    Ok(w)
}

/// A free site of `system`.
fn free_site(state: &UniverseState, system: SystemId) -> Option<nomad_sim::galaxy::SiteId> {
    state
        .galaxy()
        .system(system)?
        .sites
        .iter()
        .copied()
        .find(|&s| state.holding_at(s).is_none())
}

/// Scenario 3: convoy evacuation and resumption; a contested gate.
fn convoys(seed: u64, rules: &Rules, checks: &mut Checks) -> Result<World, String> {
    let mut w = World::new(seed, rules)?;
    let site = free_site(&w.state, w.system).ok_or("no free site")?;
    let remote = place_holding(&mut w.state, C1, site).ok_or("no remote holding")?;
    let remote_at = Vec2::from(w.state.galaxy().site(site).ok_or("no site")?.position);
    let (reserve, mother) = (w.reserve, w.mother);
    w.send(
        C1,
        Command::Economy(EconomyCommand::Convoy {
            fleet: reserve,
            route: vec![remote, mother],
        }),
    );
    w.run(30 * SECOND);
    // Hostiles 15 km from the remote stop: inside Balanced's 20 km evacuation radius.
    let offset = (remote_at - w.centre)
        .normalized()
        .unwrap_or(Vec2::new(1.0, 0.0));
    let hostiles = place_fleet(
        &mut w.state,
        Owner::Commander(C2),
        w.system,
        remote_at + offset * 15_000.0,
        0.0,
        &[ShipClass::Vx; 2],
    );
    sense(&mut w.state);
    let mut outs = w.run(30 * SECOND);
    let evacuated = deputy_event(
        &outs,
        |e| matches!(e, DeputyEvent::ConvoyEvacuated { fleet, .. } if *fleet == reserve),
    );
    check(
        evacuated.is_some(),
        format!("the convoy evacuated at {evacuated:?}"),
        checks,
    );
    // They leave: C2 takes them home.
    w.order(C2, hostiles, FleetOrder::Retreat { to: None }, false);
    let radius = f64::from(
        rules
            .ai
            .preset(nomad_sim::DoctrinePreset::Balanced)
            .holding
            .evacuation_radius_m,
    );
    let mother_at = w.centre;
    // The last tick C1 sensed a hostile ship within the radius of the docked convoy or its
    // stop (D-210 counts sensed hostiles; one that leaves C1's detection no longer counts).
    let mut last_near = None;
    let mut resumed = None;
    for _ in 0..10 * 60 * SECOND {
        let out = w.tick();
        let ships = w.state.fleet(hostiles).map(|f| f.ships).unwrap_or_default();
        let view = w.state.view(Owner::Commander(C1));
        let near = ships.iter().any(|&s| {
            view.sensed_ship(s).is_some_and(|v| {
                [mother_at, remote_at]
                    .iter()
                    .any(|&p| (Vec2::from(v.position) - p).length() <= radius)
            })
        });
        if near {
            last_near = Some(out.tick);
        }
        outs.push(out);
        if resumed.is_none() {
            resumed = deputy_event(
                &outs,
                |e| matches!(e, DeputyEvent::ConvoyResumed { fleet } if *fleet == reserve),
            );
        }
        // Until its route has been given again.
        if resumed.is_some_and(|r| w.state.tick().get() > r.get() + 2 * SECOND) {
            break;
        }
    }
    match (last_near, resumed) {
        (Some(g), Some(r)) => check(
            r.get() + 2 * SECOND >= g.get() + 60 * SECOND && r.get() <= g.get() + 62 * SECOND,
            format!(
                "it resumed {:.1} s after the last hostile within 20 km (60 s, D-210; a sensors pass and a slot of slack)",
                secs(r.get().saturating_sub(g.get()))
            ),
            checks,
        ),
        _ => check(
            false,
            format!("last hostile near {last_near:?}, resumed {resumed:?}"),
            checks,
        ),
    }
    let again = w.applied(Tick::ZERO, |c| {
        matches!(c, Command::Economy(EconomyCommand::Convoy { fleet, route }) if *fleet == reserve && !route.is_empty())
    });
    check(
        again.is_some(),
        format!("its route was given again at {again:?}"),
        checks,
    );
    Ok(w)
}

/// Scenario 3b: a contested gate on the way to a holding in another system.
fn contested_gate(seed: u64, rules: &Rules, checks: &mut Checks) -> Result<World, String> {
    let mut w = World::new(seed, rules)?;
    let (site, system) = w
        .state
        .galaxy()
        .sites()
        .find(|(id, s)| {
            s.system != w.system
                && w.state.holding_at(*id).is_none()
                && w.state.galaxy().neighbours(w.system).contains(&s.system)
        })
        .map(|(id, s)| (id, s.system))
        .ok_or("no neighbouring free site")?;
    let remote = place_holding(&mut w.state, C1, site).ok_or("no remote holding")?;
    // The gate of the home system towards it.
    let gate = w
        .state
        .galaxy()
        .system(w.system)
        .ok_or("no system")?
        .gates
        .iter()
        .copied()
        .find(|&g| {
            let galaxy = w.state.galaxy();
            galaxy.gate(g).is_some_and(|gate| {
                gate.lanes.iter().any(|&l| {
                    galaxy
                        .lane(l)
                        .and_then(|lane| lane.other_end(g))
                        .and_then(|other| galaxy.gate(other))
                        .is_some_and(|t| t.system == system)
                })
            })
        });
    let Some(gate) = gate else {
        check(
            true,
            "no direct gate to the neighbour on this seed: skipped".to_owned(),
            checks,
        );
        return Ok(w);
    };
    let gate_at = Vec2::from(w.state.galaxy().gate(gate).ok_or("no gate")?.position);
    place_fleet(
        &mut w.state,
        Owner::Commander(C2),
        w.system,
        gate_at + Vec2::new(1_000.0, 0.0),
        0.0,
        &[ShipClass::Vx; 2],
    );
    // A C1 picket detects them (D-285).
    place_fleet(
        &mut w.state,
        Owner::Commander(C1),
        w.system,
        gate_at + Vec2::new(-1_500.0, 0.0),
        0.0,
        &[ShipClass::Vx],
    );
    sense(&mut w.state);
    let (reserve, mother) = (w.reserve, w.mother);
    w.send(
        C1,
        Command::Economy(EconomyCommand::Convoy {
            fleet: reserve,
            route: vec![remote, mother],
        }),
    );
    let outs = w.run(5 * 60 * SECOND);
    let held_or_rerouted = deputy_event(
        &outs,
        |e| matches!(e, DeputyEvent::ConvoyHeld { fleet, gate: g } | DeputyEvent::ConvoyRerouted { fleet, gate: g } if *fleet == reserve && *g == gate),
    );
    let rerouted =
        deputy_event(&outs, |e| matches!(e, DeputyEvent::ConvoyRerouted { .. })).is_some();
    check(
        held_or_rerouted.is_some(),
        format!(
            "the convoy {} at the contested gate",
            if rerouted { "rerouted" } else { "held" }
        ),
        checks,
    );
    let through = outs.iter().flat_map(|o| &o.fleet_events).any(|e| {
        matches!(e, FleetEvent::SpoolUpStarted { fleet, gate: g, .. } if *fleet == reserve && *g == gate)
    });
    check(
        !through,
        "it never spooled up at the contested gate".to_owned(),
        checks,
    );
    Ok(w)
}

/// Scenario 4: replacements within the budget, a deferral, a module rebuilt.
#[expect(
    clippy::too_many_lines,
    reason = "one scenario script with its checks, read top to bottom"
)]
fn rebuilding(seed: u64, rules: &Rules, checks: &mut Checks) -> Result<World, String> {
    let mut w = World::new(seed, rules)?;
    let garrison = w.garrison;
    w.send(
        C1,
        Command::Doctrine(DoctrineCommand::SetReplacement {
            entries: vec![ReplacementEntry {
                fleet: garrison,
                class: ShipClass::Vx,
                count: 3,
                role: None,
            }],
        }),
    );
    // Up to C1's exposure window, so peripheral modules can be destroyed (D-169).
    let window = w.state.exposure_window(C1).ok_or("no window")?;
    let opens = window.opens.get().max(Ticks::from_hours(2).get());
    w.run(opens - w.state.tick().get() + 30 * SECOND);
    let budget_from = w.state.tick();
    let solar = w
        .state
        .modules_at(w.mother)
        .into_iter()
        .find(|m| m.kind == ModuleKind::Solar)
        .ok_or("no solar array")?;
    let raiders = w.hostiles(
        &[vec![ShipClass::Vx; 10], vec![ShipClass::Lancer; 6]].concat(),
        8_000.0,
        0.0,
    );
    w.order(C2, raiders, FleetOrder::Attack { target: solar.id }, false);
    let mut outs = Vec::new();
    for _ in 0..15 * 60 * SECOND {
        outs.push(w.tick());
        // Once the array is gone, the raiders turn on the garrison.
        if w.state.module(solar.id).is_none()
            && let Some(f) = w.state.fleet(raiders)
            && !matches!(f.order, Some(FleetOrder::Attack { target }) if target == garrison.sim_id())
            && w.state
                .fleet(garrison)
                .is_some_and(|g| matches!(g.place, Place::InSystem(_)))
        {
            w.order(
                C2,
                raiders,
                FleetOrder::Attack {
                    target: garrison.sim_id(),
                },
                false,
            );
        }
    }
    let built = w.applied(budget_from, |c| {
        matches!(
            c,
            Command::Economy(EconomyCommand::Build {
                item: nomad_sim::BuildItem::Ship {
                    class: ShipClass::Vx
                },
                ..
            })
        )
    });
    let losses = outs
        .iter()
        .flat_map(|o| &o.combat_events)
        .filter(|e| matches!(e, nomad_sim::CombatEvent::ShipDestroyed { fleet, .. } if *fleet == garrison))
        .count();
    check(losses > 0, format!("the garrison lost {losses} VX"), checks);
    check(
        built.is_some(),
        format!("a lost VX was queued again within the budget at {built:?}"),
        checks,
    );
    let deferred = deputy_event(&outs, |e| {
        matches!(
            e,
            DeputyEvent::Deferred {
                reason: DeferReason::BudgetExhausted,
                ..
            }
        )
    });
    w.log.push((
        Tick::ZERO,
        format!("deferral over the budget: {deferred:?}"),
    ));
    let solar_again = w.applied(budget_from, |c| {
        matches!(
            c,
            Command::Economy(EconomyCommand::Build {
                item: nomad_sim::BuildItem::Module {
                    kind: ModuleKind::Solar,
                    slot: Slot::Solar
                },
                ..
            })
        )
    });
    let solar_deferred = deputy_event(&outs, |e| {
        matches!(
            e,
            DeputyEvent::Deferred {
                what: Rebuild::Module {
                    kind: ModuleKind::Solar,
                    ..
                },
                ..
            }
        )
    });
    check(
        w.state.module(solar.id).is_none() && (solar_again.is_some() || solar_deferred.is_some()),
        format!(
            "the destroyed solar array was queued again in its slot ({solar_again:?}) or deferred ({solar_deferred:?})"
        ),
        checks,
    );
    check(
        losses <= 1 || deferred.is_some(),
        "beyond the budget, a replacement is deferred and reported".to_owned(),
        checks,
    );
    Ok(w)
}

/// Scenario 5: direct control.
#[expect(
    clippy::too_many_lines,
    reason = "one scenario script with its checks, read top to bottom"
)]
fn direct_control(seed: u64, rules: &Rules, checks: &mut Checks) -> Result<World, String> {
    // A direct order is never overridden: a raid puts the holding on alert, the garrison
    // under a direct order stays where it was sent.
    let mut w = World::new(seed, rules)?;
    let g = w.garrison;
    w.pending.push(CommanderInput {
        commander: C1,
        client_seq: 0,
        kind: CommanderInputKind::Connected,
    });
    w.order(C1, g, FleetOrder::Launch, false);
    w.run(2);
    let away = w.at(Vec2::new(0.0, 25_000.0));
    w.send(
        C1,
        Command::Fleet {
            fleet: g,
            order: FleetOrder::Move { to: away },
            queue: Queueing::Replace,
            retreat: false,
        },
    );
    w.run(5);
    let raiders = w.hostiles(&[ShipClass::Vx; 2], 7_000.0, 0.0);
    let _ = raiders;
    let outs = w.run(30 * SECOND);
    let alert = deputy_event(&outs, |e| matches!(e, DeputyEvent::AlertStarted { .. }));
    let kept = w.state.fleet(g).is_some_and(|f| {
        matches!(f.order, Some(FleetOrder::Move { .. }) | None) && f.direct_control.is_some()
    });
    check(
        alert.is_some() && kept,
        "on alert, the garrison under a direct order keeps it".to_owned(),
        checks,
    );
    // Release: back to its role, docking at the mother station.
    w.send(C1, Command::Doctrine(DoctrineCommand::Release { fleet: g }));
    let outs = w.run(3 * 60 * SECOND);
    // Docked at home again, or on its way there or into the fight under the Deputy.
    let mother = w.mother;
    let back = outs.iter().any(|o| {
        o.fleet_events.iter().any(
            |e| matches!(e, FleetEvent::Docked { fleet, holding } if *fleet == g && *holding == mother),
        )
    }) || w.state.fleet(g).is_some_and(|f| {
        matches!(
            f.order,
            Some(FleetOrder::Attack { .. } | FleetOrder::Dock { .. } | FleetOrder::Move { .. })
        ) && f.direct_control.is_none()
    });
    check(back, "Release returns it to its role".to_owned(), checks);

    // Retreat only with the opt-in (RK-10): a direct order into a losing fight.
    for opt_in in [false, true] {
        let mut r = World::new(seed, rules)?;
        let g = r.garrison;
        r.order(C1, g, FleetOrder::Launch, opt_in);
        r.run(2);
        // Eight VX of C2 within weapon range of it.
        let here = r
            .state
            .fleet(g)
            .map(|f| Vec2::from(f.position))
            .ok_or("no garrison")?;
        let enemy = place_fleet(
            &mut r.state,
            Owner::Commander(C2),
            r.system,
            here + Vec2::new(1_200.0, 0.0),
            std::f64::consts::PI,
            &[ShipClass::Vx; 16],
        );
        sense(&mut r.state);
        r.order(C1, g, FleetOrder::StationKeep { relative_to: None }, opt_in);
        r.order(C2, enemy, FleetOrder::Attack { target: g.sim_id() }, false);
        let mut retreat = false;
        // The fleet's hull as the retreat threshold reads it: its ships' hull out of their
        // maximum plus that of the ships it lost (A1 plan §5.5).
        let threshold = u64::from(
            rules
                .ai
                .preset(nomad_sim::DoctrinePreset::Balanced)
                .retreat_below_permille,
        );
        let ship_max = u64::from(rules.combat.class(ShipClass::Vx).hull);
        let (mut lost, mut below) = (0_u64, false);
        for _ in 0..3 * 60 * SECOND {
            let out = r.tick();
            lost += ship_max
                * out
                    .combat_events
                    .iter()
                    .filter(|e| matches!(e, nomad_sim::CombatEvent::ShipDestroyed { fleet, .. } if *fleet == g))
                    .count() as u64;
            if let Some(f) = r.state.fleet(g) {
                let (left, max) = f
                    .ships
                    .iter()
                    .filter_map(|&s| r.state.ship_hull(s))
                    .fold((0_u64, 0_u64), |(a, b), (l, m)| {
                        (a + u64::from(l), b + u64::from(m))
                    });
                below |= left * 1_000 < threshold * (max + lost);
            }
            // The Deputy's retreat applied (it may dock at once, beside the station).
            retreat |= r
                .applied(Tick::ZERO, |c| {
                    matches!(c, Command::Fleet { fleet, order: FleetOrder::Retreat { .. }, .. } if *fleet == g)
                })
                .is_some();
            if retreat || r.state.fleet(g).is_none() {
                break;
            }
        }
        check(
            below && retreat == opt_in,
            format!(
                "under a direct order {} the opt-in, below the threshold ({below}): {}",
                if opt_in { "with" } else { "without" },
                if retreat { "retreated" } else { "no retreat" }
            ),
            checks,
        );
    }

    // The D-162 lapse: offline, the grace ends, the fleet returns to its role.
    let mut l = World::new(seed, rules)?;
    let g = l.garrison;
    l.pending.push(CommanderInput {
        commander: C1,
        client_seq: 0,
        kind: CommanderInputKind::Connected,
    });
    l.order(C1, g, FleetOrder::Launch, false);
    l.run(2);
    // A patrol never completes, so it is still a direct order when the grace ends.
    let route = vec![
        l.at(Vec2::new(0.0, 20_000.0)),
        l.at(Vec2::new(20_000.0, 20_000.0)),
    ];
    l.order(C1, g, FleetOrder::Patrol { route }, false);
    l.run(5);
    l.pending.push(CommanderInput {
        commander: C1,
        client_seq: 0,
        kind: CommanderInputKind::Disconnected,
    });
    let outs = l.run(6 * 60 * SECOND);
    let lapsed = outs.iter().any(|o| {
        o.announcements.iter().any(|a| matches!(&a.kind, nomad_sim::AnnouncementKind::OrdersLapsed { fleets, .. } if fleets.contains(&g)))
    });
    let docked = outs.iter().any(|o| {
        o.fleet_events
            .iter()
            .any(|e| matches!(e, FleetEvent::Docked { fleet, .. } if *fleet == g))
    });
    check(
        lapsed && docked,
        format!("after the grace its orders lapsed ({lapsed}) and it went home to dock ({docked})"),
        checks,
    );
    Ok(w)
}

/// Scenario 6: the Deputy defends during a catch-up.
fn catch_up(seed: u64, rules: &Rules, checks: &mut Checks) -> Result<World, String> {
    let mut w = World::new(seed, rules)?;
    w.operator.push(OperatorInput::CatchUpStart);
    w.run(1);
    check(w.state.catching_up(), "catching up".to_owned(), checks);
    w.hostiles(&[ShipClass::Vx; 2], 7_000.0, 0.0);
    let outs = w.run(30 * SECOND);
    let alert = deputy_event(&outs, |e| matches!(e, DeputyEvent::AlertStarted { .. }));
    let launched = outs
        .iter()
        .flat_map(|o| &o.fleet_events)
        .any(|e| matches!(e, FleetEvent::Launched { fleet, .. } if *fleet == w.garrison));
    let nobody = COMMANDERS.iter().all(|&c| !w.state.counts_as_online(c));
    check(
        alert.is_some() && launched && nobody,
        "with everyone offline, the Deputy put the holding on alert and launched the garrison"
            .to_owned(),
        checks,
    );
    Ok(w)
}

type Scenario = fn(u64, &Rules, &mut Checks) -> Result<World, String>;

fn scenarios() -> Vec<(&'static str, Scenario)> {
    vec![
        ("1 raid on a holding (§5.3–§5.6)", raid),
        ("2 SHLD SURGE and FLAK SCREEN (D-147, D-148)", abilities),
        ("3 convoy evacuation and resumption (D-210)", convoys),
        (
            "3 contested gate: reroute or hold (D-210, D-285)",
            contested_gate,
        ),
        ("4 replacements, budget and a module (D-208)", rebuilding),
        ("5 direct control (RK-10, D-162)", direct_control),
        ("6 catch-up (D-047, D-080)", catch_up),
    ]
}

/// `--deputy`: every scenario with its checks.
pub fn run(out: &mut impl Write, run: &DeputyRun) -> Result<(), String> {
    let w = |out: &mut dyn Write, line: String| writeln!(out, "{line}").map_err(|e| e.to_string());
    w(
        out,
        format!(
            "deputy | seed {} | C1 defends with Balanced doctrine; C2 attacks",
            run.seed
        ),
    )?;
    let mut failed = 0;
    for (name, scenario) in scenarios() {
        let started = Instant::now();
        let mut checks = Vec::new();
        let world = scenario(run.seed, &run.rules, &mut checks)?;
        let ok = checks.iter().filter(|(ok, _)| *ok).count();
        w(
            out,
            format!(
                "{name} | {ok} of {} checks passed | {:.1} s",
                checks.len(),
                started.elapsed().as_secs_f64()
            ),
        )?;
        for (passed, what) in &checks {
            w(
                out,
                format!("  [{}] {what}", if *passed { "ok" } else { "FAILED" }),
            )?;
        }
        for (tick, line) in world.log.iter().take(12) {
            w(out, format!("    {:>7.1} s  {line}", secs(tick.get())))?;
        }
        if ok < checks.len() {
            failed += 1;
        }
    }
    if failed > 0 {
        return Err(format!("{failed} deputy scenarios failed their checks"));
    }
    Ok(())
}

/// `--deputy bench`: the Deputy's cost and the catch-up speed (D-203).
pub fn bench(out: &mut impl Write, run: &DeputyRun, ticks: u64) -> Result<(), String> {
    let mut state = generate_stage1_with_rules(
        SeasonConfig::new(run.seed, 0),
        COMMANDERS,
        run.rules.clone(),
    )
    .map_err(|e| e.to_string())?;
    let mothers: Vec<(CommanderId, SystemId, Vec2)> = COMMANDERS
        .iter()
        .filter_map(|&c| {
            let (_, h) = state
                .holdings()
                .find(|(_, h)| h.owner == c && h.mother_station)?;
            let site = state.galaxy().site(h.site)?;
            Some((c, site.system, Vec2::from(site.position)))
        })
        .collect();
    let mut deputy_us = Vec::with_capacity(usize::try_from(ticks).unwrap_or(0));
    let mut tick_us = Vec::with_capacity(deputy_us.capacity());
    let mut issued = 0_usize;
    let raid_every = 30 * 60 * SECOND;
    let started = Instant::now();
    for t in 0..ticks {
        let mut inputs = TickInputs::default();
        if t == 0 {
            inputs.operator.push(OperatorInput::CatchUpStart);
        }
        // A raid on one mother station every 30 minutes, in turn, by the next commander.
        if t % raid_every == raid_every / 2 {
            let i = usize::try_from(t / raid_every).unwrap_or(0) % mothers.len();
            let (_, system, centre) = mothers[i];
            let (raider, _, _) = mothers[(i + 1) % mothers.len()];
            let fleet = place_fleet(
                &mut state,
                Owner::Commander(raider),
                system,
                centre + Vec2::new(9_000.0, 0.0),
                0.0,
                &[ShipClass::Vx; 4],
            );
            inputs.commanders.push(CommanderInput {
                commander: raider,
                client_seq: t,
                kind: CommanderInputKind::Command(Command::Fleet {
                    fleet,
                    order: FleetOrder::AttackMove {
                        to: nomad_sim::Location {
                            system,
                            position: centre.into(),
                        },
                    },
                    queue: Queueing::Replace,
                    retreat: false,
                }),
            });
        }
        let copy = Instant::now();
        std::hint::black_box(state.probe_deputies(false));
        let copied = copy.elapsed();
        let eval = Instant::now();
        issued += state.probe_deputies(true);
        let evaluated = eval.elapsed();
        deputy_us.push(evaluated.saturating_sub(copied).as_secs_f64() * 1e6);
        let step_started = Instant::now();
        step(&mut state, &inputs);
        tick_us.push(step_started.elapsed().as_secs_f64() * 1e6);
    }
    let wall = started.elapsed().as_secs_f64();
    let step_wall: f64 = tick_us.iter().sum::<f64>() / 1e6;
    let stats = |v: &mut Vec<f64>| {
        v.sort_by(f64::total_cmp);
        let mean = v.iter().sum::<f64>() / v.len().max(1) as f64;
        let p99 = v
            .get(v.len().saturating_sub(1) * 99 / 100)
            .copied()
            .unwrap_or(0.0);
        (mean, p99, v.last().copied().unwrap_or(0.0))
    };
    let (d_mean, d_p99, d_max) = stats(&mut deputy_us);
    let (t_mean, t_p99, t_max) = stats(&mut tick_us);
    let universe_s = secs(ticks);
    let counted = deputy_counters(&state);
    writeln!(
        out,
        "deputy bench | seed {} | stage 1, 4 commanders, catch-up, a raid every 30 min | {:.1} h ({ticks} ticks)\n  \
         Deputies per tick: mean {d_mean:.2} µs, p99 {d_p99:.2} µs, max {d_max:.0} µs = {:.1}% (mean) and {:.1}% (p99) of a 500× tick (200 µs; D-203 budget 25%) | {issued} commands\n  \
         whole tick: mean {t_mean:.1} µs, p99 {t_p99:.1} µs, max {t_max:.0} µs | catch-up speed {:.0}× real time (steps only; {:.0}× with the probes), D-203 target ≥ 500×",
        run.seed,
        universe_s / 3_600.0,
        d_mean / 2.0,
        d_p99 / 2.0,
        universe_s / step_wall.max(1e-9),
        universe_s / wall.max(1e-9),
    )
    .map_err(|e| e.to_string())?;
    writeln!(
        out,
        "  Deputy commands: {} issued, {} applied, rejected {:?}, dropped {:?}",
        counted.issued, counted.applied, counted.rejected, counted.dropped
    )
    .map_err(|e| e.to_string())
}

/// Every commander's Deputy counters summed: what its commands came to (A1 plan §4.8, the
/// counters reconcile).
fn deputy_counters(state: &UniverseState) -> nomad_sim::LayerCounters {
    let mut counted = nomad_sim::LayerCounters::default();
    for ((_, layer), c) in state.ai_counters() {
        if *layer == nomad_sim::Layer::Deputy {
            counted.issued += c.issued;
            counted.applied += c.applied;
            for (reason, n) in &c.rejected {
                *counted.rejected.entry(*reason).or_default() += n;
            }
            for (reason, n) in &c.dropped {
                *counted.dropped.entry(*reason).or_default() += n;
            }
        }
    }
    counted
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_one(scenario: Scenario) {
        let mut checks = Vec::new();
        scenario(1, &Rules::embedded(), &mut checks).unwrap();
        let failed: Vec<_> = checks.iter().filter(|(ok, _)| !ok).collect();
        assert!(failed.is_empty(), "{failed:?}");
    }

    #[test]
    fn the_scenario_worlds_set_up() {
        let w = World::new(1, &Rules::embedded()).unwrap();
        assert!(w.state.fleet(w.garrison).is_some() && w.state.fleet(w.reserve).is_some());
        assert_eq!(scenarios().len(), 7);
    }

    #[test]
    #[ignore = "long simulation: minutes of raid; CI runs fast tests only (D-190), run with --ignored"]
    fn deputy_raid() {
        run_one(raid);
    }

    #[test]
    #[ignore = "long simulation: a breach's worth of ticks; CI runs fast tests only (D-190), run with --ignored"]
    fn deputy_abilities() {
        run_one(abilities);
    }

    #[test]
    #[ignore = "long simulation: minutes of convoy; CI runs fast tests only (D-190), run with --ignored"]
    fn deputy_convoys() {
        run_one(convoys);
        run_one(contested_gate);
    }

    #[test]
    #[ignore = "long simulation: up to a day of ticks before the window; CI runs fast tests only (D-190), run with --ignored"]
    fn deputy_rebuilding() {
        run_one(rebuilding);
    }

    #[test]
    #[ignore = "long simulation: minutes of fights and a grace; CI runs fast tests only (D-190), run with --ignored"]
    fn deputy_direct_control_and_catch_up() {
        run_one(direct_control);
        run_one(catch_up);
    }
}
