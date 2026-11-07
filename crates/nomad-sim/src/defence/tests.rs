//! Tests of station combat against S5 plan §8 "Station tests (S5b)" and "Validator".

use std::collections::BTreeSet;

use nomad_types::{
    Command, EconomyCommand, FactionId, FleetOrder, HoldingOrder, ModuleKind, Owner, PowerProfile,
    Queueing, RulesOfEngagement, Slot, StationAbility,
};

use super::DefenceEvent;
use crate::combat::{Gun, Projectile, ProjectileId, Shooter, WeaponSlot, Weapons};
use crate::command::CommandRejection as R;
use crate::command::tests::defence_reasons;
use crate::economy::{self, Consumer, EconomyEvent, Hull};
use crate::fleet::{Fleet, FleetId, ShipClass, ShipId};
use crate::galaxy::SystemId;
use crate::geometry::Vec2;
use crate::holding::HoldingId;
use crate::id::{CommanderId, SimId};
use crate::snapshot::{decode, encode, state_hash};
use crate::state::UniverseState;
use crate::step::TickOutputs;
use crate::testkit::{commander, home, idle, order, send, spawn_fleet, stage1};
use crate::time::{TICKS_PER_SECOND, Tick};
use crate::validate_command;

const LANCER: ShipClass = ShipClass::Lancer;
const WARDEN: ShipClass = ShipClass::Warden;
const VX: ShipClass = ShipClass::Vx;
const RAIDER: ShipClass = ShipClass::RaiderStrikeCraft;
const HSTL: ShipClass = ShipClass::HeavyCruiser;
const MILLI: u64 = 1_000;
const FULL: u64 = 150_000 * MILLI;
/// Sectors in pylon slot order: north, east, south, west in the system frame (D-046).
const NORTH: u8 = 0;
const EAST: u8 = 1;
const SOUTH: u8 = 2;
const WEST: u8 = 3;
/// Directions from the mother station's centre, in degrees from east, with a clear line of
/// fire to its dome: the solar array (north-north-west, 1.6 km out, 700 m radius) and the
/// harvester (south-south-east) are structures too, and take shots that cross them first.
const CLEAR_NORTH: f64 = 60.0;
const CLEAR_SOUTH: f64 = -110.0;

/// Commander 1's mother station after one second, so its dome is up (domes are refreshed in
/// the holding's economy slot): the Universe, the holding, its system and its centre.
struct Station {
    state: UniverseState,
    holding: HoldingId,
    system: SystemId,
    centre: Vec2,
}

fn station() -> Station {
    let mut state = stage1(7);
    let (holding, system) = home(&state, 1);
    let site = state.holding(holding).unwrap().site;
    let centre = Vec2::from(state.galaxy().site(site).unwrap().position);
    idle(&mut state, TICKS_PER_SECOND);
    Station {
        state,
        holding,
        system,
        centre,
    }
}

/// Attackers fire at what their Attack order names and at nothing else unprovoked: explicit
/// targets are fired at under return fire (D-188).
const fn attacker() -> RulesOfEngagement {
    RulesOfEngagement::ReturnFire
}

/// Targets that never shoot back.
const fn passive() -> RulesOfEngagement {
    RulesOfEngagement::HoldFire
}

fn raiders() -> Owner {
    Owner::Npc(FactionId(1))
}

impl Station {
    /// A point `distance_m` from the dome centre at `angle_deg` counter-clockwise from east.
    fn at(&self, angle_deg: f64, distance_m: f64) -> Vec2 {
        self.centre + Vec2::from_angle(angle_deg.to_radians()) * distance_m
    }

    /// A fleet of `owner` at `point`, with rules of engagement `roe`.
    fn fleet(
        &mut self,
        owner: Owner,
        point: Vec2,
        classes: &[ShipClass],
        roe: RulesOfEngagement,
    ) -> FleetId {
        let fleet = spawn_fleet(&mut self.state, owner, self.system, point, classes);
        if let Some(f) = self.state.entities.get_mut::<Fleet>(fleet.sim_id()) {
            f.rules_of_engagement = roe;
        }
        fleet
    }

    /// Commander `n` orders `fleet` to attack `target`.
    fn attack(&mut self, n: u32, fleet: FleetId, target: SimId) {
        let out = send(
            &mut self.state,
            n,
            order(fleet, FleetOrder::Attack { target }),
        );
        assert_eq!(out.commands[0].outcome, Ok(()), "{:?}", out.commands);
    }

    /// An NPC fleet's attack order, set directly: the raider director (A1) will issue it.
    fn raid(&mut self, fleet: FleetId, target: SimId) {
        crate::fleet::orders::give(
            &mut self.state,
            fleet,
            FleetOrder::Attack { target },
            Queueing::Replace,
            &mut Vec::new(),
        );
    }

    fn strength(&self, sector: u8) -> u64 {
        self.state.defence(self.holding).unwrap().sectors[usize::from(sector)].strength_milli
    }

    fn set_strength(&mut self, sector: u8, milli: u64) {
        let d = self.state.defence.holdings.get_mut(&self.holding).unwrap();
        d.sectors[usize::from(sector)].strength = milli;
    }

    fn module(&self, slot: Slot) -> SimId {
        self.state.economy.holdings[&self.holding].slots[&slot]
    }

    fn place(&mut self, slot: Slot, kind: ModuleKind) -> SimId {
        crate::scenario::place_module(&mut self.state, self.holding, slot, kind).unwrap()
    }

