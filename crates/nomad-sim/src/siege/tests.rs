//! Tests of the siege lifecycle against S6 plan §8 "Done when" (S6b).
//!
//! The Reinforced wait is 12 h or more of Universe time. Nothing in it depends on ticks that
//! pass without inputs, so the tests skip it: [`skip_to`] moves the tick counter to just
//! before a deadline and lets the next step fire it, as the step would after every tick
//! between. Combat runs tick by tick.

use nomad_types::{
    BuildItem, Command, EconomyCommand, FleetOrder, ModuleKind, Owner, Queueing, ShipClass, Slot,
};

use super::{DefenceState, StationLeg};
use crate::announce::{AnnouncementKind, HeldReason, Recipients};
use crate::command::CommandRejection as R;
use crate::defence::DefenceEvent;
use crate::economy::{self, Hull};
use crate::fleet::{Fleet, FleetId, Place};
use crate::galaxy::{SiteId, SystemId};
use crate::geometry::Vec2;
use crate::holding::HoldingId;
use crate::id::{CommanderId, SimId};
use crate::input::{OperatorInput, TickInputs};
use crate::snapshot::{decode, encode, state_hash};
use crate::state::UniverseState;
use crate::step::{TickOutputs, step};
use crate::testkit::{commander, home, idle, kit_fleet, order, send, spawn_fleet, stage1};
use crate::time::{TICKS_PER_SECOND, Tick, Ticks};
use crate::validate_command;

const LANCER: ShipClass = ShipClass::Lancer;
const MILLI: u64 = 1_000;
const NORTH: u8 = 0;
const DEFENDER: u32 = 1;
const ATTACKER: u32 = 2;

/// Tick of day `day` at `hours:minutes` UTC: the season starts at the epoch's midnight.
fn t(day: u64, hours: u64, minutes: u64) -> Tick {
    Tick::new(((day * 24 + hours) * 60 + minutes) * 60 * TICKS_PER_SECOND)
}

/// The next step computes `tick`. Nothing in the Reinforced wait depends on the ticks
/// skipped: every deadline that falls due meanwhile (window closes, grace ends, siege and
/// jump deadlines, transits) still fires on its own tick, stepped on the way.
pub(crate) fn skip_to(state: &mut UniverseState, tick: Tick) {
    assert!(tick > state.tick, "skips forward only");
    loop {
        let next = [
            state.commanders.deadlines.next(),
            state.sieges.deadlines.first().map(|&(t, _)| t),
            state.transits.pending().map(|(t, _)| t).min(),
        ]
        .into_iter()
        .flatten()
        .filter(|&t| t > state.tick)
        .min();
        match next {
            Some(due) if due < tick => {
                state.tick = Tick::new(due.get() - 1);
                step(state, &TickInputs::default());
            }
            _ => break,
        }
    }
    state.tick = Tick::new(tick.get() - 1);
}

fn operator(state: &mut UniverseState, ops: Vec<OperatorInput>) -> TickOutputs {
    step(
        state,
        &TickInputs {
            operator: ops,
            commanders: vec![],
        },
    )
}

/// AI commander `n`'s schedule has it online or not (S6a's harness input until A5).
fn presence(state: &mut UniverseState, n: u32, online: bool) -> TickOutputs {
    let out = operator(
        state,
        vec![OperatorInput::SetPresence {
            commander: CommanderId(n),
            online,
        }],
    );
    assert_eq!(out.inputs_rejected, 0);
    out
}

fn announcements(outputs: &[TickOutputs]) -> Vec<(Tick, AnnouncementKind, Recipients)> {
    outputs
        .iter()
        .flat_map(|o| &o.announcements)
        .map(|a| (a.tick, a.kind.clone(), a.to.clone()))
        .collect()
}

/// A stage-1 Universe whose commanders are AI (window 19:00–23:00 UTC), with the defender's
/// target holding and an attacking fleet 3.5 km north of it, ordered to attack the dome.
struct Scene {
    state: UniverseState,
    target: HoldingId,
    fleet: FleetId,
}

impl Scene {
    /// The defender's mother station as the target.
    fn mother() -> Self {
        let state = stage1(7);
        let (target, system) = home(&state, DEFENDER);
        Self::around(state, target, system)
    }

    /// A completed holding of the defender on a free site of its home system as the target,
    /// with a fleet of 2 VX docked there.
    fn remote() -> Self {
        let mut state = stage1(7);
        let (_, system) = home(&state, DEFENDER);
        let site = free_site(&state, system);
        let target = place_holding(&mut state, site, DEFENDER);
        let docked = spawn_fleet(
            &mut state,
            commander(DEFENDER),
            system,
            Vec2::ZERO,
            &[ShipClass::Vx; 2],
        );
        state
            .entities
            .get_mut::<Fleet>(docked.sim_id())
            .unwrap()
            .place = Place::Docked(target);
        Self::around(state, target, system)
    }

    fn around(mut state: UniverseState, target: HoldingId, system: SystemId) -> Self {
        let site = state.holding(target).unwrap().site;
        let centre = Vec2::from(state.galaxy().site(site).unwrap().position);
        // The dome comes up in the holding's first economy slot.
        idle(&mut state, TICKS_PER_SECOND);
        let fleet = spawn_fleet(
            &mut state,
            commander(ATTACKER),
            system,
            centre + Vec2::new(0.0, 3_500.0),
            &[LANCER; 6],
        );
        Self {
            state,
            target,
            fleet,
        }
    }

