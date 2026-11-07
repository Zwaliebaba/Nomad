//! The stakes-rule property test (S6 plan §8): over at least 1,000 seeded breaches with random
//! windows, presence and fleets, every Fallen happens inside its owner's window and at least
//! 12 h after its breach, and no core is damaged outside a siege (D-015).
//!
//! Each seed generates a stage-1 Universe with random exposure windows and a random season
//! start time of day. The defender, commander 1, has its mother station and up to four more
//! holdings; each is attacked by a fleet of random size from another commander, in the
//! direction of a random sector, starting at a random time over the first two days. The
//! attackers' presence toggles at random, a catch-up sometimes covers an exit, and every
//! core starts its siege at a random hull. Combat is stepped tick by tick: during each
//! breach, for the first minutes of every siege, and on every deadline. The quiet hours
//! between are skipped ([`super::tests::skip_to`]), stepping every deadline on its own tick.
//!
//! Checked on every step:
//! - a core of a completed holding takes damage only while its siege runs: started at the
//!   latest on that tick, and not ended by Held before combat on it;
//! - a siege starts exactly when an attacker counts as online at the exit, else Held with no
//!   attacker online (D-160, D-047);
//! - every exit is a stagger slot of the owner's window, at least 12 h after its breach, and
//!   one window holds at most 4 exits, 60 min apart (D-033, D-158);
//! - every fall happens inside the owner's window, at least 12 h after its breach.

use std::collections::{BTreeMap, BTreeSet};

use nomad_types::{CommanderKind, FleetOrder, ModuleKind, Owner, QuarterHour, Queueing, Slot};

use super::DefenceState;
use super::tests::skip_to;
use crate::announce::{AnnouncementKind, HeldReason};
use crate::commander::{CommanderSetup, counts_as_online};
use crate::economy::{self, Hull};
use crate::fleet::ShipClass;
use crate::galaxy::{STAGE1_COMMANDERS, SiteId, generate_stage1_roster};
use crate::geometry::Vec2;
use crate::holding::HoldingId;
use crate::id::CommanderId;
use crate::input::{OperatorInput, TickInputs};
use crate::rng::{RngStreams, StreamKey};
use crate::rules::Rules;
use crate::state::{SeasonConfig, UniverseState};
use crate::step::{TickOutputs, step};
use crate::testkit::spawn_fleet;
use crate::time::{TICKS_PER_SECOND, Tick, Ticks};

const DAY_MS: i64 = 86_400_000;
const HOUR_MS: i64 = 3_600_000;
/// The test's own random stream: scenario choices, not the Universe's.
const SCRIPT: StreamKey = StreamKey {
    scope: 0,
    purpose: 0x5354_4b45, // "STKE"
};
/// Ticks of combat stepped at the start of each siege before skipping to its next deadline.
const SIEGE_TICKS: u64 = 1_200;

/// What one run saw.
#[derive(Default, Debug)]
struct Tally {
    breaches: u32,
    sieges: u32,
    held_clock: u32,
    held_absent: u32,
    falls: u32,
}

/// Per breached holding: when, whose, and the owner's window start then.
#[derive(Clone, Copy, Debug)]
struct Breach {
    tick: Tick,
    owner: CommanderId,
    window: QuarterHour,
}

struct Run {
    state: UniverseState,
    rng: RngStreams,
    /// Scripted operator inputs by tick: presence and catch-up.
    script: BTreeMap<Tick, Vec<OperatorInput>>,
    breaches: BTreeMap<HoldingId, Breach>,
    /// Exits announced per owner.
    exits: BTreeMap<CommanderId, BTreeSet<Tick>>,
    /// When each running siege started.
    sieges: BTreeMap<HoldingId, Tick>,
    /// The fleet attacking each target.
    fleets: BTreeMap<HoldingId, crate::fleet::FleetId>,
    tally: Tally,
}

impl Run {
    fn below(&mut self, bound: u64) -> u64 {
        self.rng.below(SCRIPT, bound)
    }

    fn ms(&self, tick: Tick) -> i64 {
        self.state.clock.universe_time_ms(tick)
    }

