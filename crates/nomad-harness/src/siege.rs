//! `--siege`: the siege lifecycle end to end, driven by scripted commands (S6 plan §8; doc 30
//! §3: "sieges run end to end when driven by scripted commands").
//!
//! Every scenario is a stage-1 Universe in which commander 1, the defender D, is human with
//! the window 19:00–23:00 UTC and never connects, so D is offline throughout. Commander 2, the
//! attacker A, is an AI whose sessions the script sets with `SetPresence`, as A5's schedules
//! will (S6 plan §4.1). A's orders are commands through the single validator (D-077); an
//! order given while A is online lapses 120 s after A goes offline (D-162). D's holdings
//! beyond the mother station, A's fleets and D's docked ships are fixtures
//! (`nomad_sim::scenario`), not claims. Every tick is stepped; nothing is skipped.
//!
//! | Scenario | Shows |
//! |---|---|
//! | `capture` | The S6 plan §5 timeline: a breach on day 1 at 14:20, Reinforced to the day 2 19:00 slot, the siege, Fallen and capture, D's docked ships' emergency jump (D-015, D-057, D-158, D-166) |
//! | `clock` | Held by the clock: A goes offline 3 min into the siege, its assault lapses and the clock runs on (D-161) |
//! | `absent` | Held at once: no attacker online at the exit (D-057, D-160) |
//! | `catch-up` | Held through a catch-up across the exit: the PC is off from day 2 18:00 to day 3 08:00 (S6 plan §5, D-047) |
//! | `stagger` | Five breaches between 10:00 and 11:00 staggered as 4 + 1 (D-033, D-158) |
//! | `jump` | A fallen mother station: the attacker's outpost, the emergency jump, the comeback grant and 72 h of protection (D-034, D-164 to D-167, D-224) |
//! | `protection` | A sector worn to 0 during protection does not breach; the first hit after it does (D-159) |
//! | `peripheral` | A solar array disabled outside the window and destroyed inside it (doc 12 §3, D-138, D-169) |
//!
//! The S6 plan §5 timeline has A offline at 14:20 (its sessions are 08:00–09:30 and
//! 18:00–20:00), but an attack needs its attacker online (PR-3, D-162), so the script gives A
//! a short session at 14:00–14:30 for the breach. Each scenario checks its outcome against
//! the rules and fails the run if a check fails.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Write;
use std::time::Instant;

use nomad_sim::galaxy::generate_stage1_roster;
use nomad_sim::scenario::{place_docked_fleet, place_fleet, place_holding};
use nomad_sim::snapshot::state_hash;
use nomad_sim::{
    AnnouncementKind, Command, CommanderId, CommanderInput, CommanderInputKind, CommanderSetup,
    EconomyEvent, FleetId, FleetOrder, HeldReason, HoldingId, OperatorInput, Owner, Place,
    QuarterHour, Queueing, Rules, SeasonConfig, ShipClass, SimId, SiteId, Slot, SystemId,
    TICKS_PER_SECOND, Tick, TickInputs, Ticks, UniverseState, Vec2, step,
};

const D: CommanderId = CommanderId(1);
const A: CommanderId = CommanderId(2);
const SECOND: u64 = TICKS_PER_SECOND;
const MINUTE: u64 = 60 * SECOND;
const HOUR: u64 = 60 * MINUTE;
const DAY: u64 = 24 * HOUR;
/// Sectors in pylon slot order N, E, S, W (D-046).
const EAST: u8 = 1;
const SECTORS: [&str; 4] = ["N", "E", "S", "W"];

/// What `--siege` runs.
pub struct SiegeRun {
    /// The season seed.
    pub seed: u64,
    /// The rules (D-104).
    pub rules: Rules,
}

/// Day `day` at `hours:minutes:seconds` UTC: the season starts at the epoch's midnight.
const fn at(day: u64, hours: u64, minutes: u64, seconds: u64) -> Tick {
    Tick::new(day * DAY + hours * HOUR + minutes * MINUTE + seconds * SECOND)
}

/// `day d HH:MM:SS` for a tick.
fn when(tick: Tick) -> String {
    let t = tick.get() / SECOND;
    format!(
        "day {} {:02}:{:02}:{:02}",
        t / 86_400,
        t % 86_400 / 3_600,
        t % 3_600 / 60,
        t % 60
    )
}

/// What the script does on a tick.
#[derive(Clone, Debug)]
enum Act {
    /// A's schedule has it online or offline.
    Presence(bool),
    /// A sends a command.
    Order(Command),
    CatchUpStart,
    CatchUpEnd,
}

/// One scenario's Universe, its script and what it saw.
struct World {
    state: UniverseState,
    script: BTreeMap<Tick, Vec<Act>>,
    names: BTreeMap<HoldingId, String>,
    seq: u64,
    /// Every announcement, with its tick.
    said: Vec<(Tick, AnnouncementKind)>,
    /// Module events, with their tick.
    modules: Vec<(Tick, EconomyEvent)>,
    log: Vec<String>,
    rejected: Vec<String>,
    micros: Vec<f64>,
}