    fn view(&self) -> super::SiegeView {
        self.state.siege(self.target).unwrap()
    }

    fn set_strength(&mut self, sector: u8, milli: u64) {
        let d = self.state.defence.holdings.get_mut(&self.target).unwrap();
        d.sectors[usize::from(sector)].strength = milli;
    }

    fn core(&self) -> SimId {
        self.state.economy.holdings[&self.target].slots[&Slot::Core]
    }

    fn attack(&mut self, target: SimId) {
        let out = send(
            &mut self.state,
            ATTACKER,
            order(self.fleet, FleetOrder::Attack { target }),
        );
        assert_eq!(out.commands[0].outcome, Ok(()), "{:?}", out.commands);
    }

    /// Skips to `at`, weakens the north sector to one point and attacks the dome until it
    /// breaches. Returns the outputs up to the breach.
    fn breach_at(&mut self, at: Tick) -> Vec<TickOutputs> {
        skip_to(&mut self.state, at);
        self.set_strength(NORTH, MILLI);
        self.attack(self.target.sim_id());
        self.run_until(600, |o| {
            o.announcements
                .iter()
                .any(|a| matches!(a.kind, AnnouncementKind::Breached { .. }))
        })
    }

    /// Steps until `done` holds for a tick's outputs, at most `cap` ticks.
    fn run_until(&mut self, cap: u64, done: impl Fn(&TickOutputs) -> bool) -> Vec<TickOutputs> {
        let mut outputs = Vec::new();
        for _ in 0..cap {
            let out = step(&mut self.state, &TickInputs::default());
            let stop = done(&out);
            outputs.push(out);
            if stop {
                return outputs;
            }
        }
        panic!("not reached in {cap} ticks");
    }

    /// The exit of the pending Reinforced state.
    fn exit(&self) -> Tick {
        match self.view().state {
            DefenceState::Reinforced { exit, .. } => exit,
            other => panic!("not Reinforced: {other:?}"),
        }
    }
}

/// A free site of `system`.
fn free_site(state: &UniverseState, system: SystemId) -> SiteId {
    state
        .galaxy()
        .system(system)
        .unwrap()
        .sites
        .iter()
        .copied()
        .find(|&s| state.holding_at(s).is_none())
        .expect("a free site")
}

/// A completed holding of commander `n` on `site`, with its core and no pylons (a fixture,
/// not a claim).
fn place_holding(state: &mut UniverseState, site: SiteId, n: u32) -> HoldingId {
    let holding = state.insert_holding(site, CommanderId(n), true).unwrap();
    economy::spawn_module(state, holding, Slot::Core, ModuleKind::Core, None);
    holding
}