    fn set_hull(&mut self, module: SimId, hp: u32) {
        self.state.entities.get_mut::<Hull>(module).unwrap().hp = hp;
    }

    fn hull(&self, module: SimId) -> u32 {
        self.state.module(module).map_or(0, |m| m.hp)
    }

    fn disable(&mut self, slot: Slot) {
        let module = self.module(slot);
        let now = self.state.tick();
        economy::disable_module(&mut self.state, module, now, &mut Vec::new());
    }

    /// Commander 1 sends a station order; its outcome.
    fn holding_order(&mut self, order: HoldingOrder) -> Result<(), R> {
        let out = send(
            &mut self.state,
            1,
            Command::Holding {
                holding: self.holding,
                order,
            },
        );
        out.commands[0].outcome
    }

    fn ability(&mut self, ability: StationAbility) -> Result<(), R> {
        self.holding_order(HoldingOrder::Ability { ability })
    }

    fn check(&self, n: u32, ability: StationAbility) -> Result<(), R> {
        validate_command(
            &self.state,
            commander(n),
            &Command::Holding {
                holding: self.holding,
                order: HoldingOrder::Ability { ability },
            },
        )
    }

    fn turrets_hold(&mut self) {
        self.state
            .defence
            .holdings
            .get_mut(&self.holding)
            .unwrap()
            .turrets_hold = true;
    }
}

/// The sectors that took damage in `outputs`.
fn sectors_hit(outputs: &[TickOutputs]) -> BTreeSet<u8> {
    defence_events(outputs)
        .into_iter()
        .filter_map(|e| match e {
            DefenceEvent::SectorDamaged { sector, .. } => Some(sector),
            _ => None,
        })
        .collect()
}

fn defence_events(outputs: &[TickOutputs]) -> Vec<DefenceEvent> {
    outputs
        .iter()
        .flat_map(|o| o.defence_events.iter().copied())
        .collect()
}

fn economy_events(outputs: &[TickOutputs]) -> Vec<EconomyEvent> {
    outputs
        .iter()
        .flat_map(|o| o.economy_events.iter().copied())
        .collect()
}

fn first_ship(state: &UniverseState, fleet: FleetId) -> ShipId {
    state.fleet(fleet).unwrap().ships[0]
}

/// Hull left of `fleet`'s first ship, 0 once the fleet is gone.
fn ship_hull(state: &UniverseState, fleet: FleetId) -> u32 {
    state
        .fleet(fleet)
        .and_then(|f| state.ship_hull(f.ships[0]))
        .map_or(0, |(left, _)| left)
}

const fn surge(sector: u8) -> StationAbility {
    StationAbility::ShieldSurge { sector }
}

#[test]
fn every_core_projects_a_full_dome_and_pylons_set_the_maxima() {
    let mut s = station();
    let view = s.state.defence(s.holding).unwrap();
    assert!(view.dome);
    for sector in view.sectors {
        assert_eq!((sector.strength_milli, sector.max_milli), (FULL, FULL));
        assert!(!sector.weak && !sector.open && !sector.breached);
    }
    // Without its pylon a sector keeps the core's 25% (D-140); strength follows the maximum
    // down within the second (the holding's economy slot).
    let pylon = s.module(Slot::Pylon(EAST));
    economy::destroy_module(&mut s.state, pylon, &mut Vec::new());
    idle(&mut s.state, TICKS_PER_SECOND);
    let view = s.state.defence(s.holding).unwrap();
    assert_eq!(view.sectors[usize::from(EAST)].max_milli, FULL / 4);
    assert_eq!(view.sectors[usize::from(EAST)].strength_milli, FULL / 4);
    assert_eq!(view.sectors[usize::from(NORTH)].max_milli, FULL);
    // A new pylon raises the maximum; the sector regenerates towards it, it does not jump.
    s.place(Slot::Pylon(EAST), ModuleKind::Pylon);
    idle(&mut s.state, TICKS_PER_SECOND);
    let east = s.state.defence(s.holding).unwrap().sectors[usize::from(EAST)];
    assert_eq!(east.max_milli, FULL);
    assert!(east.strength_milli < FULL / 4 + 100 * MILLI, "{east:?}");
    assert!(east.weak);
}

#[test]
fn hostile_fire_stops_at_the_dome_and_outgoing_fire_passes() {
    let mut s = station();
    // A friendly WARDEN inside the dome, by the launch point, and a hostile LANCER outside,
    // 2 km to the west of it: each in range of the other.
    let launch = Vec2::from(s.state.launch_point(s.holding).unwrap());
    assert!((launch - s.centre).length() < 1_260.0);
    let warden = s.fleet(
        commander(1),
        launch,
        &[WARDEN],
        RulesOfEngagement::WeaponsFree,
    );
    let lancer = s.fleet(
        commander(2),
        launch + Vec2::new(-2_000.0, 0.0),
        &[LANCER],
        RulesOfEngagement::WeaponsFree,
    );
    let out = idle(&mut s.state, 120);
    // The torpedo stopped at the west sector; the WARDEN is untouched.
    assert_eq!(ship_hull(&s.state, warden), 7_500);
    assert!(s.strength(WEST) < FULL);
    assert!(defence_events(&out).iter().any(|e| matches!(
        e,
        DefenceEvent::SectorDamaged { sector, by, .. } if *sector == WEST && *by == commander(2)
    )));
    // The WARDEN's batteries went out through its own dome and hit the LANCER.
    assert!(ship_hull(&s.state, lancer) < 800);
    for sector in [NORTH, EAST, SOUTH] {
        assert_eq!(s.strength(sector), FULL);
    }
}