impl World {
    /// A stage-1 Universe with D human (window 19:00 UTC) and the others AI.
    fn new(seed: u64, rules: &Rules) -> Result<Self, String> {
        let window = QuarterHour::at(19, 0).ok_or("19:00 is a quarter hour")?;
        let roster = [
            CommanderSetup::human(D, window),
            CommanderSetup::ai(A),
            CommanderSetup::ai(CommanderId(3)),
            CommanderSetup::ai(CommanderId(4)),
        ];
        let state = generate_stage1_roster(SeasonConfig::new(seed, 0), roster, rules.clone())
            .map_err(|e| e.to_string())?;
        let mut world = Self {
            state,
            script: BTreeMap::new(),
            names: BTreeMap::new(),
            seq: 0,
            said: Vec::new(),
            modules: Vec::new(),
            log: Vec::new(),
            rejected: Vec::new(),
            micros: Vec::new(),
        };
        let mother = world.mother();
        world.names.insert(mother, "H1 (mother)".to_owned());
        Ok(world)
    }

    fn mother(&self) -> HoldingId {
        self.state
            .holdings()
            .find(|(_, h)| h.owner == D && h.mother_station)
            .map_or(HoldingId(SimId::from_raw(0)), |(id, _)| id)
    }

    fn home(&self) -> Result<SystemId, String> {
        let site = self
            .state
            .holding(self.mother())
            .ok_or("no mother station")?
            .site;
        Ok(self.state.galaxy().site(site).ok_or("no site")?.system)
    }

    /// D gains a completed holding on a free site of its home system, named `name`.
    fn holding(&mut self, name: &str) -> Result<HoldingId, String> {
        let system = self.home()?;
        let site = self
            .state
            .galaxy()
            .system(system)
            .ok_or("no system")?
            .sites
            .iter()
            .copied()
            .find(|&s| self.state.holding_at(s).is_none())
            .ok_or("no free site")?;
        let h = place_holding(&mut self.state, D, site).ok_or("cannot place a holding")?;
        self.names.insert(h, name.to_owned());
        Ok(h)
    }

    fn centre(&self, holding: HoldingId) -> Result<(SystemId, Vec2), String> {
        let h = self.state.holding(holding).ok_or("no holding")?;
        let site = self.state.galaxy().site(h.site).ok_or("no site")?;
        Ok((site.system, Vec2::from(site.position)))
    }

    /// A fleet of A's, `distance_m` east of `holding`, facing it.
    fn attackers(
        &mut self,
        holding: HoldingId,
        distance_m: f64,
        lancers: usize,
        wardens: usize,
    ) -> Result<FleetId, String> {
        let (system, centre) = self.centre(holding)?;
        let classes = [
            vec![ShipClass::Lancer; lancers],
            vec![ShipClass::Warden; wardens],
        ]
        .concat();
        Ok(place_fleet(
            &mut self.state,
            Owner::Commander(A),
            system,
            centre + Vec2::new(distance_m, 0.0),
            std::f64::consts::PI,
            &classes,
        ))
    }

    fn plan(&mut self, tick: Tick, act: Act) {
        self.script.entry(tick).or_default().push(act);
    }

    /// A online from `from` to `to`.
    fn session(&mut self, from: Tick, to: Tick) {
        self.plan(from, Act::Presence(true));
        self.plan(to, Act::Presence(false));
    }

    /// A orders `fleet` to attack `target` on `tick`.
    fn attack(&mut self, tick: Tick, fleet: FleetId, target: SimId) {
        self.plan(
            tick,
            Act::Order(Command::Fleet {
                fleet,
                order: FleetOrder::Attack { target },
                queue: Queueing::Replace,
                retreat: false,
            }),
        );
    }

    fn core(&self, holding: HoldingId) -> Option<SimId> {
        self.state
            .holding_economy(holding)?
            .slots
            .into_iter()
            .find_map(|(slot, m)| (slot == Slot::Core).then_some(m?.id))
    }

    fn name(&self, holding: HoldingId) -> String {
        self.names
            .get(&holding)
            .cloned()
            .unwrap_or_else(|| format!("holding {}", holding.sim_id().get()))
    }

    fn site_name(&self, site: SiteId) -> String {
        let system = self
            .state
            .galaxy()
            .site(site)
            .map_or_else(String::new, |s| {
                format!(" in system #{}", s.system.sim_id().get())
            });
        format!("site #{}{system}", site.sim_id().get())
    }