#[test]
fn the_exit_is_the_earliest_slot_twelve_hours_on_and_breaches_stagger() {
    // D-158, D-033 and the S6 plan §5 timeline, with the default window 19:00–23:00 UTC.
    let mut state = stage1(7);
    let owner = CommanderId(DEFENDER);
    let exit = |state: &UniverseState, breach| super::lifecycle::exit_slot(state, owner, breach);
    // Day 1 14:20: day 1 19:00 is 4 h 40 min away, so day 2 19:00.
    assert_eq!(exit(&state, t(1, 14, 20)), t(2, 19, 0));
    // 12 h exactly is enough; a breach inside the window waits for a later one.
    assert_eq!(exit(&state, t(1, 7, 0)), t(1, 19, 0));
    assert_eq!(exit(&state, t(1, 20, 0)), t(2, 19, 0));
    // 11.5 h before the window: slot 1 gives 12.5 h, and no day is lost (D-158).
    assert_eq!(exit(&state, t(1, 7, 30)), t(1, 20, 0));
    assert_eq!(
        exit(&state, t(1, 10, 1)),
        t(1, 23, 0).after(Ticks::from_hours(20))
    );
    // 12 h exactly before slot 3: 22:00 the same day (D-158).
    assert_eq!(exit(&state, t(1, 10, 0)), t(1, 22, 0));
    // H2 to H6 breached after 10:00 and before 11:00 on day 1: day 2 at 19:00, 20:00, 21:00
    // and 22:00, and the fifth rolls over to day 3 19:00 (S6 plan §5).
    let (_, system) = home(&state, DEFENDER);
    let mut holdings = Vec::new();
    for _ in 0..5 {
        let site = free_site(&state, system);
        holdings.push(place_holding(&mut state, site, DEFENDER));
    }
    idle(&mut state, TICKS_PER_SECOND);
    let mut exits = Vec::new();
    for (i, &holding) in (0_u64..).zip(&holdings) {
        let breach = t(1, 10, 5 + 10 * i);
        state.tick = breach;
        let mut out = TickOutputs::default();
        super::lifecycle::breach(&mut state, breach, holding, NORTH, &mut out);
        exits.push(state.siege(holding).unwrap().state);
    }
    let exits: Vec<Tick> = exits
        .into_iter()
        .map(|s| match s {
            DefenceState::Reinforced { exit, .. } => exit,
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        exits,
        [
            t(2, 19, 0),
            t(2, 20, 0),
            t(2, 21, 0),
            t(2, 22, 0),
            t(3, 19, 0)
        ]
    );
    // While they are pending, the owner's window is frozen (D-156).
    assert!(super::defence_pending(&state, owner));
    assert!(!super::defence_pending(&state, CommanderId(ATTACKER)));
}

#[test]
fn a_breach_makes_the_holding_reinforced_and_immune_with_its_exit_announced() {
    let mut s = Scene::mother();
    let out = s.breach_at(t(1, 14, 20));
    let breach_tick = out.last().unwrap().tick;
    let view = s.view();
    let DefenceState::Reinforced {
        breached,
        sector,
        exit,
        attackers,
    } = view.state
    else {
        panic!("{:?}", view.state);
    };
    assert_eq!((breached, sector), (breach_tick, NORTH));
    assert_eq!(exit, t(2, 19, 0));
    assert_eq!(attackers, [CommanderId(ATTACKER)]);
    assert!(s.state.defence(s.target).unwrap().immune);
    let (_, kind, to) = announcements(&out)
        .into_iter()
        .find(|(_, k, _)| matches!(k, AnnouncementKind::Breached { .. }))
        .unwrap();
    assert_eq!(
        to,
        Recipients::Commanders(vec![CommanderId(DEFENDER), CommanderId(ATTACKER)])
    );
    assert_eq!(
        kind,
        AnnouncementKind::Breached {
            holding: s.target,
            sector: NORTH,
            exit_tick: exit,
            attackers: vec![CommanderId(ATTACKER)],
        }
    );
    // Reinforced: the dome takes no damage, and nothing reaches the core.
    let out = idle(&mut s.state, 300);
    assert!(
        !out.iter()
            .flat_map(|o| &o.defence_events)
            .any(|e| matches!(e, DefenceEvent::SectorDamaged { .. }))
    );
    assert_eq!(s.state.module(s.core()).unwrap().hp, 600_000);
    // A pending defence freezes the window (D-156), and the holding is not breachable.
    assert!(super::defence_pending(&s.state, CommanderId(DEFENDER)));
    assert!(!super::breachable(&s.state, s.target, s.state.tick()));
}

#[test]
fn the_siege_starts_at_the_exit_with_an_attacker_online_and_holds_by_the_clock() {
    let mut s = Scene::mother();
    s.breach_at(t(1, 14, 20));
    let exit = s.exit();
    presence(&mut s.state, ATTACKER, true);
    skip_to(&mut s.state, exit);
    let out = step(&mut s.state, &TickInputs::default());
    let ends = exit.after(Ticks::from_minutes(60));
    assert!(
        announcements(std::slice::from_ref(&out))
            .iter()
            .any(|(_, k, _)| *k
                == AnnouncementKind::SiegeStarted {
                    holding: s.target,
                    end_tick: ends,
                    open_sector: NORTH,
                })
    );
    let view = s.state.defence(s.target).unwrap();
    assert!(!view.immune);
    assert!(view.sectors[usize::from(NORTH)].open);
    assert_eq!(view.sectors.iter().filter(|s| s.open).count(), 1);
    assert!(matches!(s.view().state, DefenceState::Siege { ends: e, .. } if e == ends));
    // The attacker goes offline: the clock runs on (D-161).
    presence(&mut s.state, ATTACKER, false);
    // The clock runs out with the core standing: Held, and 48 h of protection (D-015).
    s.set_strength(1, 10 * MILLI);
    skip_to(&mut s.state, ends);
    let out = step(&mut s.state, &TickInputs::default());
    let held = announcements(std::slice::from_ref(&out));
    assert!(held.iter().any(|(_, k, _)| *k
        == AnnouncementKind::Held {
            holding: s.target,
            reason: HeldReason::Clock,
            breachable_from: ends.after(Ticks::from_hours(48)),
        }));
    let view = s.state.defence(s.target).unwrap();
    assert!(!view.immune && view.sectors.iter().all(|s| !s.open));
    // Every sector is at least half full; a full one stays full (D-057).
    assert!(
        view.sectors
            .iter()
            .all(|s| s.strength_milli * 2 >= s.max_milli)
    );
    // The weak one is raised to half, plus at most the tick's regeneration after it.
    let half = view.sectors[1].max_milli / 2;
    assert!((half..=half + 50 * MILLI).contains(&view.sectors[1].strength_milli));
    assert_eq!(view.sectors[2].strength_milli, view.sectors[2].max_milli);
    assert_eq!(s.view().state, DefenceState::Nominal);
    assert_eq!(s.view().breachable_from, ends.after(Ticks::from_hours(48)));
    assert!(!super::defence_pending(&s.state, CommanderId(DEFENDER)));
}

#[test]
fn with_no_attacker_online_at_the_exit_it_is_held_at_once_also_through_a_catch_up() {
    for catch_up in [false, true] {
        let mut s = Scene::mother();
        s.breach_at(t(1, 14, 20));
        let exit = s.exit();
        if catch_up {
            // The attacker's schedule says online, but nobody is online in catch-up (D-047).
            presence(&mut s.state, ATTACKER, true);
            operator(&mut s.state, vec![OperatorInput::CatchUpStart]);
        }
        skip_to(&mut s.state, exit);
        let out = step(&mut s.state, &TickInputs::default());
        let held = announcements(std::slice::from_ref(&out));
        assert!(
            held.iter().any(|(_, k, _)| *k
                == AnnouncementKind::Held {
                    holding: s.target,
                    reason: HeldReason::NoAttackerOnline,
                    breachable_from: exit.after(Ticks::from_hours(48)),
                }),
            "catch-up {catch_up}: {held:?}"
        );
        assert!(!s.state.defence(s.target).unwrap().immune);
        assert_eq!(s.view().state, DefenceState::Nominal);
    }
}

#[test]
fn an_attacker_inside_the_disconnect_grace_counts_as_online_at_the_exit() {
    // D-160: a network blip at the start does not hand the defender a free Held.
    let mut s = Scene::mother();
    s.breach_at(t(1, 14, 20));
    let exit = s.exit();
    presence(&mut s.state, ATTACKER, true);
    skip_to(
        &mut s.state,
        Tick::new(exit.get() - Ticks::from_secs(60).get()),
    );
    presence(&mut s.state, ATTACKER, false);
    skip_to(&mut s.state, exit);
    let out = step(&mut s.state, &TickInputs::default());
    assert!(
        announcements(std::slice::from_ref(&out))
            .iter()
            .any(|(_, k, _)| matches!(k, AnnouncementKind::SiegeStarted { .. }))
    );
}

#[test]
fn during_protection_a_sector_reaches_zero_without_a_breach_and_the_next_hit_after_breaches() {
    // D-159.
    let mut s = Scene::mother();
    s.breach_at(t(1, 14, 20));
    let exit = s.exit();
    skip_to(&mut s.state, exit);
    step(&mut s.state, &TickInputs::default());
    let protected_until = s.view().breachable_from;
    assert_eq!(protected_until, exit.after(Ticks::from_hours(48)));
    // Worn to 0 during protection: it blocks, takes hits, and no breach happens.
    s.set_strength(NORTH, MILLI);
    s.attack(s.target.sim_id());
    let out = idle(&mut s.state, 400);
    let events: Vec<DefenceEvent> = out.iter().flat_map(|o| o.defence_events.clone()).collect();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, DefenceEvent::SectorDamaged { sector: NORTH, .. }))
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, DefenceEvent::SectorBreached { .. }))
    );
    assert_eq!(s.view().state, DefenceState::Nominal);
    // Once protection ends, the next hostile hit on the sector at 0 is a breach.
    skip_to(&mut s.state, protected_until);
    s.set_strength(NORTH, 0);
    let out = s.run_until(200, |o| {
        o.defence_events
            .iter()
            .any(|e| matches!(e, DefenceEvent::SectorBreached { .. }))
    });
    assert!(out.last().unwrap().tick >= protected_until);
    assert!(matches!(
        s.view().state,
        DefenceState::Reinforced { attackers, .. } if attackers == [CommanderId(ATTACKER)]
    ));
}

