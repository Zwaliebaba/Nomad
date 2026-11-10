//! The raider director in the step: its commands go through the outbox and the validator as
//! `Owner::Npc`, raids continue during catch-up, a group's raid runs its course, and
//! snapshots during transit and during the raid clock restore identically (A3 plan §9
//! "Determinism" and "Director tests").

use nomad_ai::{DirectorEvent, Layer, Withdrawal};
use nomad_types::{Command, FactionId, FleetOrder, Owner, Queueing};

use super::RaidEvent;
use crate::ai::gate::submit_for_test;
use crate::command::{CommandRejection, validate_command};
use crate::fleet::{FleetId, Place, ShipClass};
use crate::galaxy::{GateId, SystemId, SystemKind};
use crate::geometry::Vec2;
use crate::holding::HoldingId;
use crate::id::CommanderId;
use crate::input::{OperatorInput, TickInputs};
use crate::snapshot::{decode, encode, state_hash};
use crate::state::UniverseState;
use crate::step::{TickOutputs, step};
use crate::testkit::{idle, spawn_fleet, stage1};
use crate::time::Tick;

const RAIDERS: Owner = Owner::Npc(FactionId(1));
const VICTIM: CommanderId = CommanderId(3);

/// A launched raid on commander 3 in the hub, where it has an outpost and a MULE in space
/// 6 km from the gate the raid arrives at. Returns the state just after the launch tick, the
/// group, the outpost and the MULE's fleet.
struct HubRaid {
    state: UniverseState,
    group: FleetId,
    outpost: HoldingId,
    mule: FleetId,
}

fn hub(state: &UniverseState) -> SystemId {
    state
        .galaxy()
        .systems()
        .find(|(_, s)| s.kind == SystemKind::Hub)
        .map(|(id, _)| id)
        .expect("stage 1 has a hub")
}

/// The hub's end of the lane to the haven.
fn hub_gate(state: &UniverseState) -> GateId {
    let haven = state.raid_factions()[0].haven;
    let gate = state.galaxy().system(haven).expect("haven").gates[0];
    let lane = state.galaxy().gate(gate).expect("gate").lanes[0];
    state
        .galaxy()
        .lane(lane)
        .and_then(|l| l.other_end(gate))
        .expect("the haven's lane has a far end")
}

fn hub_raid(seed: u64) -> HubRaid {
    let mut state = stage1(seed);
    let hub = hub(&state);
    let site = state.galaxy().system(hub).expect("hub").sites[0];
    let outpost = state
        .insert_holding(site, VICTIM, false)
        .expect("free site");
    crate::economy::spawn_outpost_core(&mut state, outpost);
    let gate_at = Vec2::from(
        state
            .galaxy()
            .gate(hub_gate(&state))
            .expect("gate")
            .position,
    );
    let towards_star = Vec2::ZERO - gate_at;
    let mule_at = gate_at + towards_star * (6_000.0 / towards_star.length());
    let mule = spawn_fleet(
        &mut state,
        Owner::Commander(VICTIM),
        hub,
        mule_at,
        &[ShipClass::Mule],
    );
    // The launch as phase 2 fires it, then the group's record pointed at commander 3 in the
    // hub before the director first sees it: the draw is the rule tests' business.
    let at = state.raid_factions()[0].next_launch;
    state.tick = Tick::new(at.get() - 1);
    while state.timers.pop_due(at).is_some() {}
    let mut events = Vec::new();
    super::launch(&mut state, at, at, FactionId(1), &mut events);
    let group = events
        .iter()
        .find_map(|e| match *e {
            RaidEvent::Launched { fleet, .. } => Some(fleet),
            _ => None,
        })
        .expect("a launch");
    let record = state
        .raids
        .factions
        .values_mut()
        .find_map(|f| f.groups.get_mut(&group))
        .expect("the group's record");
    record.target = VICTIM;
    record.system = hub;
    step(&mut state, &TickInputs::default());
    HubRaid {
        state,
        group,
        outpost,
        mule,
    }
}

/// The director's commands applied, by order kind.
fn applied(state: &UniverseState) -> u64 {
    state
        .ai_counters()
        .get(&(RAIDERS, Layer::Director))
        .map_or(0, |c| c.applied)
}