#[test]
fn a_hostile_flak_shell_stops_at_the_dome() {
    let mut s = station();
    // A friendly VX inside the dome; a hostile WARDEN's flak shell aimed at it bursts never.
    let inside = s.at(90.0, 1_000.0);
    let vx = s.fleet(commander(1), inside, &[VX], passive());
    let outside = s.fleet(commander(1), s.at(90.0, 1_600.0), &[VX], passive());
    let warden = s.fleet(commander(2), s.at(90.0, 2_900.0), &[WARDEN], passive());
    // Shells from 2.9 km out at 800 m/s: 1.9 km and 1.3 km of flight, 24 and 17 ticks.
    for (at, ticks) in [(inside, 24), (s.at(90.0, 1_600.0), 17)] {
        let from = s.at(90.0, 2_900.0);
        let speed = 800.0;
        s.state.projectiles.launch(Projectile {
            id: ProjectileId(0),
            system: s.system,
            owner: commander(2),
            shooter: Shooter::Ship(first_ship(&s.state, warden)),
            gun: Gun::Ship {
                class: WARDEN,
                weapon: 2,
            },
            position: from,
            velocity: (at - from).normalized().unwrap() * speed,
            remaining_m: 2_000.0,
            burst: Some(crate::combat::Burst { ticks, at }),
        });
    }
    idle(&mut s.state, 40);
    assert_eq!(ship_hull(&s.state, vx), 600, "the dome stopped the shell");
    assert_eq!(
        ship_hull(&s.state, outside),
        600 - 60,
        "30 × 200% outside it"
    );
    // Flak does nothing to domes (doc 15 §3.2).
    assert_eq!(s.strength(NORTH), FULL);
}

#[test]
fn the_sector_hit_is_the_one_whose_arc_the_shot_crosses() {
    // D-046: N, E, S and W arcs of 90° centred on their pylons, in the system frame.
    for (angle, expected) in [
        (CLEAR_NORTH, NORTH),
        (50.0, NORTH),
        (30.0, EAST),
        (-40.0, EAST),
        (CLEAR_SOUTH, SOUTH),
        (-130.0, SOUTH),
        (150.0, WEST),
        (220.0, WEST),
    ] {
        let mut s = station();
        let point = s.at(angle, 3_500.0);
        let lancer = s.fleet(commander(2), point, &[LANCER], attacker());
        s.attack(2, lancer, s.holding.sim_id());
        let out = idle(&mut s.state, 100);
        assert_eq!(
            sectors_hit(&out),
            BTreeSet::from([expected]),
            "a shot from {angle}°"
        );
    }
    // A shot from due north meets the solar array first, which lies outside the dome.
    let mut s = station();
    let lancer = s.fleet(commander(2), s.at(95.0, 3_500.0), &[LANCER], attacker());
    s.attack(2, lancer, s.holding.sim_id());
    let out = idle(&mut s.state, 100);
    let solar = s.module(Slot::Solar);
    assert!(sectors_hit(&out).is_empty());
    assert!(s.hull(solar) < 20_000);
    // The geometry itself, at the arcs' centres and just inside their edges.
    let rules = crate::rules::Rules::embedded();
    let directions = super::dome::pylon_directions(&rules);
    for (angle, expected) in [
        (90.0, 0),
        (46.0, 0),
        (44.0, 1),
        (0.0, 1),
        (-89.0, 2),
        (181.0, 3),
    ] {
        let offset = Vec2::from_angle(f64::to_radians(angle));
        assert_eq!(
            super::dome::sector_at(&directions, offset),
            expected,
            "{angle}°"
        );
    }
}

#[test]
fn regeneration_follows_the_allocated_power() {
    let mut s = station();
    // A sector at two thirds regenerates 50 points a second at the full 120 MW (D-142).
    s.set_strength(NORTH, 100_000 * MILLI);
    idle(&mut s.state, 20);
    let power = s.state.power(s.holding).unwrap();
    assert_eq!(power.regeneration_kw(NORTH), 120_000);
    let before = s.strength(NORTH);
    idle(&mut s.state, 100);
    assert_eq!(s.strength(NORTH) - before, 500 * MILLI);
    // With every sector short and the solar array off, production first: 520 MW, 160 MW for
    // the pylons, 180 MW for production, and 180 MW shared by the four sectors.
    s.disable(Slot::Solar);
    s.state
        .economy
        .holdings
        .get_mut(&s.holding)
        .unwrap()
        .profile = PowerProfile::ProductionFirst;
    for sector in 0..4 {
        s.set_strength(sector, 50_000 * MILLI);
    }
    idle(&mut s.state, 20);
    let power = s.state.power(s.holding).unwrap();
    for sector in 0..4 {
        assert_eq!(power.regeneration_kw(sector), 45_000);
    }
    let before = s.strength(SOUTH);
    idle(&mut s.state, 100);
    // 45 MW × 50 / 120 = 18.75 points a second, exactly over 10 s.
    assert_eq!(s.strength(SOUTH) - before, 187_500);
    // Full sectors and open sectors ask for nothing, and an open one does not regenerate.
    s.state.restore_sectors(s.holding, 100);
    assert!(s.state.open_sector(s.holding, WEST));
    s.set_strength(WEST, 0);
    idle(&mut s.state, 20);
    assert_eq!(s.state.economy.holdings[&s.holding].regen_demand_kw, [0; 4]);
    assert_eq!(s.strength(WEST), 0);
}

