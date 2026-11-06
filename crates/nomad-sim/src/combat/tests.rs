//! Tests of ship combat against S5 plan §8 (S5a).

use nomad_types::{FactionId, FleetOrder, Owner, RulesOfEngagement};

use super::engagement::taking_fire;
use super::fire::lead;
use super::projectile::{Burst, Projectile, ProjectileId};
use super::{CombatEvent, Damage, Hit, WeaponSlot, Weapons, apply};
use crate::fleet::movement::DT;
use crate::fleet::{Fleet, FleetEvent, FleetId, MotionMode, ShipClass, ShipId, Shroud};
use crate::galaxy::SystemId;
use crate::geometry::Vec2;
use crate::snapshot::{decode, encode, state_hash};
use crate::state::UniverseState;
use crate::testkit::{commander, home, idle, order, send, site_position, spawn_fleet, stage1};
use crate::time::{Tick, Ticks};

const VX: ShipClass = ShipClass::Vx;
const LANCER: ShipClass = ShipClass::Lancer;
const WARDEN: ShipClass = ShipClass::Warden;
const MULE: ShipClass = ShipClass::Mule;
const RAIDER: ShipClass = ShipClass::RaiderStrikeCraft;
const HSTL: ShipClass = ShipClass::HeavyCruiser;

fn npc() -> Owner {
    Owner::Npc(FactionId(1))
}

/// A stage-1 Universe and a point in commander 1's home system well clear of the mother
/// station, so no obstacle is near.
fn arena() -> (UniverseState, SystemId, Vec2) {
    let state = stage1(5);
    let (_, system) = home(&state, 1);
    let base = site_position(&state, system, 0) + Vec2::new(8_000.0, 0.0);
    (state, system, base)
}

fn place(
    state: &mut UniverseState,
    owner: Owner,
    system: SystemId,
    at: Vec2,
    classes: &[ShipClass],
    roe: RulesOfEngagement,
) -> FleetId {
    let fleet = spawn_fleet(state, owner, system, at, classes);
    if let Some(f) = state.entities.get_mut::<Fleet>(fleet.sim_id()) {
        f.rules_of_engagement = roe;
    }
    fleet
}

fn ships(state: &UniverseState, fleet: FleetId) -> Vec<ShipId> {
    state.fleet(fleet).map(|f| f.ships).unwrap_or_default()
}

fn slots(state: &UniverseState, ship: ShipId) -> Vec<WeaponSlot> {
    state
        .entities
        .get::<Weapons>(ship.sim_id())
        .map(|w| w.slots)
        .unwrap_or_default()
}

fn hull(state: &UniverseState, ship: ShipId) -> u32 {
    state.ship_hull(ship).map_or(0, |(left, _)| left)
}

fn combat_events(outputs: &[crate::step::TickOutputs]) -> Vec<CombatEvent> {
    outputs
        .iter()
        .flat_map(|o| o.combat_events.iter().copied())
        .collect()
}

#[test]
fn lock_reaches_full_at_the_lock_time_and_resets_on_a_target_change() {
    let (mut state, system, base) = arena();
    let vx = place(
        &mut state,
        commander(1),
        system,
        base,
        &[VX],
        RulesOfEngagement::WeaponsFree,
    );
    let first = place(
        &mut state,
        commander(2),
        system,
        base + Vec2::new(1_000.0, 0.0),
        &[MULE],
        RulesOfEngagement::HoldFire,
    );
    let shooter = ships(&state, vx)[0];
    let target = ships(&state, first)[0];
    // Lock rises one tick's worth per tick: 2 s for a VX (doc 15 §12).
    idle(&mut state, 1);
    assert_eq!(slots(&state, shooter)[0].target, Some(target));
    assert_eq!(slots(&state, shooter)[0].lock_ms, 100);
    idle(&mut state, 18);
    assert_eq!(slots(&state, shooter)[0].lock_ms, 1_900);
    idle(&mut state, 1);
    assert_eq!(slots(&state, shooter)[0].lock_ms, 2_000);
    idle(&mut state, 5);
    assert_eq!(slots(&state, shooter)[0].lock_ms, 2_000);

    // An Attack order on another hostile in range switches the target and resets the lock.
    let second = place(
        &mut state,
        commander(3),
        system,
        base + Vec2::new(0.0, 1_200.0),
        &[MULE],
        RulesOfEngagement::HoldFire,
    );
    send(
        &mut state,
        1,
        order(
            vx,
            FleetOrder::Attack {
                target: second.sim_id(),
            },
        ),
    );
    let new_target = ships(&state, second)[0];
    assert_eq!(slots(&state, shooter)[0].target, Some(new_target));
    assert_eq!(slots(&state, shooter)[0].lock_ms, 100);
}