fn rejected(state: &UniverseState) -> u64 {
    state
        .ai_counters()
        .get(&(RAIDERS, Layer::Director))
        .map_or(0, |c| c.rejected_total() + c.dropped_total())
}

/// Skips to just before the group's lane transit ends and steps into the arrival.
fn skip_transit(state: &mut UniverseState, group: FleetId) -> TickOutputs {
    let Some(Place::InTransit { arrive, .. }) = state.fleet(group).map(|f| f.place) else {
        panic!("the group is not in a lane: {:?}", state.fleet(group));
    };
    assert!(
        state.timers.next_deadline().is_none_or(|t| t > arrive),
        "no other timer comes first"
    );
    state.tick = Tick::new(arrive.get() - 1);
    step(state, &TickInputs::default())
}

/// Steps until `done` holds, at most `limit` ticks.
fn until(
    state: &mut UniverseState,
    limit: u64,
    mut done: impl FnMut(&UniverseState, &TickOutputs) -> bool,
) -> Vec<TickOutputs> {
    let mut outs = Vec::new();
    for _ in 0..limit {
        let out = step(state, &TickInputs::default());
        let stop = done(state, &out);
        outs.push(out);
        if stop {
            return outs;
        }
    }
    panic!("not done within {limit} ticks");
}

fn in_transit(state: &UniverseState, group: FleetId) -> bool {
    matches!(
        state.fleet(group).map(|f| f.place),
        Some(Place::InTransit { .. })
    )
}

#[test]
fn the_director_jumps_attacks_and_passes_the_validator_during_catch_up_too() {
    let HubRaid {
        mut state,
        group,
        mule,
        ..
    } = hub_raid(7);
    // Catch-up from now on: no new raid, but this one carries on (D-047).
    step(
        &mut state,
        &TickInputs {
            operator: vec![OperatorInput::CatchUpStart],
            commanders: vec![],
        },
    );
    assert!(state.catching_up());
    until(&mut state, 200, |s, _| in_transit(s, group));
    assert_eq!(applied(&state), 1, "the jump");
    let out = skip_transit(&mut state, group);
    assert_eq!(
        state.fleet(group).map(|f| f.place),
        Some(Place::InSystem(hub(&state)))
    );
    let arrived = out.tick;
    // The clock starts at the end of the shroud, and the MULE comes first.
    let outs = until(&mut state, 300, |s, _| {
        s.fleet(group).and_then(|f| f.order).is_some_and(
            |o| matches!(o, FleetOrder::Attack { target } if state_has_ship_of(s, mule, target)),
        )
    });
    let clock = outs
        .iter()
        .find(|o| {
            o.raid_events.contains(&RaidEvent::Director {
                faction: FactionId(1),
                event: DirectorEvent::ClockStarted { fleet: group },
            })
        })
        .map(|o| o.tick)
        .expect("the clock started");
    assert!(clock.get() > arrived.get());
    let started = state
        .ai_memory()
        .faction(FactionId(1))
        .and_then(|m| m.clock_started(group));
    assert_eq!(
        started,
        Some(arrived.after(crate::time::Ticks::from_secs(10)))
    );
    assert!(state.ai_waiting(RAIDERS) <= 1);
    assert_eq!(rejected(&state), 0, "{:?}", state.ai_counters());
    assert!(applied(&state) >= 2);
}

/// Whether `target` is a ship of `fleet`.
fn state_has_ship_of(state: &UniverseState, fleet: FleetId, target: nomad_types::SimId) -> bool {
    state
        .fleet(fleet)
        .is_some_and(|f| f.ships.iter().any(|s| s.sim_id() == target))
}

#[test]
fn an_npc_command_is_rejected_alike_from_the_outbox_and_the_validator() {
    let HubRaid {
        mut state, group, ..
    } = hub_raid(3);
    let haven = state.raid_factions()[0].haven;
    // A jump to where it already is.
    let command = Command::Fleet {
        fleet: group,
        order: FleetOrder::Jump { system: haven },
        queue: Queueing::Replace,
        retreat: false,
    };
    assert_eq!(
        validate_command(&state, RAIDERS, &command),
        Err(CommandRejection::AlreadyThere)
    );
    let now = state.tick();
    submit_for_test(&mut state, RAIDERS, Layer::Director, now, now, command);
    idle(&mut state, 8);
    let counters = &state.ai_counters()[&(RAIDERS, Layer::Director)];
    assert_eq!(
        counters.rejected.get(&CommandRejection::AlreadyThere),
        Some(&1)
    );
}