#[test]
fn a_sector_without_its_pylon_regenerates_at_a_quarter() {
    let mut s = station();
    let pylon = s.module(Slot::Pylon(SOUTH));
    economy::destroy_module(&mut s.state, pylon, &mut Vec::new());
    idle(&mut s.state, TICKS_PER_SECOND);
    assert_eq!(s.strength(SOUTH), FULL / 4);
    s.set_strength(SOUTH, 10_000 * MILLI);
    idle(&mut s.state, 20);
    assert_eq!(
        s.state.power(s.holding).unwrap().regeneration_kw(SOUTH),
        30_000
    );
    let before = s.strength(SOUTH);
    idle(&mut s.state, 100);
    // D-142: at most 12.5 points a second and 30 MW for a 25% sector.
    assert_eq!(s.strength(SOUTH) - before, 125 * MILLI);
}

#[test]
fn a_breach_is_reported_once() {
    let mut s = station();
    let point = s.at(CLEAR_NORTH, 3_500.0);
    let lancers = s.fleet(commander(2), point, &[LANCER; 6], attacker());
    s.set_strength(NORTH, 1_000 * MILLI);
    s.attack(2, lancers, s.holding.sim_id());
    let out = idle(&mut s.state, 400);
    let breaches: Vec<DefenceEvent> = defence_events(&out)
        .into_iter()
        .filter(|e| matches!(e, DefenceEvent::SectorBreached { .. }))
        .collect();
    assert_eq!(breaches.len(), 1, "{breaches:?}");
    assert!(matches!(
        breaches[0],
        DefenceEvent::SectorBreached { holding, sector: NORTH, .. } if holding == s.holding
    ));
    let view = s.state.defence(s.holding).unwrap();
    assert!(view.sectors[usize::from(NORTH)].breached);
    assert!(view.sectors[usize::from(NORTH)].weak);
    // The ledger names the attacker for S6 (D-057).
    assert_eq!(
        s.state.dome_attackers(s.holding, Tick::ZERO),
        [CommanderId(2)]
    );
    let ledger = s.state.damage_ledger(s.holding).unwrap();
    assert_eq!(ledger.dome.len(), 1);
    assert!(ledger.dome[0].1.milli >= 1_000 * MILLI);
    // Restoring re-arms it (S6's Held, 50%; D-057).
    s.state.restore_sectors(s.holding, 50);
    let view = s.state.defence(s.holding).unwrap();
    assert_eq!(view.sectors[usize::from(NORTH)].strength_milli, FULL / 2);
    assert!(!view.sectors[usize::from(NORTH)].breached);
}

#[test]
fn raiders_never_reduce_a_sector() {
    let mut s = station();
    // D-056: a raid group with a heavy cruiser ordered against the dome does nothing to it.
    let point = s.at(90.0, 3_400.0);
    let raid = s.fleet(raiders(), point, &[RAIDER, RAIDER, HSTL], attacker());
    s.raid(raid, s.holding.sim_id());
    // And a raider torpedo flying at the dome stops there without effect.
    let from = s.at(0.0, 2_000.0);
    s.state.projectiles.launch(Projectile {
        id: ProjectileId(0),
        system: s.system,
        owner: raiders(),
        shooter: Shooter::Ship(first_ship(&s.state, raid)),
        gun: Gun::Ship {
            class: RAIDER,
            weapon: 0,
        },
        position: from,
        velocity: Vec2::new(-350.0, 0.0),
        remaining_m: 2_500.0,
        burst: None,
    });
    let out = idle(&mut s.state, 300);
    for sector in 0..4 {
        assert_eq!(s.strength(sector), FULL);
    }
    assert!(
        defence_events(&out).is_empty(),
        "{:?}",
        defence_events(&out)
    );
    assert!(s.state.damage_ledger(s.holding).unwrap().dome.is_empty());
    assert_eq!(s.hull(s.module(Slot::Core)), 600_000);
}

#[test]
fn raiders_disable_the_solar_array() {
    let mut s = station();
    let solar = s.module(Slot::Solar);
    let (_, centre, _) = s.state.module_position(solar).unwrap();
    let point = Vec2::from(centre) + Vec2::new(0.0, 2_400.0);
    let raid = s.fleet(raiders(), point, &[RAIDER], attacker());
    s.raid(raid, solar);
    s.set_hull(solar, 300);
    let out = idle(&mut s.state, 200);
    // 100 × 150% against structures (doc 15 §3.2, §12).
    assert!(economy_events(&out).contains(&EconomyEvent::ModuleDisabled { module: solar }));
    assert!(s.state.module(solar).unwrap().disabled);
    // Raiders are not commanders: nothing in the ledger (D-163).
    assert!(s.state.damage_ledger(s.holding).unwrap().tally.is_empty());
}

