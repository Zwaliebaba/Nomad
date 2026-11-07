//! Season tests (S8 plan §8 "Done when").

use std::collections::{BTreeMap, BTreeSet};

use nomad_types::{
    ControlState, DepositValue, ModuleKind, SeasonEndReason, SeasonResult, Slot, Standing,
};

use super::standings::{Tally, rank, tally};
use super::*;
use crate::announce::AnnouncementKind as K;
use crate::economy::{EconomyEvent, spawn_module};
use crate::galaxy::{SiteId, SystemKind, generate_stage1};
use crate::holding::{Holding, HoldingId, majority_threshold};
use crate::id::CommanderId;
use crate::input::{CommanderInput, CommanderInputKind, TickInputs};
use crate::snapshot::{decode, encode, state_hash};
use crate::state::SeasonConfig;
use crate::step::{TickOutputs, step};
use crate::testkit::{COMMANDERS, idle};
use crate::time::Tick;

const C1: CommanderId = CommanderId(1);
const C2: CommanderId = CommanderId(2);
const C3: CommanderId = CommanderId(3);
const C4: CommanderId = CommanderId(4);

/// A stage-1 Universe whose season ends at `final_tick`.
fn season(seed: u64, final_tick: u64) -> UniverseState {
    generate_stage1(
        SeasonConfig {
            final_tick: Tick::new(final_tick),
            ..SeasonConfig::new(seed, 0)
        },
        COMMANDERS,
    )
    .expect("stage 1 generates")
}

fn tally_of(systems: u32, holdings: u32, value_milli: u64) -> Tally {
    Tally {
        systems,
        holdings,
        deposit_value: DepositValue::from_milli(value_milli),
    }
}

/// Free sites of `system`, in identity order.
fn free_sites(state: &UniverseState, system: SystemId) -> Vec<SiteId> {
    state
        .galaxy()
        .system(system)
        .expect("system exists")
        .sites
        .iter()
        .copied()
        .filter(|&site| state.holding_at(site).is_none())
        .collect()
}

/// Completed holdings `commander` owns in `system`.
fn owned_in(state: &UniverseState, commander: CommanderId, system: SystemId) -> usize {
    state
        .holdings()
        .filter(|(_, h)| h.owner == commander && h.core_complete)
        .filter(|(_, h)| state.galaxy().site(h.site).map(|s| s.system) == Some(system))
        .count()
}

/// Gives `commander` completed holdings in `system` until it holds `count` of them there.
fn hold(state: &mut UniverseState, commander: CommanderId, system: SystemId, count: usize) {
    let missing = count.saturating_sub(owned_in(state, commander, system));
    for site in free_sites(state, system).into_iter().take(missing) {
        state.insert_holding(site, commander, true).unwrap();
    }
    assert_eq!(owned_in(state, commander, system), count);
}

/// Gives `commander` a majority of `system`'s sites as completed holdings (D-041).
fn control(state: &mut UniverseState, commander: CommanderId, system: SystemId) {
    let sites = state.galaxy().system(system).unwrap().sites.len();
    hold(state, commander, system, majority_threshold(sites));
}

/// The claimable systems, homes of the other commanders last, so a commander can take
/// majorities in the first ones.
fn claimable_systems(state: &UniverseState, commander: CommanderId) -> Vec<SystemId> {
    let mut systems: Vec<(bool, SystemId)> = state
        .galaxy()
        .systems()
        .filter(|(_, s)| is_claimable(s.kind))
        .map(|(id, s)| {
            let foreign_home = s.kind == SystemKind::Home
                && s.sites.iter().any(|&site| {
                    state
                        .holding_at(site)
                        .is_some_and(|(_, h)| h.owner != commander)
                });
            (foreign_home, id)
        })
        .collect();
    systems.sort();
    systems.into_iter().map(|(_, id)| id).collect()
}