/// The siege of `s`'s target, started with the attacker online, the core down to `hull`
/// and the attackers firing at it through the open north sector. Returns the outputs up to
/// the core going down.
fn fall(s: &mut Scene, hull: u32) -> Vec<TickOutputs> {
    s.breach_at(t(1, 14, 20));
    let exit = s.exit();
    presence(&mut s.state, ATTACKER, true);
    skip_to(&mut s.state, exit);
    step(&mut s.state, &TickInputs::default());
    let core = s.core();
    s.state.entities.get_mut::<Hull>(core).unwrap().hp = hull;
    s.attack(core);
    s.run_until(900, |o| {
        o.defence_events
            .iter()
            .any(|e| matches!(e, DefenceEvent::CoreDown { .. }))
    })
}

/// The defender's remote holding, with 2 VX docked and a turret being built, falls to the
/// attacker. Returns the scene, the docked fleet, the outputs up to the fall and the
/// command caps of defender and attacker before it.
fn capture_scene() -> (Scene, FleetId, Vec<TickOutputs>, (u32, u32)) {
    let mut s = Scene::remote();
    let docked = s
        .state
        .fleet_ids()
        .into_iter()
        .find(|&f| s.state.fleet(f).unwrap().place == Place::Docked(s.target))
        .unwrap();
    // A turret being built is cancelled without refund (D-168).
    let pool = s
        .state
        .economy
        .pools
        .entry(CommanderId(DEFENDER))
        .or_default();
    *pool = economy::Stock::units(50_000, 5_000);
    let out = send(
        &mut s.state,
        DEFENDER,
        Command::Economy(EconomyCommand::Build {
            holding: s.target,
            item: BuildItem::Module {
                kind: ModuleKind::Turret,
                slot: Slot::Inner(0),
            },
        }),
    );
    assert_eq!(out.commands[0].outcome, Ok(()));
    idle(&mut s.state, 2 * TICKS_PER_SECOND);
    assert!(
        s.state
            .holding_economy(s.target)
            .unwrap()
            .queue
            .iter()
            .any(|q| q.building.is_some())
    );
    let caps = (
        s.state.command_cap(CommanderId(DEFENDER)),
        s.state.command_cap(CommanderId(ATTACKER)),
    );
    let out = fall(&mut s, 400);
    (s, docked, out, caps)
}

#[test]
fn a_fallen_holding_is_captured_by_the_attacker() {
    let (s, _, out, caps) = capture_scene();
    let fell = out.last().unwrap().tick;
    // The attacker captures (D-057): owner, caps, core 25%, queue, protection.
    let h = s.state.holding(s.target).unwrap();
    assert_eq!(h.owner, CommanderId(ATTACKER));
    assert_eq!(
        (
            s.state.command_cap(CommanderId(DEFENDER)),
            s.state.command_cap(CommanderId(ATTACKER))
        ),
        (caps.0 - 10, caps.1 + 10)
    );
    assert_eq!(s.state.module(s.core()).unwrap().hp, 75_000);
    assert!(s.state.holding_economy(s.target).unwrap().queue.is_empty());
    assert!(
        !s.state.economy.holdings[&s.target]
            .slots
            .contains_key(&Slot::Inner(0))
    );
    let view = s.view();
    assert_eq!(view.state, DefenceState::Nominal);
    assert_eq!(view.breachable_from, fell.after(Ticks::from_hours(48)));
    let defence = s.state.defence(s.target).unwrap();
    assert!(!defence.immune && defence.sectors.iter().all(|s| !s.open));
    assert!(
        defence
            .sectors
            .iter()
            .all(|s| s.strength_milli * 4 >= s.max_milli)
    );
    let said = announcements(&out);
    assert!(said.iter().any(|(_, k, to)| *to == Recipients::Public
        && *k
            == AnnouncementKind::Captured {
                holding: s.target,
                from: CommanderId(DEFENDER),
                to: CommanderId(ATTACKER),
                breachable_from: fell.after(Ticks::from_hours(48)),
            }));
}