#[test]
fn a_peripheral_structure_is_disabled_at_zero_and_recovers_after_ten_quiet_minutes() {
    let mut s = station();
    let solar = s.module(Slot::Solar);
    let (_, centre, _) = s.state.module_position(solar).unwrap();
    let point = Vec2::from(centre) + Vec2::new(0.0, 2_400.0);
    let lancer = s.fleet(commander(2), point, &[LANCER], attacker());
    s.attack(2, lancer, solar);
    s.set_hull(solar, 200);
    let out = idle(&mut s.state, 120);
    assert!(economy_events(&out).contains(&EconomyEvent::ModuleDisabled { module: solar }));
    assert!(defence_events(&out).iter().any(|e| matches!(
        e,
        DefenceEvent::ModuleDamaged { module, by, amount: 200, .. }
            if *module == solar && *by == commander(2)
    )));
    // Supply loses the solar array's 620 MW.
    idle(&mut s.state, 10);
    assert_eq!(s.state.power(s.holding).unwrap().supply_kw, 520_000);
    // Disabled structures stay targetable (D-169); outside S6's window it stays disabled.
    idle(&mut s.state, 60);
    assert!(s.state.module(solar).unwrap().disabled);
    assert_eq!(s.hull(solar), 0);
    // The attacker stays inside the ops area for 5 more minutes, then leaves.
    idle(&mut s.state, 5 * 60 * TICKS_PER_SECOND);
    let ships = s.state.fleet(lancer).unwrap().ships;
    for ship in ships {
        s.state.entities.despawn(ship.sim_id());
    }
    s.state.entities.despawn(lancer.sim_id());
    // Ten minutes after the last hostile, not before (D-149), with 10% hull at least.
    idle(&mut s.state, (10 * 60 - 2) * TICKS_PER_SECOND);
    assert!(s.state.module(solar).unwrap().disabled);
    let out = idle(&mut s.state, 3 * TICKS_PER_SECOND);
    assert!(economy_events(&out).contains(&EconomyEvent::ModuleRestored { module: solar }));
    let view = s.state.module(solar).unwrap();
    assert!(!view.disabled);
    assert!(view.hp >= 2_000, "{view:?}");
}

#[test]
fn repair_brings_a_disabled_structure_back_at_once() {
    let mut s = station();
    let harvester = s.module(Slot::Deposit(0));
    let now = s.state.tick();
    economy::damage_module(&mut s.state, harvester, 20_000, now);
    s.disable(Slot::Deposit(0));
    let out = send(
        &mut s.state,
        1,
        Command::Economy(EconomyCommand::SetRepair {
            holding: s.holding,
            on: true,
        }),
    );
    assert_eq!(out.commands[0].outcome, Ok(()));
    // Within the second: in the holding's next economy slot, which may be this very tick.
    let mut outs = vec![out];
    outs.extend(idle(&mut s.state, TICKS_PER_SECOND));
    assert!(economy_events(&outs).contains(&EconomyEvent::ModuleRestored { module: harvester }));
    assert!(s.hull(harvester) >= 1_500);
}

#[test]
fn inner_modules_are_hit_only_through_an_open_sector_and_the_core_goes_down_once() {
    let mut s = station();
    // A turret due east, between the east sector and the core; it holds fire here.
    let turret = s.place(Slot::Inner(4), ModuleKind::Turret);
    s.turrets_hold();
    let core = s.module(Slot::Core);
    let point = s.at(0.0, 2_900.0);
    let lancers = s.fleet(commander(2), point, &[LANCER; 3], attacker());
    s.attack(2, lancers, turret);
    let out = idle(&mut s.state, 150);
    // Closed: the dome takes the torpedoes; the turret and the core are untouched.
    assert_eq!(sectors_hit(&out), BTreeSet::from([EAST]));
    assert_eq!(s.hull(turret), 30_000);
    // S6 opens the sector for a siege: the turret is hit and, at 0, destroyed (doc 15 §6).
    assert!(s.state.open_sector(s.holding, EAST));
    s.set_hull(turret, 100);
    let out = idle(&mut s.state, 150);
    assert!(economy_events(&out).iter().any(|e| matches!(
        e,
        EconomyEvent::ModuleDestroyed { module, kind: ModuleKind::Turret, .. } if *module == turret
    )));
    assert!(s.state.module(turret).is_none());
    assert!(
        !s.state.economy.holdings[&s.holding]
            .slots
            .contains_key(&Slot::Inner(4))
    );
    // Then the core, which goes down once.
    s.state.reset_siege_tally(s.holding, s.state.tick());
    s.attack(2, lancers, core);
    s.set_hull(core, 500);
    let out = idle(&mut s.state, 300);
    let downs: Vec<DefenceEvent> = defence_events(&out)
        .into_iter()
        .filter(|e| matches!(e, DefenceEvent::CoreDown { .. }))
        .collect();
    assert_eq!(downs.len(), 1, "{downs:?}");
    assert!(
        matches!(downs[0], DefenceEvent::CoreDown { holding, by, .. }
        if holding == s.holding && by == commander(2))
    );
    assert_eq!(s.hull(core), 0);
    // The siege tally: 500 core hull points from commander 2 (D-057).
    let ledger = s.state.damage_ledger(s.holding).unwrap();
    assert_eq!(ledger.tally.len(), 1);
    let (who, tally) = ledger.tally[0];
    assert_eq!((who, tally.core_milli), (CommanderId(2), 500 * MILLI));
    assert!(tally.total_milli >= tally.core_milli);
}