/// An outpost of `commander` on `site` whose core completes after `seconds` of its holding's
/// economy seconds (D-054, with a short deployment for the test).
fn outpost(
    state: &mut UniverseState,
    commander: CommanderId,
    site: SiteId,
    seconds: u64,
) -> HoldingId {
    let holding = state.insert_holding(site, commander, false).unwrap();
    spawn_module(
        state,
        holding,
        Slot::Core,
        ModuleKind::Core,
        Some(seconds * 1_000),
    );
    holding
}

fn announced(outputs: &[TickOutputs]) -> Vec<(Tick, K)> {
    outputs
        .iter()
        .flat_map(|o| &o.announcements)
        .filter(|a| {
            matches!(
                a.kind,
                K::ControlChanged { .. }
                    | K::StandingsChanged { .. }
                    | K::NearEarlyWin { .. }
                    | K::NearEarlyWinEnded { .. }
                    | K::SeasonEnded { .. }
            )
        })
        .map(|a| {
            assert_eq!(
                a.to,
                Recipients::Public,
                "season announcements are public (D-075)"
            );
            (a.tick, a.kind.clone())
        })
        .collect()
}

fn assert_consistent(state: &UniverseState) {
    let problems = state.season_problems();
    assert!(problems.is_empty(), "{problems:?}");
}

// ------------------------------------------------------------------- ranking

#[test]
fn every_tie_break_applies_in_order() {
    let none = Standings::default();
    let at = Tick::new(5);
    // Systems first, even against more holdings and value.
    let tallies = BTreeMap::from([(C1, tally_of(1, 9, 9_000)), (C2, tally_of(2, 4, 1_000))]);
    let ranked = rank(&tallies, &none, at);
    assert_eq!(ranked.ranked()[0].commander, C2);
    // Then holdings.
    let tallies = BTreeMap::from([(C1, tally_of(2, 5, 1_000)), (C2, tally_of(2, 4, 9_000))]);
    assert_eq!(rank(&tallies, &none, at).ranked()[0].commander, C1);
    // Then deposit value (D-066).
    let tallies = BTreeMap::from([(C1, tally_of(2, 5, 1_000)), (C2, tally_of(2, 5, 1_500))]);
    assert_eq!(rank(&tallies, &none, at).ranked()[0].commander, C2);
    // Then since: who reached the tied position first (D-193). C2 has been at it since
    // tick 3, C1 changes now.
    let before = rank(
        &BTreeMap::from([(C1, tally_of(1, 4, 1_000)), (C2, tally_of(2, 5, 1_500))]),
        &none,
        Tick::new(3),
    );
    let tallies = BTreeMap::from([(C1, tally_of(2, 5, 1_500)), (C2, tally_of(2, 5, 1_500))]);
    let ranked = rank(&tallies, &before, Tick::new(9));
    let order: Vec<(CommanderId, u32, Tick)> = ranked
        .iter()
        .map(|s| (s.commander, s.rank, s.since))
        .collect();
    assert_eq!(order, [(C2, 1, Tick::new(3)), (C1, 2, Tick::new(9))]);
    // Equal on all four: a shared rank (D-194), and the next rank counts both.
    let tallies = BTreeMap::from([
        (C1, tally_of(2, 5, 1_500)),
        (C2, tally_of(1, 5, 1_500)),
        (C3, tally_of(2, 5, 1_500)),
    ]);
    let ranked = rank(&tallies, &none, at);
    let ranks: Vec<(CommanderId, u32)> = ranked.iter().map(|s| (s.commander, s.rank)).collect();
    assert_eq!(ranks, [(C1, 1), (C3, 1), (C2, 3)]);
    assert_eq!(
        ranked.leaders().map(|s| s.commander).collect::<Vec<_>>(),
        [C1, C3]
    );
    assert_eq!(ranked.leader_holdings(), 5);
}