#[test]
fn a_ranging_laser_makes_allied_lock_rise_half_as_fast_again() {
    let (mut state, system, base) = arena();
    let target = place(
        &mut state,
        commander(1),
        system,
        base,
        &[MULE],
        RulesOfEngagement::HoldFire,
    );
    let cruiser = place(
        &mut state,
        npc(),
        system,
        base + Vec2::new(3_000.0, 0.0),
        &[HSTL],
        RulesOfEngagement::WeaponsFree,
    );
    let raider = place(
        &mut state,
        npc(),
        system,
        base + Vec2::new(0.0, 2_000.0),
        &[RAIDER],
        RulesOfEngagement::WeaponsFree,
    );
    let raider_ship = ships(&state, raider)[0];
    let target_ship = ships(&state, target)[0];
    // Tick 1: the cruiser takes the target; its laser designates it from tick 2.
    idle(&mut state, 1);
    assert_eq!(
        slots(&state, ships(&state, cruiser)[0])[0].target,
        Some(target_ship)
    );
    assert_eq!(slots(&state, raider_ship)[0].lock_ms, 100);
    idle(&mut state, 10);
    // 100 + 10 × 150 (doc 15 §4: 50% faster).
    assert_eq!(slots(&state, raider_ship)[0].lock_ms, 1_600);
}

#[test]
fn lead_aim_meets_a_target_flying_straight() {
    let from = Vec2::new(0.0, 0.0);
    let at = Vec2::new(1_000.0, 0.0);
    let velocity = Vec2::new(0.0, 200.0);
    let speed = 800.0;
    let (angle, distance) = lead(from, at, velocity, speed);
    let flight = distance / speed;
    let projectile = Vec2::from_angle(angle) * speed;
    // Step both a tick at a time and find the closest approach.
    let mut closest = f64::MAX;
    let mut t = 0.0;
    while t <= flight + 2.0 * DT {
        let gap = (from + projectile * t) - (at + velocity * t);
        closest = closest.min(gap.length());
        t += DT;
    }
    // Within one tick's travel of the projectile, and inside a VX's 90 m hit circle.
    assert!(closest < 90.0, "closest approach {closest} m");
    // Exactly on the predicted point at the predicted time.
    let gap = (from + projectile * flight) - (at + velocity * flight);
    assert!(gap.length() < 1e-6);
    // A target at rest is aimed at directly.
    let (angle, distance) = lead(from, at, Vec2::ZERO, speed);
    assert!(angle.abs() < 1e-12 && (distance - 1_000.0).abs() < 1e-9);
}

#[test]
fn spread_follows_lock_and_shots_stay_inside_the_cone() {
    let (mut state, system, base) = arena();
    let lancers = place(
        &mut state,
        commander(1),
        system,
        base,
        &[LANCER],
        RulesOfEngagement::WeaponsFree,
    );
    place(
        &mut state,
        commander(2),
        system,
        base + Vec2::new(2_500.0, 0.0),
        &[MULE],
        RulesOfEngagement::HoldFire,
    );
    let shooter = ships(&state, lancers)[0];
    // The torpedo fires at once, at no lock: inside the 30 mrad cone around the target.
    idle(&mut state, 1);
    let p = &state.projectiles.flying[0];
    assert_eq!(p.shooter, shooter);
    let to_target = (base + Vec2::new(2_500.0, 0.0)) - p.position;
    let off = crate::geometry::angle_difference(to_target.angle(), p.velocity.angle()).abs();
    assert!(off <= 0.015 + 1e-12, "{off} rad off");
    assert!((p.velocity.length() - 350.0).abs() < 1e-9);
}