#[test]
fn snapshots_in_transit_and_during_the_raid_clock_restore_identically() {
    let HubRaid {
        mut state, group, ..
    } = hub_raid(11);
    until(&mut state, 200, |s, _| in_transit(s, group));
    let check = |state: &UniverseState| {
        let mut live = state.clone();
        let mut restored = decode(&encode(state).expect("encodes")).expect("decodes");
        assert_eq!(restored, live);
        idle(&mut live, 60);
        idle(&mut restored, 60);
        assert_eq!(
            state_hash(&live).expect("hashes"),
            state_hash(&restored).expect("hashes")
        );
    };
    // In the lane.
    check(&state);
    skip_transit(&mut state, group);
    // Under the shroud, then 20 s into the clock.
    check(&state);
    idle(&mut state, 300);
    check(&state);
}

#[test]
fn the_same_seed_gives_the_same_raid_and_the_same_hash() {
    let run = || {
        let HubRaid {
            mut state, group, ..
        } = hub_raid(13);
        let mut events = Vec::new();
        for out in until(&mut state, 200, |s, _| in_transit(s, group)) {
            events.extend(out.raid_events);
        }
        events.extend(skip_transit(&mut state, group).raid_events);
        for out in idle(&mut state, 400) {
            events.extend(out.raid_events);
        }
        (events, state_hash(&state).expect("hashes"))
    };
    let (first, hash) = run();
    assert!(!first.is_empty());
    assert_eq!(run(), (first, hash));
}

#[test]
fn a_raid_runs_its_course_and_the_group_is_removed_at_the_haven() {
    let HubRaid {
        mut state,
        group,
        outpost,
        mule,
    } = hub_raid(5);
    let haven = state.raid_factions()[0].haven;
    until(&mut state, 200, |s, _| in_transit(s, group));
    skip_transit(&mut state, group);
    let mut lost_mule = false;
    let mut withdrew = None;
    until(&mut state, 15_000, |s, out| {
        lost_mule |= s.fleet(mule).is_none();
        for e in &out.raid_events {
            if let RaidEvent::Director {
                event: DirectorEvent::Withdrew { fleet, reason },
                ..
            } = *e
                && fleet == group
            {
                withdrew = Some((out.tick, reason));
            }
        }
        withdrew.is_some() && in_transit(s, group)
    });
    // The MULE (2,500 hull, 150 a second from D-313's four strike craft) did not survive,
    // nor did the outpost: it starts at 30,000 and grows as fast as one raider wears it
    // (D-224), so four take a net 112.5 a second, a fresh core in under 5 of the 15 minutes
    // (the tuning item of A3 plan §5; at D-220's floor of two it survived).
    assert!(lost_mule, "the MULE was destroyed");
    assert!(
        state.holding(outpost).is_none(),
        "the outpost was destroyed"
    );
    let (when, reason) = withdrew.expect("withdrew");
    assert_eq!(reason, Withdrawal::Clock);
    let clock = state
        .ai_memory()
        .faction(FactionId(1))
        .and_then(|m| m.clock_started(group))
        .expect("the clock started");
    // 15 minutes (D-313), at the group's next evaluation (2 Hz).
    assert!((9_000..9_005).contains(&(when.get() - clock.get())));
    let out = skip_transit(&mut state, group);
    assert!(out.raid_events.iter().any(|e| matches!(
        e,
        RaidEvent::Returned { fleet, ships: 4, .. } if *fleet == group
    )));
    assert!(state.fleet(group).is_none());
    assert!(state.raid_factions()[0].groups.is_empty());
    assert_eq!(rejected(&state), 0, "{:?}", state.ai_counters());
    // The director forgets it.
    step(&mut state, &TickInputs::default());
    assert!(state.ai_memory().faction(FactionId(1)).is_none());
    assert_eq!(state.fleet(group).map(|f| f.place), None);
    assert!(state.fleet_ids().iter().all(|&f| {
        state
            .fleet(f)
            .is_none_or(|v| v.place != Place::InSystem(haven))
    }));
}