    /// Steps one tick with the scripted inputs due on it, checking every property.
    fn step(&mut self) {
        let now = self.state.tick.next();
        let inputs = TickInputs {
            operator: self.script.remove(&now).unwrap_or_default(),
            commanders: vec![],
        };
        // Who would count as online at an exit due now, after this tick's inputs.
        let due: Vec<(HoldingId, Vec<CommanderId>)> = self
            .state
            .sieges
            .holdings
            .iter()
            .filter_map(|(&h, r)| match &r.state {
                DefenceState::Reinforced {
                    exit, attackers, ..
                } if *exit == now => Some((h, attackers.clone())),
                _ => None,
            })
            .collect();
        let mut exits_due = BTreeMap::new();
        if !due.is_empty() {
            let mut probe = self.state.clone();
            for op in &inputs.operator {
                match *op {
                    OperatorInput::SetPresence { commander, online } => {
                        crate::commander::set_presence(&mut probe, commander, online);
                    }
                    OperatorInput::CatchUpStart => probe.catching_up = true,
                    OperatorInput::CatchUpEnd => probe.catching_up = false,
                    OperatorInput::SpawnDiagnostics { .. } => {}
                }
            }
            crate::commander::sync_presence(&mut probe, now);
            for (h, attackers) in due {
                let online = attackers.iter().any(|&a| counts_as_online(&probe, a, now));
                exits_due.insert(h, online);
            }
        }
        let in_siege: BTreeSet<HoldingId> = self.sieges.keys().copied().collect();
        let cores: BTreeMap<crate::id::SimId, HoldingId> = self
            .state
            .holdings
            .iter()
            .filter(|(_, h)| h.core_complete)
            .filter_map(|(&id, _)| {
                let core = self
                    .state
                    .economy
                    .holdings
                    .get(&id)?
                    .slots
                    .get(&Slot::Core)?;
                Some((*core, id))
            })
            .collect();
        let sites: BTreeMap<SiteId, HoldingId> = self
            .state
            .holdings
            .iter()
            .map(|(&id, h)| (h.site, id))
            .collect();

        let out = step(&mut self.state, &inputs);
        self.check(&out, now, &exits_due, &in_siege, &cores, &sites);
    }

    fn check(
        &mut self,
        out: &TickOutputs,
        now: Tick,
        exits_due: &BTreeMap<HoldingId, bool>,
        in_siege: &BTreeSet<HoldingId>,
        cores: &BTreeMap<crate::id::SimId, HoldingId>,
        sites: &BTreeMap<SiteId, HoldingId>,
    ) {
        let mut started = BTreeSet::new();
        let mut held = BTreeSet::new();
        let mut fallen = Vec::new();
        for a in &out.announcements {
            match a.kind {
                AnnouncementKind::Breached {
                    holding, exit_tick, ..
                } => {
                    let owner = self.state.holdings[&holding].owner;
                    let window = self.state.exposure_window(owner).unwrap().start;
                    self.breaches.insert(
                        holding,
                        Breach {
                            tick: now,
                            owner,
                            window,
                        },
                    );
                    self.check_exit(holding, exit_tick);
                    self.tally.breaches += 1;
                }
                AnnouncementKind::SiegeStarted { holding, .. } => {
                    assert_eq!(
                        exits_due.get(&holding),
                        Some(&true),
                        "{holding:?} at {now:?}"
                    );
                    started.insert(holding);
                    self.sieges.insert(holding, now);
                    self.tally.sieges += 1;
                    // The core starts its siege at a random hull, so some fall and some hold.
                    if let Some(&core) =
                        self.state.economy.holdings[&holding].slots.get(&Slot::Core)
                    {
                        let hp = 500 + u32::try_from(self.below(15_000)).unwrap();
                        self.state.entities.get_mut::<Hull>(core).unwrap().hp = hp;
                        self.renew_attack(holding, core);
                    }
                }
                AnnouncementKind::Held {
                    holding, reason, ..
                } => {
                    held.insert(holding);
                    self.sieges.remove(&holding);
                    match reason {
                        HeldReason::Clock => self.tally.held_clock += 1,
                        HeldReason::NoAttackerOnline => {
                            assert_eq!(exits_due.get(&holding), Some(&false), "{holding:?}");
                            self.tally.held_absent += 1;
                        }
                    }
                }
                AnnouncementKind::Captured { holding, .. } => fallen.push(holding),
                AnnouncementKind::MotherStationFell { site, .. } => fallen.push(sites[&site]),
                _ => {}
            }
        }
        for holding in fallen {
            self.check_fall(holding, now);
            self.sieges.remove(&holding);
            self.tally.falls += 1;
        }
        // No core of a completed holding is damaged outside a siege (D-015, doc 15 §6).
        let allowed: BTreeSet<HoldingId> = in_siege
            .union(&started)
            .copied()
            .filter(|h| !held.contains(h))
            .collect();
        for event in &out.defence_events {
            if let crate::DefenceEvent::ModuleDamaged {
                holding, module, ..
            } = *event
                && cores.get(&module) == Some(&holding)
            {
                assert!(
                    allowed.contains(&holding),
                    "the core of {holding:?} was damaged outside a siege on {now:?}"
                );
            }
        }
    }