#[test]
fn a_weapon_fires_at_its_interval_without_bursts_after_idling() {
    let (mut state, system, base) = arena();
    let vx = place(
        &mut state,
        commander(1),
        system,
        base,
        &[VX],
        RulesOfEngagement::WeaponsFree,
    );
    place(
        &mut state,
        commander(2),
        system,
        base + Vec2::new(1_000.0, 0.0),
        &[MULE],
        RulesOfEngagement::HoldFire,
    );
    let shooter = ships(&state, vx)[0];
    let count = |state: &UniverseState| state.projectiles.next_id;
    // 4 shots a second (doc 15 §12): 40 in 10 s, the first on the first tick.
    idle(&mut state, 100);
    assert_eq!(count(&state), 40);
    // Steady fire is 250 ms apart on average: 300, 200, 300, 200 ms … at 100 ms ticks.
    // Hold fire for 10 s, then weapons free again: no burst of saved-up shots.
    if let Some(f) = state.entities.get_mut::<Fleet>(vx.sim_id()) {
        f.rules_of_engagement = RulesOfEngagement::HoldFire;
    }
    idle(&mut state, 100);
    let before = count(&state);
    assert_eq!(slots(&state, shooter).first().and_then(|s| s.target), None);
    if let Some(f) = state.entities.get_mut::<Fleet>(vx.sim_id()) {
        f.rules_of_engagement = RulesOfEngagement::WeaponsFree;
    }
    idle(&mut state, 1);
    assert_eq!(count(&state) - before, 1);
    idle(&mut state, 9);
    assert_eq!(count(&state) - before, 4);
}

#[test]
fn flak_damages_only_hostile_light_and_medium_ships_within_its_burst() {
    let (mut state, system, base) = arena();
    let hold = RulesOfEngagement::HoldFire;
    let warden = place(&mut state, commander(1), system, base, &[WARDEN], hold);
    let burst_at = base + Vec2::new(1_500.0, 0.0);
    let light = place(&mut state, commander(2), system, burst_at, &[VX], hold);
    let medium = place(
        &mut state,
        commander(2),
        system,
        burst_at + Vec2::new(0.0, 100.0),
        &[MULE],
        hold,
    );
    let heavy = place(
        &mut state,
        npc(),
        system,
        burst_at + Vec2::new(0.0, -100.0),
        &[HSTL],
        hold,
    );
    let friend = place(
        &mut state,
        commander(1),
        system,
        burst_at + Vec2::new(-100.0, 0.0),
        &[VX],
        hold,
    );
    let outside = place(
        &mut state,
        commander(2),
        system,
        burst_at + Vec2::new(0.0, 400.0),
        &[VX],
        hold,
    );
    let shooter = ships(&state, warden)[0];
    state.projectiles.launch(Projectile {
        id: ProjectileId(0),
        system,
        owner: commander(1),
        shooter,
        class: WARDEN,
        weapon: 2,
        position: base,
        velocity: Vec2::new(800.0, 0.0),
        remaining_m: 2_000.0,
        burst: Some(Burst {
            ticks: 1,
            at: burst_at,
        }),
    });
    idle(&mut state, 1);
    // 30 × 200% against Light, 30 × 25% = 7 against Medium (D-052).
    assert_eq!(hull(&state, ships(&state, light)[0]), 600 - 60);
    assert_eq!(hull(&state, ships(&state, medium)[0]), 2_500 - 7);
    assert_eq!(hull(&state, ships(&state, heavy)[0]), 30_000);
    assert_eq!(hull(&state, ships(&state, friend)[0]), 600);
    assert_eq!(hull(&state, ships(&state, outside)[0]), 600);
    assert!(state.projectiles.flying.is_empty());
}

#[test]
fn projectiles_pass_through_friendly_and_shrouded_ships() {
    let (mut state, system, base) = arena();
    let hold = RulesOfEngagement::HoldFire;
    let shooter_fleet = place(&mut state, commander(1), system, base, &[VX], hold);
    let friend = place(
        &mut state,
        commander(1),
        system,
        base + Vec2::new(300.0, 0.0),
        &[VX],
        hold,
    );
    let shrouded = place(
        &mut state,
        commander(2),
        system,
        base + Vec2::new(600.0, 0.0),
        &[VX],
        hold,
    );
    let target = place(
        &mut state,
        commander(2),
        system,
        base + Vec2::new(900.0, 0.0),
        &[VX],
        hold,
    );
    let shrouded_ship = ships(&state, shrouded)[0];
    state.entities.insert(
        shrouded_ship.sim_id(),
        Shroud {
            until: Tick::new(1_000_000),
        },
    );
    state.projectiles.launch(Projectile {
        id: ProjectileId(0),
        system,
        owner: commander(1),
        shooter: ships(&state, shooter_fleet)[0],
        class: VX,
        weapon: 0,
        position: base,
        velocity: Vec2::new(800.0, 0.0),
        remaining_m: 1_500.0,
        burst: None,
    });
    idle(&mut state, 20);
    assert_eq!(hull(&state, ships(&state, friend)[0]), 600);
    assert_eq!(hull(&state, shrouded_ship), 600);
    assert_eq!(hull(&state, ships(&state, target)[0]), 600 - 8);
}