#[test]
fn since_is_kept_while_the_triple_is_unchanged() {
    let first = rank(
        &BTreeMap::from([(C1, tally_of(1, 3, 2_000)), (C2, tally_of(0, 1, 1_000))]),
        &Standings::default(),
        Tick::new(10),
    );
    // C2 changes, C1 does not.
    let second = rank(
        &BTreeMap::from([(C1, tally_of(1, 3, 2_000)), (C2, tally_of(0, 2, 2_000))]),
        &first,
        Tick::new(20),
    );
    assert_eq!(second.of(C1).unwrap().since, Tick::new(10));
    assert_eq!(second.of(C2).unwrap().since, Tick::new(20));
    // Back to an earlier triple: unchanged only since now (D-193).
    let third = rank(
        &BTreeMap::from([(C1, tally_of(1, 3, 2_000)), (C2, tally_of(0, 1, 1_000))]),
        &second,
        Tick::new(30),
    );
    assert_eq!(third.of(C2).unwrap().since, Tick::new(30));
}

/// A small deterministic generator for the property tests.
struct XorShift(u64);

impl XorShift {
    fn below(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % n
    }
}

#[test]
fn ranking_matches_a_brute_force_sort() {
    let mut rng = XorShift(0x5eed_0008);
    for _ in 0..500 {
        let commanders = 1 + rng.below(8);
        let mut previous_tallies = BTreeMap::new();
        let mut tallies = BTreeMap::new();
        for c in 1..=commanders {
            let id = CommanderId(u32::try_from(c).unwrap());
            // Small ranges, so ties on every key are common.
            let random = |rng: &mut XorShift| {
                tally_of(
                    u32::try_from(rng.below(3)).unwrap(),
                    u32::try_from(rng.below(3)).unwrap(),
                    rng.below(3) * 500,
                )
            };
            let earlier = random(&mut rng);
            previous_tallies.insert(id, earlier);
            let now = if rng.below(2) == 0 {
                earlier
            } else {
                random(&mut rng)
            };
            tallies.insert(id, now);
        }
        let previous = rank(&previous_tallies, &Standings::default(), Tick::new(1));
        let ranked = rank(&tallies, &previous, Tick::new(2));
        assert_eq!(ranked.len(), tallies.len());
        let key = |s: &Standing| {
            (
                std::cmp::Reverse(s.systems),
                std::cmp::Reverse(s.holdings),
                std::cmp::Reverse(s.deposit_value),
                s.since,
            )
        };
        for s in ranked.iter() {
            let t = tallies[&s.commander];
            assert_eq!(
                (s.systems, s.holdings, s.deposit_value),
                (t.systems, t.holdings, t.deposit_value)
            );
            let expected_since = if previous_tallies[&s.commander] == t {
                Tick::new(1)
            } else {
                Tick::new(2)
            };
            assert_eq!(s.since, expected_since);
            // Brute force: 1 + everyone strictly better.
            let better = ranked.iter().filter(|o| key(o) < key(s)).count();
            assert_eq!(s.rank as usize, better + 1, "{s:?} in {ranked:?}");
        }
        // Best first.
        assert!(ranked.ranked().windows(2).all(|w| key(&w[0]) <= key(&w[1])));
    }
}

#[test]
fn the_tally_does_not_depend_on_insertion_order() {
    let state = season(3, 1_000);
    let mut rng = XorShift(0x0bad_cafe);
    // A random set of holdings on random sites, completed or not.
    let sites: Vec<SiteId> = state
        .galaxy()
        .sites()
        .filter(|(_, s)| {
            state
                .galaxy()
                .system(s.system)
                .is_some_and(|sys| is_claimable(sys.kind))
        })
        .map(|(id, _)| id)
        .collect();
    let mut holdings: Vec<Holding> = Vec::new();
    for &site in &sites {
        if rng.below(3) == 0 {
            continue;
        }
        holdings.push(Holding {
            site,
            owner: COMMANDERS[usize::try_from(rng.below(4)).unwrap()],
            core_complete: rng.below(4) > 0,
            mother_station: false,
        });
    }
    let everyone: BTreeSet<CommanderId> = COMMANDERS.into_iter().collect();
    let count = |order: &[Holding]| {
        let map: BTreeMap<HoldingId, Holding> = order
            .iter()
            .enumerate()
            .map(|(i, h)| {
                (
                    HoldingId(crate::SimId::from_raw(1_000 + i as u64)),
                    h.clone(),
                )
            })
            .collect();
        let (control, tallies) = tally(state.galaxy(), &map, &everyone);
        (control, rank(&tallies, &Standings::default(), Tick::new(7)))
    };
    let reference = count(&holdings);
    let mut shuffled = holdings.clone();
    for _ in 0..20 {
        for i in (1..shuffled.len()).rev() {
            let j = usize::try_from(rng.below(i as u64 + 1)).unwrap();
            shuffled.swap(i, j);
        }
        assert_eq!(count(&shuffled), reference);
    }
    // Outposts count for nothing (D-041).
    let completed: Vec<Holding> = holdings
        .iter()
        .filter(|h| h.core_complete)
        .cloned()
        .collect();
    assert_eq!(count(&completed), reference);
}