    /// Steps until `end`, or until `stop` holds after a tick.
    fn run(&mut self, end: Tick, stop: impl Fn(&Self) -> bool) {
        while self.state.tick() < end {
            let now = self.state.tick().next();
            let mut inputs = TickInputs::default();
            for act in self.script.remove(&now).unwrap_or_default() {
                match act {
                    Act::Presence(online) => inputs.operator.push(OperatorInput::SetPresence {
                        commander: A,
                        online,
                    }),
                    Act::CatchUpStart => inputs.operator.push(OperatorInput::CatchUpStart),
                    Act::CatchUpEnd => inputs.operator.push(OperatorInput::CatchUpEnd),
                    Act::Order(command) => {
                        self.seq += 1;
                        inputs.commanders.push(CommanderInput {
                            commander: A,
                            client_seq: self.seq,
                            kind: CommanderInputKind::Command(command),
                        });
                    }
                }
            }
            let started = Instant::now();
            let out = step(&mut self.state, &inputs);
            self.micros.push(started.elapsed().as_secs_f64() * 1e6);
            for result in &out.commands {
                if let Err(reason) = result.outcome {
                    self.rejected.push(format!("{} {reason:?}", when(out.tick)));
                }
            }
            for a in &out.announcements {
                let line = self.describe(&a.kind);
                if let Some(line) = line {
                    self.log.push(format!("  {}  {line}", when(out.tick)));
                }
                self.said.push((out.tick, a.kind.clone()));
            }
            for e in &out.economy_events {
                if matches!(
                    e,
                    EconomyEvent::ModuleDisabled { .. }
                        | EconomyEvent::ModuleDestroyed { .. }
                        | EconomyEvent::OutpostDeployed { .. }
                ) {
                    self.modules.push((out.tick, *e));
                }
            }
            if stop(self) {
                break;
            }
        }
    }

    /// A timeline line for an announcement, if it is one of the siege's.
    fn describe(&self, kind: &AnnouncementKind) -> Option<String> {
        let sector = |s: u8| SECTORS.get(usize::from(s)).copied().unwrap_or("?");
        Some(match kind {
            AnnouncementKind::Breached {
                holding,
                sector: s,
                exit_tick,
                attackers,
            } => format!(
                "{} breached in sector {}: Reinforced, the exit at {} (attackers {attackers:?})",
                self.name(*holding),
                sector(*s),
                when(*exit_tick)
            ),
            AnnouncementKind::SiegeStarted {
                holding,
                end_tick,
                open_sector,
            } => format!(
                "{}: siege, sector {} open, the clock ends at {}",
                self.name(*holding),
                sector(*open_sector),
                when(*end_tick)
            ),
            AnnouncementKind::Held {
                holding,
                reason,
                breachable_from,
            } => format!(
                "{}: Held ({}), breachable from {}",
                self.name(*holding),
                match reason {
                    HeldReason::Clock => "the clock ran out",
                    HeldReason::NoAttackerOnline => "no attacker online",
                },
                when(*breachable_from)
            ),
            AnnouncementKind::Captured {
                holding,
                from,
                to,
                breachable_from,
            } => format!(
                "{} fell: captured from C{} by C{}, protected until {}",
                self.name(*holding),
                from.0,
                to.0,
                when(*breachable_from)
            ),
            AnnouncementKind::OrdersLapsed { commander, fleets } => format!(
                "C{} offline for 120 s: {} fleet(s) drop their orders",
                commander.0,
                fleets.len()
            ),
            AnnouncementKind::WindowMoved { .. } => return None,
            _ => return self.describe_jump(kind),
        })
    }

    /// A timeline line for the announcements of emergency jumps and outposts.
    fn describe_jump(&self, kind: &AnnouncementKind) -> Option<String> {
        Some(match kind {
            AnnouncementKind::FleetsJumped { commander, fleets } => {
                let mut text = format!("C{}'s docked ships emergency-jump:", commander.0);
                for (fleet, site, arrive) in fleets {
                    let _ = write!(
                        text,
                        " fleet {} to {} arriving {};",
                        fleet.sim_id().get(),
                        self.site_name(*site),
                        when(*arrive)
                    );
                }
                text
            }
            AnnouncementKind::MotherStationFell {
                site,
                from,
                to,
                outpost,
            } => format!(
                "C{}'s mother station fell on {}: the site is C{}'s outpost {}",
                from.0,
                self.site_name(*site),
                to.0,
                outpost.sim_id().get()
            ),
            AnnouncementKind::MotherStationJumped {
                to_site,
                arrival_tick,
                protected_until,
                grant,
                ..
            } => format!(
                "the station jumps to {}, arriving {}, protected until {}; comeback grant {} A + \
                 {} C",
                to_site.map_or_else(|| "no free site".to_owned(), |s| self.site_name(s)),
                arrival_tick.map_or_else(|| "-".to_owned(), when),
                protected_until.map_or_else(|| "-".to_owned(), when),
                grant.alloy / 1_000,
                grant.crystal / 1_000
            ),
            AnnouncementKind::StationLanded {
                site,
                breachable_from,
                ..
            } => format!(
                "the station lands on {}: a mother station again, protected until {}",
                self.site_name(*site),
                when(*breachable_from)
            ),
            AnnouncementKind::OutpostDestroyed { holding, .. } => {
                format!("outpost {} destroyed", holding.sim_id().get())
            }
            _ => return None,
        })
    }