#[test]
fn after_a_capture_the_old_owners_docked_ships_jump_to_their_nearest_holding() {
    // D-057, D-166: untargetable, taking no orders, and docked on arrival.
    let (mut s, docked, out, _) = capture_scene();
    let fell = out.last().unwrap().tick;
    let said = announcements(&out);
    let (mother, _) = home(&s.state, DEFENDER);
    let mother_site = s.state.holding(mother).unwrap().site;
    let Place::EmergencyJump { site, arrive } = s.state.fleet(docked).unwrap().place else {
        panic!("{:?}", s.state.fleet(docked).unwrap().place);
    };
    assert_eq!(site, mother_site);
    assert!(arrive > fell);
    assert!(said.iter().any(|(_, k, _)| *k
        == AnnouncementKind::FleetsJumped {
            commander: CommanderId(DEFENDER),
            fleets: vec![(docked, mother_site, arrive)],
        }));
    assert!(crate::command::locate(&s.state, docked.sim_id()).is_none());
    assert_eq!(
        validate_command(
            &s.state,
            commander(DEFENDER),
            &order(docked, FleetOrder::Launch)
        ),
        Err(R::InTransit)
    );
    assert!(
        s.state.fleet_problems().is_empty(),
        "{:?}",
        s.state.fleet_problems()
    );
    skip_to(&mut s.state, arrive);
    let out = step(&mut s.state, &TickInputs::default());
    assert_eq!(s.state.fleet(docked).unwrap().place, Place::Docked(mother));
    assert!(
        out.fleet_events
            .contains(&crate::fleet::FleetEvent::Docked {
                fleet: docked,
                holding: mother
            })
    );
}

#[test]
fn the_capturer_has_the_most_core_damage_then_the_most_damage_then_a_seeded_draw() {
    // D-057, D-163, D-229.
    let tally = |core: u64, total: u64| crate::defence::Tally {
        core_milli: core,
        total_milli: total,
    };
    let mut picks = std::collections::BTreeSet::new();
    for seed in 0..12 {
        let mut state = stage1(seed);
        let (holding, _) = home(&state, DEFENDER);
        let ledger = &mut state.defence.holdings.entry(holding).or_default().ledger;
        ledger.tally.insert(CommanderId(2), tally(500, 900));
        ledger.tally.insert(CommanderId(3), tally(500, 900));
        ledger.tally.insert(CommanderId(4), tally(400, 5_000));
        let mut again = state.clone();
        let owner = CommanderId(DEFENDER);
        let by = commander(4);
        let pick = super::lifecycle::capturer(&mut state, holding, owner, by);
        assert_eq!(
            pick,
            super::lifecycle::capturer(&mut again, holding, owner, by)
        );
        assert!(matches!(pick, Some(CommanderId(2 | 3))), "{pick:?}");
        picks.insert(pick);
        // A larger total breaks a core tie; more core damage beats any total.
        let ledger = &mut state.defence.holdings.get_mut(&holding).unwrap().ledger;
        ledger.tally.insert(CommanderId(3), tally(500, 901));
        assert_eq!(
            super::lifecycle::capturer(&mut state, holding, owner, by),
            Some(CommanderId(3))
        );
    }
    // The draw is not the lower `CommanderId` every time (D-229).
    assert_eq!(picks.len(), 2, "{picks:?}");
}