#[test]
fn shrouded_ships_neither_fire_nor_are_targeted() {
    let (mut state, system, base) = arena();
    let a = place(
        &mut state,
        commander(1),
        system,
        base,
        &[VX],
        RulesOfEngagement::WeaponsFree,
    );
    let b = place(
        &mut state,
        commander(2),
        system,
        base + Vec2::new(1_000.0, 0.0),
        &[VX],
        RulesOfEngagement::WeaponsFree,
    );
    let until = Tick::new(1_000_000);
    for ship in ships(&state, b) {
        state.entities.insert(ship.sim_id(), Shroud { until });
    }
    idle(&mut state, 30);
    assert_eq!(state.projectiles.next_id, 0, "nobody fired");
    assert!(slots(&state, ships(&state, a)[0]).is_empty());
}

#[test]
fn a_cruising_fleet_does_not_fire() {
    // Far from any site, with no hostile within 20 km, a fleet cruises, and a cruising fleet
    // may not fire (doc 15 §3.3).
    let (mut state, system, _) = arena();
    let far = Vec2::new(0.0, 0.0);
    let fleet = place(
        &mut state,
        commander(1),
        system,
        far,
        &[VX],
        RulesOfEngagement::WeaponsFree,
    );
    assert_eq!(state.fleet(fleet).map(|f| f.mode), Some(MotionMode::Cruise));
    assert!(super::fire::may_fire(&state, fleet).is_none());
    // Engaged, it may.
    state.set_fleet_engaged(fleet, true);
    assert!(super::fire::may_fire(&state, fleet).is_some());
}

#[test]
fn damage_applies_in_target_then_projectile_order() {
    let (mut state, system, base) = arena();
    let hold = RulesOfEngagement::HoldFire;
    let victim = place(&mut state, commander(1), system, base, &[VX], hold);
    let attacker_b = place(
        &mut state,
        commander(2),
        system,
        base + Vec2::new(1_000.0, 0.0),
        &[VX],
        hold,
    );
    let attacker_c = place(
        &mut state,
        commander(3),
        system,
        base + Vec2::new(0.0, 1_000.0),
        &[VX],
        hold,
    );
    let ship = ships(&state, victim)[0];
    state.entities.insert(ship.sim_id(), Damage { taken: 590 });
    let hit = |projectile: u64, owner: u32, shooter: FleetId, state: &UniverseState| Hit {
        target: ship,
        projectile: ProjectileId(projectile),
        amount: 20,
        owner: commander(owner),
        shooter: ships(state, shooter)[0],
    };
    // Listed out of order: projectile 3 (commander 3) lands before projectile 5.
    let hits = vec![hit(5, 2, attacker_b, &state), hit(3, 3, attacker_c, &state)];
    let mut events = Vec::new();
    apply(&mut state, Tick::new(1), hits, &mut events);
    assert!(events.contains(&CombatEvent::ShipDestroyed {
        ship,
        class: VX,
        owner: commander(1),
        by: commander(3),
    }));
    assert!(events.contains(&CombatEvent::FleetDestroyed {
        fleet: victim,
        owner: commander(1),
    }));
    // Only the 10 points that remained count as damage, all from commander 3.
    assert!(events.contains(&CombatEvent::ShipDamaged {
        ship,
        fleet: victim,
        by: commander(3),
        amount: 10,
    }));
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, CombatEvent::ShipDamaged { by, .. } if *by == commander(2)))
    );
    assert!(state.ship(ship).is_none());
    assert!(state.fleet(victim).is_none());
}