    /// The first announcement matching `pick`, with its tick.
    fn first<T>(&self, pick: impl Fn(&AnnouncementKind) -> Option<T>) -> Option<(Tick, T)> {
        self.said
            .iter()
            .find_map(|(tick, kind)| pick(kind).map(|t| (*tick, t)))
    }
}

/// Records `what` as failed unless `ok`.
fn check(ok: bool, what: &str, errors: &mut Vec<String>) {
    if !ok {
        errors.push(what.to_owned());
    }
}

/// The breach of `holding`: its tick, sector and exit.
fn breach_of(world: &World, holding: HoldingId) -> Option<(Tick, (u8, Tick))> {
    world.first(|k| match *k {
        AnnouncementKind::Breached {
            holding: h,
            sector,
            exit_tick,
            ..
        } if h == holding => Some((sector, exit_tick)),
        _ => None,
    })
}

/// When `holding` held, and why.
fn held_of(world: &World, holding: HoldingId) -> Option<(Tick, (HeldReason, Tick))> {
    world.first(|k| match *k {
        AnnouncementKind::Held {
            holding: h,
            reason,
            breachable_from,
        } if h == holding => Some((reason, breachable_from)),
        _ => None,
    })
}

/// Whether `tick` lies inside D's window, 19:00–23:00 UTC.
const fn in_window(tick: Tick) -> bool {
    let of_day = tick.get() % DAY;
    of_day >= 19 * HOUR && of_day < 23 * HOUR
}

/// One scenario: its name, what it shows, and the function that runs it.
struct Scenario {
    name: &'static str,
    run: fn(&mut World, &mut Vec<String>) -> Result<(), String>,
}

fn scenarios() -> Vec<Scenario> {
    vec![
        Scenario {
            name: "capture",
            run: capture,
        },
        Scenario {
            name: "clock",
            run: clock,
        },
        Scenario {
            name: "absent",
            run: absent,
        },
        Scenario {
            name: "catch-up",
            run: catch_up,
        },
        Scenario {
            name: "stagger",
            run: stagger,
        },
        Scenario {
            name: "jump",
            run: jump,
        },
        Scenario {
            name: "protection",
            run: protection,
        },
        Scenario {
            name: "peripheral",
            run: peripheral,
        },
    ]
}

/// The S6 plan §5 timeline: D's H2 breached on day 1 at 14:20 by A's fleet (10 LANCER and 4
/// WARDEN, D-058's mid-size fleet), the exit at day 2 19:00, the siege, the fall and the
/// capture, D's docked ships jumping to the mother station.
fn capture(w: &mut World, errors: &mut Vec<String>) -> Result<(), String> {
    let h2 = w.holding("H2")?;
    let docked = place_docked_fleet(&mut w.state, h2, &[ShipClass::Vx; 2]).ok_or("no room")?;
    let fleet = w.attackers(h2, 6_000.0, 10, 4)?;
    for day in 1..=2 {
        w.session(at(day, 8, 0, 0), at(day, 9, 30, 0));
        w.session(at(day, 18, 0, 0), at(day, 20, 0, 0));
    }
    w.session(at(1, 14, 0, 0), at(1, 14, 30, 0));
    w.attack(at(1, 14, 18, 30), fleet, h2.sim_id());
    let core = w.core(h2).ok_or("no core")?;
    w.attack(at(2, 19, 0, 1), fleet, core);
    let mother = w.mother();
    w.run(at(2, 21, 0, 0), |w| {
        w.state
            .fleet(docked)
            .is_some_and(|f| f.place == Place::Docked(mother))
    });
    let (breached, (sector, exit)) = breach_of(w, h2).ok_or("H2 was not breached")?;
    check(sector == EAST, "the breach is in sector E", errors);
    check(
        exit == at(2, 19, 0, 0),
        "the exit is day 2 19:00 (D-158)",
        errors,
    );
    let fall = w.first(|k| match *k {
        AnnouncementKind::Captured {
            holding,
            to,
            breachable_from,
            ..
        } if holding == h2 => Some((to, breachable_from)),
        _ => None,
    });
    let Some((fell, (to, protected))) = fall else {
        errors.push("H2 did not fall".to_owned());
        return Ok(());
    };
    check(to == A, "A captures (most core damage, D-163)", errors);
    check(
        in_window(fell),
        "the fall lies in D's window (D-015)",
        errors,
    );
    check(
        fell.since(breached) >= Ticks::from_hours(12),
        "the fall is at least 12 h after the breach (D-015)",
        errors,
    );
    check(
        protected == fell.after(Ticks::from_hours(48)),
        "A is protected for 48 h (D-057)",
        errors,
    );
    check(
        w.state.holding(h2).is_some_and(|h| h.owner == A),
        "H2 is A's",
        errors,
    );
    check(
        w.state.module(core).is_some_and(|m| m.hp >= m.max_hp / 4),
        "the core was restored to 25% (D-057)",
        errors,
    );
    check(
        w.state
            .fleet(docked)
            .is_some_and(|f| f.place == Place::Docked(mother)),
        "D's docked ships jumped to the mother station and docked (D-057, D-166)",
        errors,
    );
    w.log.push(format!(
        "  the core was lost {:.1} h after the breach, inside D's window, with D offline",
        fell.since(breached).get() as f64 / HOUR as f64
    ));
    Ok(())
}