    /// The attacker renews its assault on the core when the siege starts: its orders lapsed
    /// if it was offline for longer than the grace since (D-162). An order is given only if
    /// the validator accepts it (D-077).
    fn renew_attack(&mut self, holding: HoldingId, core: crate::id::SimId) {
        let Some(&fleet) = self.fleets.get(&holding) else {
            return;
        };
        let Some(owner) = self.state.fleet(fleet).map(|f| f.owner) else {
            return;
        };
        let command = nomad_types::Command::Fleet {
            fleet,
            order: FleetOrder::Attack { target: core },
            queue: Queueing::Replace,
        };
        if crate::validate_command(&self.state, owner, &command).is_ok() {
            crate::fleet::orders::give(
                &mut self.state,
                fleet,
                FleetOrder::Attack { target: core },
                Queueing::Replace,
                &mut Vec::new(),
            );
        }
    }

    /// An exit is a stagger slot of the owner's window as it was at the breach, at least
    /// 12 h after it, and its window holds at most 4 exits 60 min apart (D-033, D-158).
    fn check_exit(&mut self, holding: HoldingId, exit: Tick) {
        let breach = self.breaches[&holding];
        assert!(
            exit >= breach.tick.after(Ticks::from_hours(12)),
            "{holding:?}"
        );
        let into = (self.ms(exit) - breach.window.time_of_day_ms()).rem_euclid(DAY_MS);
        let slot = into / HOUR_MS;
        assert!(slot < 4, "{holding:?}: exit {into} ms into the window");
        assert!(
            into % HOUR_MS < 100,
            "{holding:?}: exit {into} ms into the window"
        );
        let exits = self.exits.entry(breach.owner).or_default();
        assert!(exits.insert(exit), "two exits on one tick");
        let window_of = |t: Tick, clock: crate::time::SeasonClock| {
            (clock.universe_time_ms(t) - breach.window.time_of_day_ms()).div_euclid(DAY_MS)
        };
        let clock = self.state.clock;
        let same: Vec<Tick> = exits
            .iter()
            .copied()
            .filter(|&t| window_of(t, clock) == window_of(exit, clock))
            .collect();
        assert!(same.len() <= 4, "{same:?}");
        for pair in same.windows(2) {
            assert!(
                pair[1].since(pair[0]) >= Ticks::from_minutes(60),
                "{same:?}"
            );
        }
    }

    /// A fall happens inside the owner's window, at least 12 h after the breach (D-015).
    fn check_fall(&self, holding: HoldingId, now: Tick) {
        let breach = self.breaches[&holding];
        assert!(
            now >= breach.tick.after(Ticks::from_hours(12)),
            "{holding:?}"
        );
        let into = (self.ms(now) - breach.window.time_of_day_ms()).rem_euclid(DAY_MS);
        assert!(
            into < 4 * HOUR_MS,
            "{holding:?} fell {into} ms into its window"
        );
        // The window is frozen while the defence is pending, so the record agrees (D-156).
        let window = self.state.exposure_window(breach.owner).unwrap();
        assert_eq!(window.start, breach.window);
        assert!(window.is_open(now));
    }