#[test]
fn an_immune_dome_blocks_but_takes_no_damage() {
    let mut s = station();
    s.state.set_dome_immune(s.holding, true);
    let point = s.at(0.0, 3_500.0);
    let lancers = s.fleet(commander(2), point, &[LANCER; 2], attacker());
    s.attack(2, lancers, s.holding.sim_id());
    let out = idle(&mut s.state, 150);
    assert_eq!(s.strength(EAST), FULL);
    assert!(defence_events(&out).is_empty());
    assert!(s.state.defence(s.holding).unwrap().immune);
    // Nothing got through to the core either.
    assert_eq!(s.hull(s.module(Slot::Core)), 600_000);
    s.state.set_dome_immune(s.holding, false);
    idle(&mut s.state, 100);
    assert!(s.strength(EAST) < FULL);
}

#[test]
fn an_attack_on_a_holding_closes_to_weapon_range_of_the_dome_edge() {
    let mut s = station();
    let point = s.at(0.0, 8_000.0);
    let fleet = s.fleet(commander(2), point, &[LANCER, LANCER, WARDEN], attacker());
    s.attack(2, fleet, s.holding.sim_id());
    let out = idle(&mut s.state, 700);
    let anchor = Vec2::from(s.state.fleet(fleet).unwrap().position);
    let distance = (anchor - s.centre).length();
    // 1.26 km of dome and about the 3 km class range (doc 15 §12): beyond turrets in inner
    // slots, which reach about 3.3 km from the centre (S5 plan §5).
    assert!((3_600.0..4_300.0).contains(&distance), "{distance}");
    assert_eq!(sectors_hit(&out), BTreeSet::from([EAST]));
}

#[test]
fn turrets_fire_outward_and_hold_on_stop() {
    let mut s = station();
    let turret = s.place(Slot::Inner(5), ModuleKind::Turret);
    let (_, at, _) = s.state.module_position(turret).unwrap();
    let vx = s.fleet(
        commander(2),
        Vec2::from(at) + Vec2::new(0.0, 2_000.0),
        &[VX],
        passive(),
    );
    idle(&mut s.state, 60);
    let view = s.state.defence(s.holding).unwrap();
    assert_eq!(
        (view.turrets, view.turrets_powered, view.turrets_firing),
        (1, 1, 1)
    );
    let hit = ship_hull(&s.state, vx);
    assert!(
        hit < 600 && (600 - hit).is_multiple_of(40),
        "turret hits of 40: {hit}"
    );
    // STOP: every turret holds fire; nothing more leaves them.
    assert_eq!(
        s.holding_order(HoldingOrder::TurretsHold { hold: true }),
        Ok(())
    );
    idle(&mut s.state, 40);
    let held = ship_hull(&s.state, vx);
    idle(&mut s.state, 100);
    assert_eq!(ship_hull(&s.state, vx), held);
    assert!(
        !s.state
            .projectiles
            .flying
            .iter()
            .any(|p| matches!(p.shooter, Shooter::Module(_)))
    );
    let view = s.state.defence(s.holding).unwrap();
    assert!(view.turrets_hold);
    assert_eq!(view.turrets_firing, 0);
    assert_eq!(
        s.holding_order(HoldingOrder::TurretsHold { hold: false }),
        Ok(())
    );
    idle(&mut s.state, 60);
    assert!(ship_hull(&s.state, vx) < held);
}

#[test]
fn an_unpowered_turret_holds_fire() {
    let mut state = stage1(7);
    // A second holding: a 120 MW core, three pylons (120 MW, served first) and a turret.
    let (_, home_system) = home(&state, 1);
    let site = state
        .galaxy()
        .system(home_system)
        .unwrap()
        .sites
        .iter()
        .copied()
        .find(|&s| state.holding_at(s).is_none())
        .unwrap();
    let holding = state.insert_holding(site, CommanderId(1), true).unwrap();
    economy::spawn_module(&mut state, holding, Slot::Core, ModuleKind::Core, None);
    let pylons: Vec<SimId> = (0..3)
        .map(|i| {
            economy::spawn_module(&mut state, holding, Slot::Pylon(i), ModuleKind::Pylon, None)
        })
        .collect();
    let turret = economy::spawn_module(
        &mut state,
        holding,
        Slot::Inner(0),
        ModuleKind::Turret,
        None,
    );
    let (_, at, _) = state.module_position(turret).unwrap();
    let vx = spawn_fleet(
        &mut state,
        commander(2),
        home_system,
        Vec2::from(at) + Vec2::new(0.0, 2_000.0),
        &[VX],
    );
    if let Some(f) = state.entities.get_mut::<Fleet>(vx.sim_id()) {
        f.rules_of_engagement = passive();
    }
    idle(&mut state, 100);
    assert!(!state.power(holding).unwrap().powered(turret));
    assert_eq!(ship_hull(&state, vx), 600);
    let view = state.defence(holding).unwrap();
    assert_eq!((view.turrets, view.turrets_powered), (1, 0));
    // With one pylon gone its 40 MW fit, and it fires.
    economy::destroy_module(&mut state, pylons[2], &mut Vec::new());
    idle(&mut state, 60);
    assert!(state.power(holding).unwrap().powered(turret));
    assert!(ship_hull(&state, vx) < 600);
}