/// A breach at day 1 06:30; A online at the exit, attacks the core, and goes offline at
/// 19:03: its orders lapse at 19:05, the clock runs on and the holding holds (D-161).
fn clock(w: &mut World, errors: &mut Vec<String>) -> Result<(), String> {
    let h2 = w.holding("H2")?;
    let fleet = w.attackers(h2, 6_000.0, 10, 4)?;
    w.session(at(1, 6, 0, 0), at(1, 7, 0, 0));
    w.attack(at(1, 6, 28, 30), fleet, h2.sim_id());
    w.session(at(1, 18, 30, 0), at(1, 19, 3, 0));
    let core = w.core(h2).ok_or("no core")?;
    w.attack(at(1, 19, 0, 1), fleet, core);
    w.run(at(1, 20, 1, 0), |w| held_of(w, h2).is_some());
    let (_, (_, exit)) = breach_of(w, h2).ok_or("H2 was not breached")?;
    check(exit == at(1, 19, 0, 0), "the exit is day 1 19:00", errors);
    let held = held_of(w, h2);
    check(
        held == Some((
            at(1, 20, 0, 0),
            (
                HeldReason::Clock,
                at(1, 20, 0, 0).after(Ticks::from_hours(48)),
            ),
        )),
        "Held by the clock at 20:00 with 48 h of protection (D-015)",
        errors,
    );
    let hp = w.state.module(core).map_or(0, |m| m.hp);
    check(hp > 0, "the core stands", errors);
    w.log.push(format!("  the core stands at {hp} hull"));
    Ok(())
}

/// A breach at day 1 06:30; A is offline at the exit: Held at once (D-057, D-160).
fn absent(w: &mut World, errors: &mut Vec<String>) -> Result<(), String> {
    let h2 = w.holding("H2")?;
    let fleet = w.attackers(h2, 6_000.0, 10, 4)?;
    w.session(at(1, 6, 0, 0), at(1, 7, 0, 0));
    w.attack(at(1, 6, 28, 30), fleet, h2.sim_id());
    w.run(at(1, 19, 1, 0), |w| held_of(w, h2).is_some());
    let held = held_of(w, h2);
    check(
        held == Some((
            at(1, 19, 0, 0),
            (
                HeldReason::NoAttackerOnline,
                at(1, 19, 0, 0).after(Ticks::from_hours(48)),
            ),
        )),
        "Held at the exit with no attacker online, 48 h of protection",
        errors,
    );
    Ok(())
}

/// The S6 plan §5 timeline with the PC off from day 2 18:00 to day 3 08:00: the server
/// catches up with everyone offline, and at 19:00 no attacker is online (D-047).
fn catch_up(w: &mut World, errors: &mut Vec<String>) -> Result<(), String> {
    let h2 = w.holding("H2")?;
    let fleet = w.attackers(h2, 6_000.0, 10, 4)?;
    w.session(at(1, 14, 0, 0), at(1, 14, 30, 0));
    w.attack(at(1, 14, 18, 30), fleet, h2.sim_id());
    w.session(at(2, 18, 0, 0), at(2, 20, 0, 0));
    w.plan(at(2, 18, 0, 0), Act::CatchUpStart);
    w.plan(at(3, 8, 0, 0), Act::CatchUpEnd);
    w.run(at(2, 19, 1, 0), |w| held_of(w, h2).is_some());
    let held = held_of(w, h2);
    check(
        held == Some((
            at(2, 19, 0, 0),
            (HeldReason::NoAttackerOnline, at(4, 19, 0, 0)),
        )),
        "Held at day 2 19:00 during the catch-up, protected until day 4 19:00",
        errors,
    );
    Ok(())
}