#[test]
fn engagement_ends_thirty_seconds_after_the_last_trigger() {
    let (mut state, system, _) = arena();
    let fleet = place(
        &mut state,
        commander(1),
        system,
        Vec2::ZERO,
        &[VX],
        RulesOfEngagement::WeaponsFree,
    );
    let attacker = place(
        &mut state,
        commander(2),
        system,
        Vec2::new(60_000.0, 0.0),
        &[VX],
        RulesOfEngagement::HoldFire,
    );
    let shooter = ships(&state, attacker)[0];
    idle(&mut state, 10);
    assert_eq!(state.fleet(fleet).map(|f| f.engaged), Some(false));
    // Taking fire at tick 10 (D-114).
    let at = state.tick();
    taking_fire(&mut state, at, fleet, shooter);
    assert_eq!(
        state.fleet(fleet).map(|f| f.engaged_until),
        Some(Some(at.after(Ticks::from_secs(30))))
    );
    idle(&mut state, 299);
    assert_eq!(state.fleet(fleet).map(|f| f.engaged), Some(true));
    assert_eq!(
        state.fleet(fleet).map(|f| f.mode),
        Some(MotionMode::Tactical)
    );
    idle(&mut state, 1);
    assert_eq!(state.tick(), at.after(Ticks::from_secs(30)));
    assert_eq!(state.fleet(fleet).map(|f| f.engaged), Some(false));
    assert_eq!(state.fleet(fleet).map(|f| f.mode), Some(MotionMode::Cruise));
    // The attacker record expired with the window.
    assert!(super::engagement::recent_attackers(&state, fleet).is_empty());
}

#[test]
fn a_hostile_within_twenty_km_engages_a_fleet() {
    let (mut state, system, _) = arena();
    let fleet = place(
        &mut state,
        commander(1),
        system,
        Vec2::ZERO,
        &[VX],
        RulesOfEngagement::WeaponsFree,
    );
    let friend = place(
        &mut state,
        commander(1),
        system,
        Vec2::new(5_000.0, 0.0),
        &[MULE],
        RulesOfEngagement::HoldFire,
    );
    idle(&mut state, 1);
    assert_eq!(
        state.fleet(fleet).map(|f| f.engaged),
        Some(false),
        "friends do not engage"
    );
    let hostile = place(
        &mut state,
        commander(2),
        system,
        Vec2::new(19_000.0, 0.0),
        &[MULE],
        RulesOfEngagement::HoldFire,
    );
    idle(&mut state, 1);
    assert_eq!(state.fleet(fleet).map(|f| f.engaged), Some(true));
    assert_eq!(state.fleet(hostile).map(|f| f.engaged), Some(true));
    assert_eq!(state.fleet(friend).map(|f| f.engaged), Some(true));
}

#[test]
fn auto_targeting_prefers_the_highest_multiplier_then_the_nearest() {
    let (mut state, system, base) = arena();
    let hold = RulesOfEngagement::HoldFire;
    let warden = place(
        &mut state,
        commander(1),
        system,
        base,
        &[WARDEN],
        RulesOfEngagement::WeaponsFree,
    );
    let near_vx = place(
        &mut state,
        commander(2),
        system,
        base + Vec2::new(1_000.0, 0.0),
        &[VX],
        hold,
    );
    let far_vx = place(
        &mut state,
        commander(2),
        system,
        base + Vec2::new(0.0, 1_500.0),
        &[VX],
        hold,
    );
    let mule = place(
        &mut state,
        commander(2),
        system,
        base + Vec2::new(-2_500.0, 0.0),
        &[MULE],
        hold,
    );
    idle(&mut state, 1);
    let s = slots(&state, ships(&state, warden)[0]);
    // Batteries: Medium 100% beats Light 50%, although the MULE is farthest.
    assert_eq!(s[0].target, Some(ships(&state, mule)[0]));
    assert_eq!(s[1].target, Some(ships(&state, mule)[0]));
    // Flak: Light 200%, and of the two VX the nearer.
    assert_eq!(s[2].target, Some(ships(&state, near_vx)[0]));
    let _ = far_vx;
}