    /// The next tick something happens: any deadline or a scripted input. Every one is
    /// stepped here, so every step is checked.
    fn next_event(&self) -> Option<Tick> {
        let now = self.state.tick;
        [
            self.state.sieges.deadlines.first().map(|&(t, _)| t),
            self.state.commanders.deadlines.next(),
            self.state.transits.pending().map(|(t, _)| t).min(),
            self.script.keys().next().copied(),
        ]
        .into_iter()
        .flatten()
        .filter(|&t| t > now)
        .min()
    }
}

/// One seeded Universe: up to five breaches of commander 1's holdings and every siege that
/// follows, to the end of the last.
fn run_seed(seed: u64, rules: &Rules) -> Tally {
    let mut rng = RngStreams::new(seed);
    let quarter = |rng: &mut RngStreams| {
        QuarterHour::new(u8::try_from(rng.below(SCRIPT, 96)).unwrap()).unwrap()
    };
    let roster: [CommanderSetup; STAGE1_COMMANDERS] = std::array::from_fn(|i| CommanderSetup {
        id: CommanderId(u32::try_from(i).unwrap() + 1),
        kind: CommanderKind::Ai,
        window_start: quarter(&mut rng),
    });
    let start_ms = i64::try_from(rng.below(SCRIPT, 86_400_000)).unwrap();
    let state =
        generate_stage1_roster(SeasonConfig::new(seed, start_ms), roster, rules.clone()).unwrap();
    let mut run = Run {
        state,
        rng,
        script: BTreeMap::new(),
        breaches: BTreeMap::new(),
        exits: BTreeMap::new(),
        sieges: BTreeMap::new(),
        fleets: BTreeMap::new(),
        tally: Tally::default(),
    };
    let defender = CommanderId(1);
    // The defender's holdings: the mother station and up to four in its home system.
    let (mother, system) = crate::testkit::home(&run.state, 1);
    let mut targets = vec![mother];
    let extra = run.below(5);
    for _ in 0..extra {
        let Some(site) = run
            .state
            .galaxy()
            .system(system)
            .unwrap()
            .sites
            .iter()
            .copied()
            .find(|&s| run.state.holding_at(s).is_none())
        else {
            break;
        };
        let h = run.state.insert_holding(site, defender, true).unwrap();
        economy::spawn_module(&mut run.state, h, Slot::Core, ModuleKind::Core, None);
        targets.push(h);
    }
    let horizon = Tick::new(7 * 864_000);
    script_presence(&mut run, horizon);
    run.step();
    // The attacks: each target at a random time in the first two days, from a random side.
    let mut starts: Vec<(Tick, HoldingId)> = targets
        .iter()
        .map(|&h| (Tick::new(100 + run.below(2 * 864_000)), h))
        .collect();
    starts.sort_unstable();
    for (when, holding) in starts {
        advance_to(&mut run, when);
        attack(&mut run, holding);
    }
    // Every siege to its end: the horizon is a week.
    while let Some(next) = run.next_event() {
        if next > horizon {
            break;
        }
        advance_to(&mut run, next);
        run.step();
        // The first minutes of every siege, tick by tick.
        while run
            .sieges
            .values()
            .any(|&from| run.state.tick.since(from).get() < SIEGE_TICKS)
        {
            run.step();
        }
    }
    run.tally
}

/// Random presence for the attackers until `horizon`, and sometimes a catch-up.
fn script_presence(run: &mut Run, horizon: Tick) {
    for n in 2..=4 {
        let mut at = Tick::new(1 + run.below(36_000));
        let mut online = run.below(2) == 0;
        while at < horizon {
            run.script
                .entry(at)
                .or_default()
                .push(OperatorInput::SetPresence {
                    commander: CommanderId(n),
                    online,
                });
            online = !online;
            // From a minute to ten hours, so some changes fall inside a grace at an exit.
            let gap = 600 + run.below(360_000);
            at = at.after(Ticks::new(gap));
        }
    }
    if run.below(3) == 0 {
        let from = Tick::new(864_000 + run.below(3 * 864_000));
        let to = from.after(Ticks::new(36_000 + run.below(10 * 36_000)));
        run.script
            .entry(from)
            .or_default()
            .push(OperatorInput::CatchUpStart);
        run.script
            .entry(to)
            .or_default()
            .push(OperatorInput::CatchUpEnd);
    }
}