/// D's H2–H6 breached after 10:00 and before 11:00 on day 1: the exits are day 2 19:00,
/// 20:00, 21:00 and 22:00, and the fifth rolls over to day 3 19:00. A is online in the
/// windows but its orders lapsed at 11:02, so each siege runs its clock; the 22:00 siege ends
/// at 23:00, the end of the window (D-033, D-158).
fn stagger(w: &mut World, errors: &mut Vec<String>) -> Result<(), String> {
    let mut targets = Vec::new();
    for i in 2..=6 {
        let h = w.holding(&format!("H{i}"))?;
        let fleet = w.attackers(h, 6_000.0, 6, 0)?;
        targets.push((h, fleet));
    }
    w.session(at(1, 10, 0, 0), at(1, 11, 0, 0));
    for (i, &(h, fleet)) in (0_u64..).zip(&targets) {
        w.attack(at(1, 10, 3 + 10 * i, 0), fleet, h.sim_id());
    }
    w.session(at(2, 18, 30, 0), at(2, 23, 30, 0));
    w.session(at(3, 18, 30, 0), at(3, 20, 30, 0));
    let last = targets.last().map(|&(h, _)| h);
    w.run(at(3, 20, 1, 0), |w| {
        last.is_some_and(|h| held_of(w, h).is_some())
    });
    let exits: Vec<Tick> = targets
        .iter()
        .filter_map(|&(h, _)| breach_of(w, h).map(|(_, (_, exit))| exit))
        .collect();
    let expected = [
        at(2, 19, 0, 0),
        at(2, 20, 0, 0),
        at(2, 21, 0, 0),
        at(2, 22, 0, 0),
        at(3, 19, 0, 0),
    ];
    check(
        exits == expected,
        "the exits are 4 + 1 (D-033, D-158)",
        errors,
    );
    for (&(h, _), exit) in targets.iter().zip(expected) {
        let held = held_of(w, h).map(|(tick, (reason, _))| (tick, reason));
        check(
            held == Some((exit.after(Ticks::from_hours(1)), HeldReason::Clock)),
            &format!("{} holds by the clock an hour after its exit", w.name(h)),
            errors,
        );
    }
    Ok(())
}

/// D's mother station breached at 06:00 on day 1 by D-058's fleet; at the exit, 19:00, A is
/// online and kills the core. Its other modules are destroyed, the site becomes A's outpost,
/// the station jumps to a free site in the system of D's nearest holding (H2, D-164) with its
/// docked start kit, D receives the comeback grant, and the landed station has 72 h of
/// protection.
fn jump(w: &mut World, errors: &mut Vec<String>) -> Result<(), String> {
    let mother = w.mother();
    let h2 = w.holding("H2")?;
    let kit = w
        .state
        .fleet_ids()
        .into_iter()
        .find(|&f| {
            w.state
                .fleet(f)
                .is_some_and(|v| v.place == Place::Docked(mother))
        })
        .ok_or("no start kit")?;
    let fleet = w.attackers(mother, 6_000.0, 10, 4)?;
    w.session(at(1, 6, 0, 0), at(1, 7, 0, 0));
    w.attack(at(1, 6, 0, 1), fleet, mother.sim_id());
    w.session(at(1, 18, 30, 0), at(1, 21, 0, 0));
    let core = w.core(mother).ok_or("no core")?;
    w.attack(at(1, 19, 0, 1), fleet, core);
    let site = w.state.holding(mother).ok_or("no mother")?.site;
    w.run(at(1, 22, 0, 0), |w| {
        w.said
            .iter()
            .any(|(_, k)| matches!(k, AnnouncementKind::StationLanded { .. }))
    });
    jump_checks(w, (mother, h2, kit, site), errors);
    Ok(())
}

/// The checks of [`jump`].
fn jump_checks(
    w: &World,
    (mother, h2, kit, site): (HoldingId, HoldingId, FleetId, SiteId),
    errors: &mut Vec<String>,
) {
    let fell = w.first(|k| match *k {
        AnnouncementKind::MotherStationFell { outpost, to, .. } => Some((outpost, to)),
        _ => None,
    });
    let Some((fell_at, (outpost, to))) = fell else {
        errors.push("the mother station did not fall".to_owned());
        return;
    };
    check(
        to == A && in_window(fell_at),
        "A gains the site, in D's window",
        errors,
    );
    let outpost_core = w.state.holding_economy(outpost).and_then(|e| {
        e.slots
            .into_iter()
            .find_map(|(slot, m)| (slot == Slot::Core).then_some(m?))
    });
    check(
        outpost_core.is_some_and(|m| m.building_ms.is_some() && m.hp < m.max_hp / 5),
        "the outpost's core is under construction, its hull growing from 10% (D-224)",
        errors,
    );
    check(
        w.state
            .holding_at(site)
            .is_some_and(|(id, h)| id == outpost && h.owner == A),
        "the fallen site is A's outpost (D-057)",
        errors,
    );
    let destroyed = w
        .modules
        .iter()
        .filter(|(_, e)| matches!(e, EconomyEvent::ModuleDestroyed { holding, .. } if *holding == mother))
        .count();
    check(
        destroyed == 9,
        "the site's 9 other modules are destroyed (D-034)",
        errors,
    );
    let jumped = w.first(|k| match *k {
        AnnouncementKind::MotherStationJumped {
            to_site,
            arrival_tick,
            protected_until,
            grant,
            ..
        } => Some((to_site, arrival_tick, protected_until, grant)),
        _ => None,
    });
    let Some((_, (Some(to_site), Some(arrive), Some(protected), grant))) = jumped else {
        errors.push("the station did not jump to a site".to_owned());
        return;
    };
    let h2_system = w.centre(h2).map(|(s, _)| s);
    check(
        w.state.galaxy().site(to_site).map(|s| s.system) == h2_system.ok(),
        "it lands in the system of D's nearest holding (D-164)",
        errors,
    );
    check(
        grant == w.state.economy_of(D).core_kit_price,
        "the grant is one core kit: D is not behind the leader (D-167)",
        errors,
    );
    let landed = w.state.holding(mother);
    check(
        landed.is_some_and(|h| h.site == to_site && h.mother_station),
        "the station landed as D's mother station",
        errors,
    );
    check(
        protected == arrive.after(Ticks::from_hours(72))
            && w.state.breachable_from(mother) == Some(protected),
        "72 h of protection from the landing (D-034)",
        errors,
    );
    check(
        w.state
            .fleet(kit)
            .is_some_and(|f| f.place == Place::Docked(mother)),
        "the start kit jumped with it and docked (D-166)",
        errors,
    );
    check(
        w.state.station_jumps().is_empty(),
        "no station is left in transit",
        errors,
    );
}