#[test]
fn shield_surge_restores_a_fifth_and_honours_cooldown_and_open_sectors() {
    let mut s = station();
    s.set_strength(NORTH, 60_000 * MILLI);
    assert_eq!(s.ability(surge(NORTH)), Ok(()));
    // 20% of the maximum at once (D-147).
    assert_eq!(s.strength(NORTH), 90_000 * MILLI);
    let power = s.state.power(s.holding).unwrap();
    assert!(
        power
            .consumers
            .iter()
            .any(|c| matches!(c.consumer, Consumer::Ability(_)) && c.allocated_kw == 200_000)
    );
    // 90 s cooldown, from the use.
    assert_eq!(s.ability(surge(EAST)), Err(R::AbilityCooldown));
    idle(&mut s.state, 89 * TICKS_PER_SECOND);
    assert_eq!(s.check(1, surge(EAST)), Err(R::AbilityCooldown));
    idle(&mut s.state, TICKS_PER_SECOND);
    assert_eq!(s.check(1, surge(EAST)), Ok(()));
    // No sector 4; no surge on an open sector (doc 15 §5.3); not someone else's holding.
    assert_eq!(s.check(1, surge(4)), Err(R::NoSuchSector));
    assert!(s.state.open_sector(s.holding, WEST));
    assert_eq!(s.check(1, surge(WEST)), Err(R::SectorOpen));
    assert_eq!(s.check(2, surge(EAST)), Err(R::NotOwner));
    // A surge never goes beyond the maximum.
    s.set_strength(EAST, FULL - MILLI);
    assert_eq!(s.ability(surge(EAST)), Ok(()));
    assert_eq!(s.strength(EAST), FULL);
}

#[test]
fn abilities_need_their_power() {
    let mut s = station();
    // Solar off and four turrets: 520 MW against 160 MW of pylons and 160 MW of turrets
    // leaves exactly SHLD SURGE's 200 MW (D-128).
    s.disable(Slot::Solar);
    for i in 2..6 {
        s.place(Slot::Inner(i), ModuleKind::Turret);
    }
    idle(&mut s.state, 10);
    assert_eq!(s.check(1, surge(NORTH)), Ok(()));
    // FLAK SCREEN's 90 MW fit; then the surge no longer does.
    assert_eq!(s.ability(StationAbility::FlakScreen), Ok(()));
    assert_eq!(s.check(1, surge(NORTH)), Err(R::NoPower));
    // Once FLAK SCREEN ends (10 s) the surge fits again; FLAK SCREEN cools down for 60 s.
    idle(&mut s.state, 10 * TICKS_PER_SECOND + 10);
    assert_eq!(
        s.check(1, StationAbility::FlakScreen),
        Err(R::AbilityCooldown)
    );
    assert_eq!(s.ability(surge(NORTH)), Ok(()));
    // The surge's 200 MW leave nothing for FLAK SCREEN until it ends.
    idle(&mut s.state, 50 * TICKS_PER_SECOND);
    assert_eq!(s.check(1, StationAbility::FlakScreen), Ok(()));
}

#[test]
fn flak_screen_bursts_at_light_and_medium_hostiles_near_the_dome() {
    let mut s = station();
    // Ten VX 1.5 km out, one VX beyond reach and a heavy cruiser close in.
    let near: Vec<FleetId> = (0..10)
        .map(|i| {
            let point = s.at(f64::from(i) * 36.0, 1_500.0);
            s.fleet(commander(2), point, &[VX], passive())
        })
        .collect();
    let far = s.fleet(commander(2), s.at(200.0, 2_600.0), &[VX], passive());
    let cruiser = s.fleet(raiders(), s.at(300.0, 1_000.0), &[HSTL], passive());
    let launched = s.state.projectiles.next_id;
    assert_eq!(s.ability(StationAbility::FlakScreen), Ok(()));
    // At most 8 bursts a volley, the nearest first, ties by identity (D-148).
    assert_eq!(s.state.projectiles.next_id - launched, 8);
    let out = idle(&mut s.state, 12 * TICKS_PER_SECOND);
    // A burst a second for 10 s: ten volleys of 8.
    assert_eq!(s.state.projectiles.next_id - launched, 80);
    // Eight took 10 bursts of 30 × 200% (D-052) and are gone; the other two, the far VX and
    // the cruiser are untouched.
    let hulls: Vec<u32> = near.iter().map(|&f| ship_hull(&s.state, f)).collect();
    assert_eq!(hulls.iter().filter(|&&h| h == 0).count(), 8, "{hulls:?}");
    assert_eq!(hulls.iter().filter(|&&h| h == 600).count(), 2, "{hulls:?}");
    assert_eq!(ship_hull(&s.state, far), 600);
    assert_eq!(ship_hull(&s.state, cruiser), 30_000);
    assert!(
        out.iter()
            .flat_map(|o| o.combat_events.iter())
            .any(|e| matches!(e, crate::CombatEvent::ShipDamaged { amount: 60, .. }))
    );
    assert!(
        s.state
            .defence(s.holding)
            .unwrap()
            .flak_screen_until
            .is_none()
    );
}