#[test]
fn a_fallen_mother_station_jumps_and_leaves_its_site_to_the_capturer_as_an_outpost() {
    let mut s = Scene::mother();
    let kit = kit_fleet(&s.state, DEFENDER);
    assert_eq!(s.state.fleet(kit).unwrap().place, Place::Docked(s.target));
    let site = s.state.holding(s.target).unwrap().site;
    let others = s.state.modules_at(s.target).len() - 1;
    // A full pool: the grant is paid whatever the cap (D-167).
    let cap = s.state.pool_cap(CommanderId(DEFENDER));
    s.state.economy.pools.insert(CommanderId(DEFENDER), cap);
    let price = s.state.core_kit_price(CommanderId(DEFENDER));
    let out = fall(&mut s, 400);
    let fell = out.last().unwrap().tick;
    // The station left: no holding there, the capturer's outpost instead (D-057, D-224).
    assert!(s.state.holding(s.target).is_none());
    let (outpost, h) = s.state.holding_at(site).unwrap();
    assert_eq!((h.owner, h.core_complete), (CommanderId(ATTACKER), false));
    let outpost_core = s.state.economy.holdings[&outpost].slots[&Slot::Core];
    assert_eq!(s.state.module(outpost_core).unwrap().hp, 30_000);
    // The other modules are destroyed (D-034).
    let destroyed = out
        .iter()
        .flat_map(|o| &o.economy_events)
        .filter(|e| matches!(e, economy::EconomyEvent::ModuleDestroyed { holding, .. } if *holding == s.target))
        .count();
    assert_eq!(destroyed, others);
    // The grant: the leader holds 1 holding, the defender none after the fall, so one kit
    // (D-167), paid above the cap.
    assert_eq!(s.state.pool(CommanderId(DEFENDER)), cap.plus(price));
    // It stays the commander's mother station for the command cap (S6 plan §4.6).
    assert_eq!(s.state.command_cap(CommanderId(DEFENDER)), 120);
    let jumps = s.state.station_jumps();
    assert_eq!(jumps.len(), 1);
    let StationLeg::ToSite { site: to, arrive } = jumps[0].leg else {
        panic!("{:?}", jumps[0].leg);
    };
    // D-164: the commander has no other holding; D-165 picks the free site in the least
    // held claimable system, by route time from the fallen site.
    assert!(arrive > fell);
    assert!(super::site_reserved(&s.state, to));
    assert_eq!(s.state.holding_at(to), None);
    // The docked start kit jumps with it.
    assert_eq!(
        s.state.fleet(kit).unwrap().place,
        Place::EmergencyJump { site: to, arrive }
    );
    let said = announcements(&out);
    assert!(said.iter().any(|(_, k, to)| *to == Recipients::Public
        && *k
            == AnnouncementKind::MotherStationFell {
                site,
                from: CommanderId(DEFENDER),
                to: CommanderId(ATTACKER),
                outpost,
            }));
    assert!(said.iter().any(|(_, k, rec)| *rec
        == Recipients::Commanders(vec![CommanderId(DEFENDER)])
        && *k
            == AnnouncementKind::MotherStationJumped {
                holding: s.target,
                from_site: site,
                to_site: Some(to),
                arrival_tick: Some(arrive),
                protected_until: Some(arrive.after(Ticks::from_hours(72))),
                grant: price,
            }));
    assert!(
        s.state.fleet_problems().is_empty(),
        "{:?}",
        s.state.fleet_problems()
    );
    assert!(
        s.state.economy_problems().is_empty(),
        "{:?}",
        s.state.economy_problems()
    );
    // The landing: a mother station again on the new site, 72 h protection, the kit docked.
    skip_to(&mut s.state, arrive);
    let out = step(&mut s.state, &TickInputs::default());
    let h = s.state.holding(s.target).unwrap();
    assert_eq!(
        (h.site, h.mother_station, h.core_complete),
        (to, true, true)
    );
    assert_eq!(
        s.view().breachable_from,
        arrive.after(Ticks::from_hours(72))
    );
    assert_eq!(s.state.module(s.core()).unwrap().hp, 150_000);
    assert_eq!(s.state.docking_capacity(s.target), 24);
    assert_eq!(s.state.fleet(kit).unwrap().place, Place::Docked(s.target));
    assert!(s.state.station_jumps().is_empty());
    assert!(
        announcements(std::slice::from_ref(&out))
            .iter()
            .any(|(_, k, _)| *k
                == AnnouncementKind::StationLanded {
                    commander: CommanderId(DEFENDER),
                    holding: s.target,
                    site: to,
                    breachable_from: arrive.after(Ticks::from_hours(72)),
                })
    );
    // A minimal dome: the core's 25% in every sector (D-140).
    idle(&mut s.state, TICKS_PER_SECOND);
    let dome = s.state.defence(s.target).unwrap();
    assert!(dome.dome);
    assert!(dome.sectors.iter().all(|d| d.max_milli == 37_500 * MILLI));
}

#[test]
fn a_reserved_site_cannot_be_claimed_and_jumping_fleets_take_no_orders() {
    let mut s = Scene::mother();
    let kit = kit_fleet(&s.state, DEFENDER);
    fall(&mut s, 400);
    let StationLeg::ToSite { site, .. } = s.state.station_jumps()[0].leg else {
        panic!("a site is free");
    };
    // Another commander's MULE carrying a kit next to the reserved site.
    let (mule_fleet, mule) = {
        let system = s.state.galaxy().site(site).unwrap().system;
        let at = Vec2::from(s.state.galaxy().site(site).unwrap().position);
        let fleet = spawn_fleet(
            &mut s.state,
            commander(3),
            system,
            at + Vec2::new(500.0, 0.0),
            &[ShipClass::Mule],
        );
        (fleet, s.state.fleet(fleet).unwrap().ships[0])
    };
    s.state.economy.cargo.insert(
        mule,
        economy::Cargo {
            stock: economy::Stock::ZERO,
            core_kit: true,
        },
    );
    assert_eq!(
        validate_command(
            &s.state,
            commander(3),
            &Command::Economy(EconomyCommand::DeployCoreKit {
                fleet: mule_fleet,
                site
            })
        ),
        Err(R::SiteTaken)
    );
    for order in [
        FleetOrder::Launch,
        FleetOrder::StationKeep { relative_to: None },
    ] {
        assert_eq!(
            validate_command(
                &s.state,
                commander(DEFENDER),
                &Command::Fleet {
                    fleet: kit,
                    order,
                    queue: Queueing::Replace
                }
            ),
            Err(R::InTransit)
        );
    }
    let ships = s.state.fleet(kit).unwrap().ships;
    assert_eq!(
        validate_command(
            &s.state,
            commander(DEFENDER),
            &Command::CreateFleet { ships }
        ),
        Err(R::InTransit)
    );
    // Rules of engagement are a standing setting and apply anywhere.
    assert_eq!(
        validate_command(
            &s.state,
            commander(DEFENDER),
            &order(
                kit,
                FleetOrder::SetRulesOfEngagement {
                    rules: nomad_types::RulesOfEngagement::HoldFire
                }
            )
        ),
        Ok(())
    );
}