// ------------------------------------------------------------------- standings

#[test]
fn every_commander_has_a_standing_from_tick_0() {
    let state = season(7, 1_000);
    let standings = state.standings();
    assert_eq!(standings.len(), 4);
    for s in standings.iter() {
        // The mother station, on a site with one standard ore field (D-098).
        assert_eq!(
            (s.systems, s.holdings, s.deposit_value, s.since),
            (0, 1, DepositValue::from_milli(1_000), Tick::ZERO)
        );
        // Equal on everything: all four share the lead (D-194).
        assert_eq!(s.rank, 1);
    }
    assert_eq!(state.claimable_systems(), 9);
    assert_eq!(state.early_win_threshold(), 6);
    assert!(
        state
            .control_states()
            .values()
            .all(|c| *c == ControlState::Open)
    );
    assert_eq!(state.standings_now(), *standings);
    assert_consistent(&state);
}

#[test]
fn outposts_count_for_nothing_and_havens_cannot_be_held() {
    let mut state = season(7, 100_000);
    let system = claimable_systems(&state, C1)[1];
    for site in free_sites(&state, system).into_iter().take(4) {
        state.insert_holding(site, C1, false).unwrap();
    }
    let outputs = idle(&mut state, 1);
    assert!(announced(&outputs).is_empty());
    assert_eq!(state.control_state(system), Some(ControlState::Open));
    assert_eq!(state.standings().of(C1).unwrap().holdings, 1);
    let haven = state
        .galaxy()
        .systems()
        .find(|(_, s)| s.kind == SystemKind::RaiderHaven)
        .map(|(id, _)| id)
        .unwrap();
    assert_eq!(state.control_state(haven), None);
    assert!(!state.control_states().contains_key(&haven));
    assert_consistent(&state);
}

#[test]
fn control_and_standings_changes_are_announced_exactly_when_the_recount_differs() {
    let mut state = season(7, 100_000);
    let system = claimable_systems(&state, C1)[1];
    // A dirty tick that changes nothing announces nothing.
    mark_dirty(&mut state);
    assert!(announced(&idle(&mut state, 1)).is_empty());

    // A first completed holding: the standings change, control does not.
    hold(&mut state, C1, system, 1);
    let outputs = idle(&mut state, 1);
    let tick = outputs[0].tick;
    let kinds = announced(&outputs);
    assert_eq!(kinds.len(), 1, "{kinds:?}");
    assert!(matches!(&kinds[0], (t, K::StandingsChanged { standings })
        if *t == tick && standings[0].commander == C1 && standings[0].holdings == 2 && standings[0].since == tick));
    // A second owner makes the system Contested.
    hold(&mut state, C2, system, 1);
    let outputs = idle(&mut state, 1);
    let kinds = announced(&outputs);
    assert_eq!(
        kinds[0].1,
        K::ControlChanged {
            system,
            from: ControlState::Open,
            to: ControlState::Contested
        }
    );
    assert!(matches!(kinds[1].1, K::StandingsChanged { .. }));
    assert_eq!(kinds.len(), 2);
    // A majority makes it Controlled; the cache answers at once.
    control(&mut state, C1, system);
    assert_eq!(
        state.control_state(system),
        Some(ControlState::Controlled(C1)),
        "recounted while dirty"
    );
    let outputs = idle(&mut state, 1);
    assert_eq!(
        announced(&outputs)[0].1,
        K::ControlChanged {
            system,
            from: ControlState::Contested,
            to: ControlState::Controlled(C1)
        }
    );
    let s = state.standings().of(C1).unwrap();
    assert_eq!((s.systems, s.rank), (1, 1));
    // Ticks without changes announce nothing.
    assert!(announced(&idle(&mut state, 20)).is_empty());
    assert_consistent(&state);
}