/// H2 held at day 1 19:00 with no attacker online, protected until day 3 19:00. On day 2 at
/// 10:00 A wears sector E to 0: no breach (D-159). On day 3 at 18:57 A attacks again; the
/// first hit after 19:00 breaches, with the exit at day 4 19:00.
fn protection(w: &mut World, errors: &mut Vec<String>) -> Result<(), String> {
    let h2 = w.holding("H2")?;
    let fleet = w.attackers(h2, 6_000.0, 10, 4)?;
    w.session(at(1, 6, 0, 0), at(1, 7, 0, 0));
    w.attack(at(1, 6, 28, 30), fleet, h2.sim_id());
    w.session(at(2, 10, 0, 0), at(2, 10, 20, 0));
    w.attack(at(2, 10, 0, 1), fleet, h2.sim_id());
    w.session(at(3, 18, 55, 0), at(3, 19, 30, 0));
    w.attack(at(3, 18, 57, 0), fleet, h2.sim_id());
    let mut worn = None;
    let breaches = |w: &World| {
        w.said
            .iter()
            .filter(
                |(_, k)| matches!(k, AnnouncementKind::Breached { holding, .. } if *holding == h2),
            )
            .count()
    };
    // Through the first breach and Held, to the attack during protection.
    w.run(at(2, 10, 0, 1), |_| false);
    let at_zero = |w: &World| {
        w.state
            .defence(h2)
            .is_some_and(|d| d.sectors[usize::from(EAST)].strength_milli == 0)
    };
    w.run(at(2, 10, 20, 0), at_zero);
    if at_zero(w) && w.state.breachable_from(h2) > Some(w.state.tick()) {
        worn = Some(w.state.tick());
        w.log.push(format!(
            "  {}  H2's sector E worn to 0 during protection: no breach (D-159)",
            when(w.state.tick())
        ));
    }
    w.run(at(3, 19, 30, 0), |w| breaches(w) >= 2);
    check(
        worn.is_some(),
        "sector E reached 0 during protection",
        errors,
    );
    let all: Vec<(Tick, Tick)> = w
        .said
        .iter()
        .filter_map(|(t, k)| match *k {
            AnnouncementKind::Breached {
                holding, exit_tick, ..
            } if holding == h2 => Some((*t, exit_tick)),
            _ => None,
        })
        .collect();
    check(all.len() == 2, "exactly two breaches", errors);
    if let [_, (second, exit)] = all.as_slice() {
        check(
            *second >= at(3, 19, 0, 0),
            "the second breach comes once protection ends",
            errors,
        );
        check(*exit == at(4, 19, 0, 0), "its exit is day 4 19:00", errors);
    }
    Ok(())
}