#[test]
fn return_fire_covers_attacks_on_the_commanders_structures() {
    let mut s = station();
    // D-145: a VX on return fire near its own dome engages the LANCER attacking the dome,
    // not a quiet hostile next to it.
    let spot = s.at(-10.0, 2_400.0);
    let defender = s.fleet(commander(1), spot, &[VX], RulesOfEngagement::ReturnFire);
    let quiet = s.fleet(
        commander(3),
        spot + Vec2::new(0.0, -900.0),
        &[VX],
        passive(),
    );
    let lancer = s.fleet(commander(2), s.at(0.0, 3_500.0), &[LANCER], attacker());
    s.attack(2, lancer, s.holding.sim_id());
    let ship = first_ship(&s.state, defender);
    let target = |state: &UniverseState| {
        state
            .entities
            .get::<Weapons>(ship.sim_id())
            .and_then(|w| w.slots.first().and_then(WeaponSlot::ship_target))
    };
    idle(&mut s.state, 30);
    assert_eq!(target(&s.state), None, "nobody has damaged anything yet");
    let out = idle(&mut s.state, 90);
    assert_eq!(sectors_hit(&out), BTreeSet::from([EAST]));
    assert_eq!(target(&s.state), Some(first_ship(&s.state, lancer)));
    assert_eq!(ship_hull(&s.state, quiet), 600);
    assert_eq!(ship_hull(&s.state, defender), 600);
}

#[test]
fn attack_orders_on_structures_are_validated() {
    let mut s = station();
    let lancer = s.fleet(commander(2), s.at(90.0, 5_000.0), &[LANCER], attacker());
    let own = s.fleet(commander(1), s.at(90.0, 5_000.0), &[LANCER], attacker());
    let check = |n: u32, fleet: FleetId, target: SimId| {
        validate_command(
            &s.state,
            commander(n),
            &order(fleet, FleetOrder::Attack { target }),
        )
    };
    let solar = s.module(Slot::Solar);
    let pylon = s.module(Slot::Pylon(NORTH));
    // A module can be targeted, and so can the holding; a pylon cannot (D-053); one's own
    // structures are friendly.
    assert_eq!(check(2, lancer, solar), Ok(()));
    assert_eq!(check(2, lancer, s.holding.sim_id()), Ok(()));
    assert_eq!(check(2, lancer, pylon), Err(R::UnknownTarget));
    assert_eq!(check(1, own, solar), Err(R::FriendlyTarget));
}

#[test]
fn every_station_rejection_reason_is_produced() {
    let mut seen = BTreeSet::new();
    let mut s = station();
    let mut expect = |s: &Station, ability: StationAbility, reason: R| {
        assert_eq!(s.check(1, ability), Err(reason), "{ability:?}");
        seen.insert(reason);
    };
    expect(&s, surge(9), R::NoSuchSector);
    assert!(s.state.open_sector(s.holding, SOUTH));
    expect(&s, surge(SOUTH), R::SectorOpen);
    s.state.close_sectors(s.holding);
    assert_eq!(s.ability(surge(SOUTH)), Ok(()));
    expect(&s, surge(SOUTH), R::AbilityCooldown);
    // FLAK SCREEN without its power: SHLD SURGE's 200 MW, pylons and four turrets on 520 MW.
    s.disable(Slot::Solar);
    for i in 2..6 {
        s.place(Slot::Inner(i), ModuleKind::Turret);
    }
    idle(&mut s.state, 10);
    expect(&s, StationAbility::FlakScreen, R::NoPower);
    assert_eq!(seen, defence_reasons());
    // An outpost has no dome, turrets or abilities.
    let mut state = stage1(7);
    let (_, system) = home(&state, 1);
    let site = state
        .galaxy()
        .system(system)
        .unwrap()
        .sites
        .iter()
        .copied()
        .find(|&site| state.holding_at(site).is_none())
        .unwrap();
    let outpost = state.insert_holding(site, CommanderId(1), false).unwrap();
    for order in [
        HoldingOrder::TurretsHold { hold: true },
        HoldingOrder::Ability {
            ability: StationAbility::FlakScreen,
        },
    ] {
        assert_eq!(
            validate_command(
                &state,
                commander(1),
                &Command::Holding {
                    holding: outpost,
                    order
                }
            ),
            Err(R::Outpost)
        );
    }
}

#[test]
fn a_station_battle_survives_a_snapshot_round_trip() {
    let mut s = station();
    s.place(Slot::Inner(4), ModuleKind::Turret);
    let attackers = s.fleet(
        commander(2),
        s.at(0.0, 2_900.0),
        &[LANCER, LANCER, LANCER, WARDEN],
        RulesOfEngagement::WeaponsFree,
    );
    let intruder = s.fleet(commander(2), s.at(CLEAR_SOUTH, 1_500.0), &[VX], passive());
    s.attack(2, attackers, s.holding.sim_id());
    idle(&mut s.state, 90);
    assert_eq!(s.ability(StationAbility::FlakScreen), Ok(()));
    idle(&mut s.state, 12);
    assert!(
        !s.state.projectiles.flying.is_empty(),
        "projectiles in flight"
    );
    assert!(s.strength(EAST) < FULL, "a sector regenerating");
    assert!(!s.state.defence.turrets.is_empty(), "a turret locking");
    let bytes = encode(&s.state).unwrap();
    let mut restored = decode(&bytes).unwrap();
    assert_eq!(restored, s.state);
    assert_eq!(encode(&restored).unwrap(), bytes);
    idle(&mut s.state, 400);
    idle(&mut restored, 400);
    assert_eq!(
        state_hash(&s.state).unwrap(),
        state_hash(&restored).unwrap()
    );
    assert!(
        s.state.economy_problems().is_empty(),
        "{:?}",
        s.state.economy_problems()
    );
    assert!(ship_hull(&s.state, intruder) < 600);
}