#[test]
fn an_outpost_under_construction_is_destroyed_at_any_time() {
    // D-054: outside any window, by a commander or by raiders.
    for raiders in [false, true] {
        let mut state = stage1(7);
        let (_, system) = home(&state, DEFENDER);
        let site = free_site(&state, system);
        let outpost = state
            .insert_holding(site, CommanderId(DEFENDER), false)
            .unwrap();
        let core = economy::spawn_outpost_core(&mut state, outpost);
        state.entities.get_mut::<Hull>(core).unwrap().hp = 200;
        let at = Vec2::from(state.galaxy().site(site).unwrap().position);
        let owner = if raiders {
            Owner::Npc(nomad_types::FactionId(1))
        } else {
            commander(ATTACKER)
        };
        let fleet = spawn_fleet(
            &mut state,
            owner,
            system,
            at + Vec2::new(2_500.0, 0.0),
            // One raider strike craft deals 37.5 hull a second to structures, exactly the
            // growth of an outpost's hull (D-224), so two.
            &if raiders {
                [ShipClass::RaiderStrikeCraft; 2]
            } else {
                [LANCER; 2]
            },
        );
        crate::fleet::orders::give(
            &mut state,
            fleet,
            FleetOrder::Attack {
                target: outpost.sim_id(),
            },
            Queueing::Replace,
            &mut Vec::new(),
        );
        let out = idle(&mut state, 300);
        assert!(state.holding(outpost).is_none(), "raiders {raiders}");
        assert!(state.holding_at(site).is_none());
        assert!(
            announcements(&out)
                .iter()
                .any(|(_, k, to)| *to == Recipients::Public
                    && *k
                        == AnnouncementKind::OutpostDestroyed {
                            holding: outpost,
                            site,
                            owner: CommanderId(DEFENDER),
                        })
        );
        assert!(state.economy_problems().is_empty());
    }
}

#[test]
fn snapshots_inside_reinforced_a_siege_and_an_emergency_jump_change_nothing() {
    // S6 plan §8 "Determinism".
    let mut s = Scene::mother();
    s.breach_at(t(1, 14, 20));
    let exit = s.exit();
    presence(&mut s.state, ATTACKER, true);
    let core = s.core();
    let script = |state: &mut UniverseState, phase: u32| match phase {
        0 => skip_to(state, Tick::new(exit.get() - Ticks::from_secs(5).get())),
        1 => {
            state.entities.get_mut::<Hull>(core).unwrap().hp = 400;
            let out = send(
                state,
                ATTACKER,
                order(s.fleet, FleetOrder::Attack { target: core }),
            );
            assert_eq!(out.commands[0].outcome, Ok(()));
        }
        _ => {}
    };
    let run = |mut state: UniverseState, restore_at: Option<u32>| {
        let mut hashes = Vec::new();
        for phase in 0..4_u32 {
            script(&mut state, phase);
            for _ in 0..600 {
                step(&mut state, &TickInputs::default());
            }
            if restore_at == Some(phase) {
                state = decode(&encode(&state).unwrap()).unwrap();
            }
            hashes.push(state_hash(&state).unwrap());
        }
        (hashes, state)
    };
    let (reference, end) = run(s.state.clone(), None);
    // The station fell and is in its jump by the end.
    assert_eq!(end.station_jumps().len(), 1);
    for restore_at in 0..4 {
        let (hashes, _) = run(s.state.clone(), Some(restore_at));
        assert_eq!(hashes, reference, "restored after phase {restore_at}");
    }
    // Inside Reinforced too.
    let restored = decode(&encode(&s.state).unwrap()).unwrap();
    assert_eq!(restored, s.state);
}