/// D's solar array under A's fire at 10:00 (disabled, outside the window) and again at 19:30
/// (destroyed, inside it; D-169).
fn peripheral(w: &mut World, errors: &mut Vec<String>) -> Result<(), String> {
    let mother = w.mother();
    let solar = w
        .state
        .holding_economy(mother)
        .and_then(|e| {
            e.slots
                .into_iter()
                .find_map(|(slot, m)| (slot == Slot::Solar).then_some(m?.id))
        })
        .ok_or("no solar array")?;
    let (system, centre) = w.centre(mother)?;
    let (_, position, _) = w.state.module_position(solar).ok_or("no position")?;
    let position = Vec2::from(position);
    let outward = (position - centre).normalized().ok_or("no direction")?;
    let fleet = place_fleet(
        &mut w.state,
        Owner::Commander(A),
        system,
        position + outward * 2_400.0,
        (outward * -1.0).angle(),
        &[ShipClass::Lancer; 2],
    );
    w.session(at(1, 10, 0, 0), at(1, 10, 30, 0));
    w.attack(at(1, 10, 0, 1), fleet, solar);
    w.session(at(1, 19, 25, 0), at(1, 19, 50, 0));
    w.attack(at(1, 19, 30, 0), fleet, solar);
    w.run(at(1, 19, 50, 0), |w| w.state.module(solar).is_none());
    let disabled = w.modules.iter().find_map(|&(t, e)| {
        matches!(e, EconomyEvent::ModuleDisabled { module } if module == solar).then_some(t)
    });
    let destroyed = w.modules.iter().find_map(|&(t, e)| {
        matches!(e, EconomyEvent::ModuleDestroyed { module, .. } if module == solar).then_some(t)
    });
    if let Some(t) = disabled {
        w.log
            .push(format!("  {}  the solar array is disabled", when(t)));
    }
    if let Some(t) = destroyed {
        w.log
            .push(format!("  {}  the solar array is destroyed", when(t)));
    }
    check(
        disabled.is_some_and(|t| !in_window(t)),
        "disabled outside the window (doc 12 §3)",
        errors,
    );
    check(
        destroyed.is_some_and(in_window),
        "destroyed inside the window (D-138, D-169)",
        errors,
    );
    Ok(())
}

/// Runs every scenario and writes the report. A failed check is an error.
pub fn run(out: &mut impl Write, run: &SiegeRun) -> Result<(), String> {
    let w = |out: &mut dyn Write, line: String| writeln!(out, "{line}").map_err(|e| e.to_string());
    w(
        out,
        format!(
            "siege | seed {} | D = C1 (human, window 19:00-23:00 UTC, offline), A = C2 (AI, \
             scripted sessions)",
            run.seed
        ),
    )?;
    let mut failed = Vec::new();
    for scenario in scenarios() {
        let started = Instant::now();
        let mut world = World::new(run.seed, &run.rules)?;
        let mut errors = Vec::new();
        (scenario.run)(&mut world, &mut errors)?;
        if !world.rejected.is_empty() {
            errors.push(format!("commands rejected: {:?}", world.rejected));
        }
        let problems = world.state.fleet_problems();
        if !problems.is_empty() {
            errors.push(format!("fleet problems: {problems:?}"));
        }
        let mut micros = std::mem::take(&mut world.micros);
        micros.sort_by(f64::total_cmp);
        let mean = micros.iter().sum::<f64>() / micros.len().max(1) as f64;
        let p99 = micros
            .get(micros.len().saturating_sub(1) * 99 / 100)
            .copied()
            .unwrap_or(0.0);
        let hash = state_hash(&world.state).map_err(|e| e.to_string())?;
        w(out, format!("{}:", scenario.name))?;
        for line in &world.log {
            w(out, line.clone())?;
        }
        let verdict = if errors.is_empty() {
            "checks passed".to_owned()
        } else {
            format!("FAILED: {}", errors.join("; "))
        };
        w(
            out,
            format!(
                "  {verdict} | until {} | tick mean {mean:.0} µs, p99 {p99:.0} µs | elapsed \
                 {:.1} s | hash {hash:016x}",
                when(world.state.tick()),
                started.elapsed().as_secs_f64()
            ),
        )?;
        if !errors.is_empty() {
            failed.push(scenario.name);
        }
    }
    if failed.is_empty() {
        Ok(())
    } else {
        Err(format!("siege scenarios failed: {failed:?}"))
    }
}

/// The names of the scenarios, for the help text and tests.
#[cfg(test)]
fn names() -> Vec<&'static str> {
    scenarios().iter().map(|s| s.name).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_scenario_names_its_rule_and_the_universe_sets_up() {
        assert_eq!(names().len(), 8);
        let rules = Rules::embedded();
        let mut world = World::new(1, &rules).unwrap();
        let h2 = world.holding("H2").unwrap();
        assert!(place_docked_fleet(&mut world.state, h2, &[ShipClass::Vx; 2]).is_some());
        // A frigate needs a dry dock (D-102), which H2 lacks.
        assert!(place_docked_fleet(&mut world.state, h2, &[ShipClass::Warden]).is_none());
        assert_eq!(when(at(2, 19, 16, 5)), "day 2 19:16:05");
        assert!(in_window(at(2, 19, 0, 0)) && !in_window(at(2, 23, 0, 0)));
    }

    #[test]
    #[ignore = "long simulation: eight sieges over about 230 simulated hours; CI runs fast tests only (D-190), run with --ignored"]
    fn every_siege_scenario_passes_its_checks() {
        let mut out = Vec::new();
        run(
            &mut out,
            &SiegeRun {
                seed: 1,
                rules: Rules::embedded(),
            },
        )
        .unwrap();
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text.matches("checks passed").count(), 8, "{text}");
    }

    #[test]
    fn a_failed_check_is_reported() {
        let mut errors = Vec::new();
        check(false, "a rule", &mut errors);
        assert_eq!(errors, ["a rule"]);
    }
}