#[test]
fn an_outpost_completing_recounts_the_standings_on_that_tick() {
    let mut state = season(7, 100_000);
    let system = claimable_systems(&state, C3)[2];
    let site = free_sites(&state, system)[0];
    let holding = outpost(&mut state, C3, site, 2);
    let outputs = idle(&mut state, 40);
    let completed = outputs
        .iter()
        .find(|o| {
            o.economy_events
                .contains(&EconomyEvent::HoldingCompleted { holding })
        })
        .expect("the outpost completes")
        .tick;
    let kinds = announced(&outputs);
    assert_eq!(kinds.len(), 1);
    assert_eq!(kinds[0].0, completed);
    assert_eq!(state.standings().of(C3).unwrap().since, completed);
    assert_eq!(state.standings().ranked()[0].commander, C3);
    assert_consistent(&state);
}

// ------------------------------------------------------------------- the result

/// Gives C2 majorities in the first `n` claimable systems it can take.
fn c2_controls(state: &mut UniverseState, n: usize) -> Vec<SystemId> {
    let systems: Vec<SystemId> = claimable_systems(state, C2).into_iter().take(n).collect();
    for &system in &systems {
        control(state, C2, system);
    }
    systems
}

#[test]
fn five_of_nine_warns_and_six_of_nine_wins_on_that_tick() {
    let mut state = season(7, 1_000_000);
    let systems = c2_controls(&mut state, 5);
    let outputs = idle(&mut state, 1);
    let kinds = announced(&outputs);
    assert!(kinds.contains(&(
        outputs[0].tick,
        K::NearEarlyWin {
            commander: C2,
            systems: 5,
            threshold: 6
        }
    )));
    assert!(state.season_result().is_none(), "5 of 9 does not win");
    assert_eq!(state.near_early_win(), &BTreeSet::from([C2]));

    // The sixth majority comes from an outpost completing (D-054, D-192).
    let sixth = claimable_systems(&state, C2)
        .into_iter()
        .find(|s| !systems.contains(s))
        .unwrap();
    let sites = state.galaxy().system(sixth).unwrap().sites.len();
    hold(&mut state, C2, sixth, majority_threshold(sites) - 1);
    let site = free_sites(&state, sixth)[0];
    let holding = outpost(&mut state, C2, site, 3);
    let outputs = idle(&mut state, 60);
    let completed = outputs
        .iter()
        .find(|o| {
            o.economy_events
                .contains(&EconomyEvent::HoldingCompleted { holding })
        })
        .unwrap()
        .tick;
    let result = state.season_result().cloned().expect("an early win");
    assert_eq!(
        result,
        SeasonResult {
            tick: completed,
            reason: SeasonEndReason::EarlyWin,
            winners: vec![C2]
        }
    );
    assert_eq!(state.final_tick(), completed);
    assert_eq!(state.tick(), completed, "no tick follows the final one");
    let last = announced(&outputs);
    assert!(
        matches!(&last.last().unwrap().1, K::SeasonEnded { result: r, standings }
        if *r == result && standings[0].commander == C2 && standings[0].systems == 6)
    );
    // The decisive tick announces the result, not a near win.
    assert!(
        !last
            .iter()
            .any(|(_, k)| matches!(k, K::NearEarlyWinEnded { .. }))
    );
    assert_consistent(&state);
}