#[test]
fn a_fallen_mother_station_goes_by_its_nearest_holding_or_waits_there_for_a_free_site() {
    // D-164 case 1: a free site in the system of the commander's nearest holding, the
    // shortest route from the fallen site first.
    let mut s = Scene::mother();
    let (_, system) = home(&s.state, DEFENDER);
    let remote_site = free_site(&s.state, system);
    place_holding(&mut s.state, remote_site, DEFENDER);
    fall(&mut s, 400);
    let StationLeg::ToSite { site, .. } = s.state.station_jumps()[0].leg else {
        panic!("{:?}", s.state.station_jumps());
    };
    assert_eq!(s.state.galaxy().site(site).unwrap().system, system);
    // It stays their mother station for the command cap: 120 plus the remote holding.
    assert_eq!(s.state.command_cap(CommanderId(DEFENDER)), 130);

    // D-164 case 3: every site taken, so it waits by its nearest holding.
    let mut s = Scene::mother();
    let remote_site = free_site(&s.state, system);
    let remote = place_holding(&mut s.state, remote_site, DEFENDER);
    let mut outposts = Vec::new();
    let sites: Vec<SiteId> = s
        .state
        .galaxy()
        .systems()
        .filter(|(_, sys)| crate::galaxy::is_claimable(sys.kind))
        .flat_map(|(_, sys)| sys.sites.clone())
        .collect();
    for site in sites {
        if s.state.holding_at(site).is_none() {
            let outpost = s.state.insert_holding(site, CommanderId(3), false).unwrap();
            economy::spawn_outpost_core(&mut s.state, outpost);
            outposts.push((outpost, site));
        }
    }
    let kit = kit_fleet(&s.state, DEFENDER);
    fall(&mut s, 400);
    let StationLeg::ToHolding { holding, arrive } = s.state.station_jumps()[0].leg else {
        panic!("{:?}", s.state.station_jumps());
    };
    assert_eq!(holding, remote);
    // Its docked ships go with it, and dock at the holding it waits by.
    assert_eq!(
        s.state.fleet(kit).unwrap().place,
        Place::EmergencyJump {
            site: remote_site,
            arrive
        }
    );
    skip_to(&mut s.state, arrive);
    step(&mut s.state, &TickInputs::default());
    assert_eq!(
        s.state.station_jumps()[0].leg,
        StationLeg::Waiting { by: Some(remote) }
    );
    assert_eq!(s.state.fleet(kit).unwrap().place, Place::Docked(remote));
    // An outpost is destroyed: the site is free, and the station jumps to it.
    let (outpost, freed) = outposts[0];
    let now = s.state.tick();
    let mut out = TickOutputs::default();
    super::capture::destroy_outpost(&mut s.state, now, outpost, &mut out);
    let StationLeg::ToSite { site, arrive } = s.state.station_jumps()[0].leg else {
        panic!("{:?}", s.state.station_jumps());
    };
    assert_eq!(site, freed);
    assert!(out.announcements.iter().any(|a| matches!(
        a.kind,
        AnnouncementKind::MotherStationJumped { to_site: Some(to), .. } if to == freed
    )));
    skip_to(&mut s.state, arrive);
    step(&mut s.state, &TickInputs::default());
    assert_eq!(s.state.holding(s.target).unwrap().site, freed);
    assert!(s.state.station_jumps().is_empty());
}

/// Commander 1's mother station with a fleet of `owner` 2.4 km beyond its peripheral module
/// in `slot`, attacking it, the module at `hull`.
fn peripheral_scene(
    owner: Owner,
    class: ShipClass,
    slot: Slot,
    hull: u32,
) -> (UniverseState, SimId) {
    let mut state = stage1(7);
    let (mother, system) = home(&state, DEFENDER);
    idle(&mut state, TICKS_PER_SECOND);
    let module = state.economy.holdings[&mother].slots[&slot];
    let site = state.holding(mother).unwrap().site;
    let centre = Vec2::from(state.galaxy().site(site).unwrap().position);
    let (_, at, _) = state.module_position(module).unwrap();
    let at = Vec2::from(at);
    let out = (at - centre).normalized().unwrap();
    let fleet = spawn_fleet(&mut state, owner, system, at + out * 2_400.0, &[class; 2]);
    crate::fleet::orders::give(
        &mut state,
        fleet,
        FleetOrder::Attack { target: module },
        Queueing::Replace,
        &mut Vec::new(),
    );
    state.entities.get_mut::<Hull>(module).unwrap().hp = hull;
    (state, module)
}

fn module_events(outputs: &[TickOutputs], module: SimId) -> Vec<economy::EconomyEvent> {
    outputs
        .iter()
        .flat_map(|o| o.economy_events.iter().copied())
        .filter(|e| match *e {
            economy::EconomyEvent::ModuleDisabled { module: m }
            | economy::EconomyEvent::ModuleDestroyed { module: m, .. } => m == module,
            _ => false,
        })
        .collect()
}

#[test]
fn a_peripheral_structure_is_disabled_outside_the_window_and_destroyed_inside_it() {
    // Doc 12 §3, D-138, D-169: the default window is 19:00–23:00 UTC.
    let (mut state, solar) = peripheral_scene(commander(ATTACKER), LANCER, Slot::Solar, 200);
    skip_to(&mut state, t(1, 10, 0));
    let out = idle(&mut state, 300);
    assert_eq!(
        module_events(&out, solar),
        [economy::EconomyEvent::ModuleDisabled { module: solar }]
    );
    assert!(state.module(solar).unwrap().disabled);
    // Still under fire when the window opens: the next hit destroys it (D-169).
    skip_to(&mut state, t(1, 19, 30));
    let out = idle(&mut state, 300);
    assert!(matches!(
        module_events(&out, solar).as_slice(),
        [economy::EconomyEvent::ModuleDestroyed {
            kind: ModuleKind::Solar,
            ..
        }]
    ));
    assert!(state.module(solar).is_none());
    // A harvester brought to 0 inside the window is destroyed at once.
    let (mut state, harvester) =
        peripheral_scene(commander(ATTACKER), LANCER, Slot::Deposit(0), 200);
    skip_to(&mut state, t(1, 20, 0));
    let out = idle(&mut state, 300);
    assert!(matches!(
        module_events(&out, harvester).as_slice(),
        [economy::EconomyEvent::ModuleDestroyed {
            kind: ModuleKind::Harvester,
            ..
        }]
    ));
}

#[test]
fn raiders_only_disable_peripheral_structures_even_inside_the_window() {
    // D-223.
    let raiders = Owner::Npc(nomad_types::FactionId(1));
    let (mut state, harvester) =
        peripheral_scene(raiders, ShipClass::RaiderStrikeCraft, Slot::Deposit(0), 200);
    skip_to(&mut state, t(1, 20, 0));
    let out = idle(&mut state, 400);
    assert_eq!(
        module_events(&out, harvester),
        [economy::EconomyEvent::ModuleDisabled { module: harvester }]
    );
    assert!(state.module(harvester).unwrap().disabled);
}