/// Skips to just before `tick`, stepping every deadline and scripted input on the way.
fn advance_to(run: &mut Run, tick: Tick) {
    while let Some(next) = run.next_event() {
        if next >= tick {
            break;
        }
        skip_to(&mut run.state, next);
        run.step();
    }
    if tick > run.state.tick.next() {
        skip_to(&mut run.state, tick);
    }
}

/// A fleet of random size attacks `holding` from the direction of a random sector, which is
/// weakened so that it breaches within seconds; stepped until it does, or a while.
fn attack(run: &mut Run, holding: HoldingId) {
    if !super::breachable(&run.state, holding, run.state.tick.next()) {
        return;
    }
    let Some(h) = run.state.holding(holding).cloned() else {
        return;
    };
    let site = run.state.galaxy().site(h.site).unwrap();
    let (system, centre) = (site.system, Vec2::from(site.position));
    let sector = u8::try_from(run.below(4)).unwrap();
    let degrees = [90.0_f64, 0.0, -90.0, 180.0][usize::from(sector)];
    let at = centre + Vec2::from_angle(degrees.to_radians()) * 3_500.0;
    let attacker = 2 + u32::try_from(run.below(3)).unwrap();
    let mut classes = vec![ShipClass::Lancer; 2 + usize::try_from(run.below(6)).unwrap()];
    classes.extend(vec![
        ShipClass::Warden;
        usize::try_from(run.below(3)).unwrap()
    ]);
    let fleet = spawn_fleet(
        &mut run.state,
        Owner::Commander(CommanderId(attacker)),
        system,
        at,
        &classes,
    );
    crate::fleet::orders::give(
        &mut run.state,
        fleet,
        FleetOrder::Attack {
            target: holding.sim_id(),
        },
        Queueing::Replace,
        &mut Vec::new(),
    );
    run.fleets.insert(holding, fleet);
    // A dome that has not come up yet does so in the holding's economy slot.
    for _ in 0..TICKS_PER_SECOND {
        run.step();
    }
    if let Some(d) = run.state.defence.holdings.get_mut(&holding) {
        d.sectors[usize::from(sector)].strength = 1_000;
    }
    for _ in 0..600 {
        run.step();
        if !super::breachable(&run.state, holding, run.state.tick.next()) {
            break;
        }
    }
}

fn stakes_rule(seeds: std::ops::Range<u64>) -> Tally {
    let rules = Rules::embedded();
    let mut total = Tally::default();
    for seed in seeds {
        let t = run_seed(seed, &rules);
        total.breaches += t.breaches;
        total.sieges += t.sieges;
        total.held_clock += t.held_clock;
        total.held_absent += t.held_absent;
        total.falls += t.falls;
    }
    total
}

#[test]
fn the_stakes_rule_holds_for_a_few_seeded_universes() {
    let tally = stakes_rule(0..2);
    assert!(tally.breaches >= 2 && tally.sieges >= 1, "{tally:?}");
}

#[test]
#[ignore = "long simulation: 1,000 seeded breaches and their sieges; CI runs fast tests only (D-190), run with --ignored"]
fn the_stakes_rule_holds_over_a_thousand_seeded_breaches() {
    let mut seeds = 0;
    let mut total = Tally::default();
    while total.breaches < 1_000 {
        let t = stakes_rule(seeds..seeds + 1);
        seeds += 1;
        total.breaches += t.breaches;
        total.sieges += t.sieges;
        total.held_clock += t.held_clock;
        total.held_absent += t.held_absent;
        total.falls += t.falls;
    }
    // Every outcome happened, so the property was tested on each.
    assert!(
        total.falls > 0 && total.held_clock > 0 && total.held_absent > 0,
        "{total:?}"
    );
    println!("{seeds} seeds: {total:?}");
}