#[test]
fn the_near_win_warning_clears_when_the_count_drops_back() {
    let mut state = season(7, 1_000_000);
    let systems = c2_controls(&mut state, 5);
    idle(&mut state, 1);
    assert_eq!(state.near_early_win(), &BTreeSet::from([C2]));
    // A capture stand-in until S6b: one of C2's holdings in a controlled system changes hands.
    let (lost, _) = state
        .holdings()
        .find(|(_, h)| {
            h.owner == C2
                && !h.mother_station
                && state.galaxy().site(h.site).map(|s| s.system) == Some(systems[4])
        })
        .unwrap();
    state.holdings.get_mut(&lost).unwrap().owner = C4;
    mark_dirty(&mut state);
    let outputs = idle(&mut state, 1);
    let kinds = announced(&outputs);
    assert!(kinds.contains(&(outputs[0].tick, K::NearEarlyWinEnded { commander: C2 })));
    assert!(state.near_early_win().is_empty());
    assert_eq!(state.standings().of(C2).unwrap().systems, 4);
    assert_consistent(&state);
}

#[test]
fn the_scheduled_end_fixes_the_result_at_the_final_tick() {
    // Nobody ahead: all four tie on everything, so all four win jointly (D-194).
    let mut state = season(7, 50);
    let outputs = idle(&mut state, 50);
    let result = state.season_result().cloned().expect("the season ended");
    assert_eq!(
        result,
        SeasonResult {
            tick: Tick::new(50),
            reason: SeasonEndReason::FinalTick,
            winners: COMMANDERS.to_vec()
        }
    );
    assert_eq!(announced(&outputs).len(), 1);
    assert!(state.season_over());

    // One more completed holding than everyone else wins alone.
    let mut state = season(7, 50);
    let system = claimable_systems(&state, C4)[0];
    hold(&mut state, C4, system, 1);
    idle(&mut state, 50);
    assert_eq!(state.season_result().unwrap().winners, [C4]);
    assert_eq!(state.final_tick(), Tick::new(50));
}

#[test]
fn step_after_the_result_changes_nothing() {
    let mut state = season(7, 20);
    idle(&mut state, 20);
    assert!(state.season_over());
    let hash = state_hash(&state).unwrap();
    let inputs = TickInputs {
        operator: vec![],
        commanders: vec![CommanderInput {
            commander: C1,
            client_seq: 1,
            kind: CommanderInputKind::Connected,
        }],
    };
    let outputs = step(&mut state, &inputs);
    assert_eq!(outputs.tick, Tick::new(20));
    assert_eq!(outputs.inputs_rejected, 1);
    assert!(outputs.announcements.is_empty() && outputs.commands.is_empty());
    idle(&mut state, 100);
    assert_eq!(state.tick(), Tick::new(20));
    assert_eq!(state_hash(&state).unwrap(), hash);
    assert!(state.connected().is_empty());
}

#[test]
fn snapshots_before_the_final_tick_and_before_an_early_win_restore_to_the_same_result() {
    // One tick before the scheduled final tick.
    let mut straight = season(9, 300);
    let system = claimable_systems(&straight, C1)[0];
    hold(&mut straight, C1, system, 2);
    idle(&mut straight, 299);
    let mut restored = decode(&encode(&straight).unwrap()).unwrap();
    idle(&mut straight, 1);
    idle(&mut restored, 1);
    assert_eq!(restored.season_result(), straight.season_result());
    assert_eq!(
        state_hash(&restored).unwrap(),
        state_hash(&straight).unwrap()
    );

    // During an outpost's last seconds before an early win.
    let mut straight = season(9, 1_000_000);
    c2_controls(&mut straight, 5);
    let sixth = claimable_systems(&straight, C2)[5];
    let sites = straight.galaxy().system(sixth).unwrap().sites.len();
    hold(&mut straight, C2, sixth, majority_threshold(sites) - 1);
    let site = free_sites(&straight, sixth)[0];
    outpost(&mut straight, C2, site, 60);
    idle(&mut straight, 550);
    assert!(!straight.season_over());
    let mut restored = decode(&encode(&straight).unwrap()).unwrap();
    idle(&mut straight, 100);
    idle(&mut restored, 100);
    let result = straight.season_result().cloned().expect("an early win");
    assert_eq!(result.reason, SeasonEndReason::EarlyWin);
    assert_eq!(restored.season_result(), Some(&result));
    assert_eq!(
        state_hash(&restored).unwrap(),
        state_hash(&straight).unwrap()
    );
}