#[test]
fn return_fire_engages_only_recent_attackers() {
    let (mut state, system, base) = arena();
    let hold = RulesOfEngagement::HoldFire;
    let fleet = place(
        &mut state,
        commander(1),
        system,
        base,
        &[VX],
        RulesOfEngagement::ReturnFire,
    );
    let quiet = place(
        &mut state,
        commander(2),
        system,
        base + Vec2::new(800.0, 0.0),
        &[VX],
        hold,
    );
    let attacker = place(
        &mut state,
        commander(3),
        system,
        base + Vec2::new(0.0, 1_200.0),
        &[VX],
        hold,
    );
    let shooter = ships(&state, fleet)[0];
    idle(&mut state, 20);
    assert!(slots(&state, shooter).is_empty(), "nobody attacked yet");
    let now = state.tick();
    let attacker_ship = ships(&state, attacker)[0];
    taking_fire(&mut state, now, fleet, attacker_ship);
    idle(&mut state, 1);
    // The nearer VX did nothing, so return fire takes the attacker (D-145).
    assert_eq!(
        slots(&state, shooter)[0].target,
        Some(ships(&state, attacker)[0])
    );
    let _ = quiet;
}

#[test]
fn hold_fire_never_fires() {
    let (mut state, system, base) = arena();
    place(
        &mut state,
        commander(1),
        system,
        base,
        &[VX, WARDEN, LANCER],
        RulesOfEngagement::HoldFire,
    );
    place(
        &mut state,
        commander(2),
        system,
        base + Vec2::new(1_000.0, 0.0),
        &[MULE],
        RulesOfEngagement::HoldFire,
    );
    idle(&mut state, 100);
    assert_eq!(state.projectiles.next_id, 0);
}

#[test]
fn destroyed_ships_leave_and_an_attack_order_completes() {
    let (mut state, system, base) = arena();
    let attackers = place(
        &mut state,
        commander(1),
        system,
        base,
        &[VX; 6],
        RulesOfEngagement::WeaponsFree,
    );
    let victim = place(
        &mut state,
        commander(2),
        system,
        base + Vec2::new(1_200.0, 0.0),
        &[VX],
        RulesOfEngagement::HoldFire,
    );
    let victim_ship = ships(&state, victim)[0];
    send(
        &mut state,
        1,
        order(
            attackers,
            FleetOrder::Attack {
                target: victim.sim_id(),
            },
        ),
    );
    let outputs = idle(&mut state, 200);
    let combat = combat_events(&outputs);
    assert!(combat.contains(&CombatEvent::ShipDestroyed {
        ship: victim_ship,
        class: VX,
        owner: commander(2),
        by: commander(1),
    }));
    assert!(combat.contains(&CombatEvent::FleetDestroyed {
        fleet: victim,
        owner: commander(2),
    }));
    let fleet_events: Vec<FleetEvent> = outputs
        .iter()
        .flat_map(|o| o.fleet_events.iter().copied())
        .collect();
    assert!(fleet_events.contains(&FleetEvent::OrderCompleted { fleet: attackers }));
    assert!(state.fleet(victim).is_none());
    assert!(
        state.fleet_problems().is_empty(),
        "{:?}",
        state.fleet_problems()
    );
}

#[test]
fn a_battle_survives_a_snapshot_round_trip() {
    let (mut state, system, base) = arena();
    let a = place(
        &mut state,
        commander(1),
        system,
        base,
        &[VX, VX, LANCER, WARDEN],
        RulesOfEngagement::WeaponsFree,
    );
    let b = place(
        &mut state,
        commander(2),
        system,
        base + Vec2::new(2_500.0, 0.0),
        &[VX, LANCER, LANCER, WARDEN],
        RulesOfEngagement::WeaponsFree,
    );
    send(
        &mut state,
        1,
        order(a, FleetOrder::Attack { target: b.sim_id() }),
    );
    idle(&mut state, 37);
    assert!(
        !state.projectiles.flying.is_empty(),
        "projectiles in flight"
    );
    assert!(
        state.entities.ids_with::<Weapons>().len() > 1,
        "locks building"
    );
    let bytes = encode(&state).unwrap();
    let mut restored = decode(&bytes).unwrap();
    assert_eq!(restored, state);
    assert_eq!(encode(&restored).unwrap(), bytes);
    idle(&mut state, 400);
    idle(&mut restored, 400);
    assert_eq!(state_hash(&state).unwrap(), state_hash(&restored).unwrap());
}
