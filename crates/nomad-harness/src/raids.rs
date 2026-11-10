//! `--raids`: NPC raids on a stage-1 Universe, measured as D-225 asks (A3 plan §8 task 5,
//! §5 "Raids happen"; D-081, D-219 to D-225, D-047).
//!
//! The four stage-1 commanders are AI commanders whose Deputies run; until A2 their
//! Strategists decide nothing, so a script claims for them as `--season` does: every
//! `--claim-hours` (default 12) each places an outpost on a free site, which gives raids
//! targets away from the home systems. The raider haven launches on its own seeded timer,
//! and the raider director steers every group. A catch-up segment, `--catch-up-hours` long
//! (default 6, 0 for none) from hour `--catch-up-from` (default 30), checks that no raid
//! starts during catch-up (D-047). The run lasts `--days` (default 7).
//!
//! It reports, per D-225:
//!
//! 1. every launch interval of each haven, which must lie within 2–4 h;
//! 2. raids per commander and per day, and the 6 h spacing, which must hold;
//! 3. the share of raids that reached their system and destroyed or disabled something;
//! 4. damage raiders did to domes and to modules inside them, which must be zero;
//! 5. launches during catch-up, which must be none;
//!
//! and the raid outcomes: withdrawals by reason, groups lost, the raiders' and the targets'
//! losses, outposts destroyed by group size (A3 plan §5's tuning item), the director's
//! command counters and the tick time. A failed must-hold check is an error. Items 2 and 3
//! are α measures of AI play: they are reported, marked, and not errors here.
//!
//! What a raid destroyed or disabled is told from the outputs: ships and outpost cores the
//! faction's weapons destroyed, and peripheral modules disabled on a tick the faction's
//! weapons hit them, each counted for the faction's raid on that commander.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::io::Write;
use std::time::Instant;

use nomad_sim::galaxy::{STAGE1_COMMANDERS, generate_stage1_with_rules};
use nomad_sim::snapshot::{decode, encode, state_hash};
use nomad_sim::{
    CombatEvent, CommanderId, DefenceEvent, DirectorEvent, EconomyEvent, FactionId, FleetId, Layer,
    ModuleKind, OperatorInput, Owner, RaidEvent, Rules, SeasonConfig, ShipClass, Slot, SystemId,
    TICKS_PER_SECOND, Tick, TickInputs, UniverseState, Withdrawal, scenario, step,
};

const COMMANDERS: [CommanderId; STAGE1_COMMANDERS] = [
    CommanderId(1),
    CommanderId(2),
    CommanderId(3),
    CommanderId(4),
];
const MINUTE: u64 = 60 * TICKS_PER_SECOND;
const HOUR: u64 = 60 * MINUTE;
const DAY: u64 = 24 * HOUR;

/// What `--raids` runs.
pub struct RaidRun {
    /// The season seed.
    pub seed: u64,
    /// How long, in days.
    pub days: u64,
    /// Hours between one commander's scripted claims; 0 for none.
    pub claim_hours: u64,
    /// The hour the catch-up segment starts.
    pub catch_up_from: u64,
    /// How long it lasts, in hours; 0 for none.
    pub catch_up_hours: u64,
    /// Restore from a snapshot at this tick.
    pub restore_at: Option<u64>,
    /// The rules (D-104).
    pub rules: Rules,
}

fn when(tick: Tick) -> String {
    let t = tick.get();
    format!(
        "day {:>2} {:02}:{:02}",
        t / DAY,
        t % DAY / HOUR,
        t % HOUR / MINUTE
    )
}

fn hours(ticks: u64) -> String {
    format!("{:.2} h", ticks as f64 / HOUR as f64)
}

/// How a raid ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum End {
    Returned { ships: u32 },
    Destroyed,
}

/// One raid as the run saw it.
#[derive(Clone, Debug)]
struct Raid {
    faction: FactionId,
    fleet: FleetId,
    target: CommanderId,
    system: SystemId,
    launched: Tick,
    fleet_value: u64,
    strike_craft: u32,
    heavy_cruisers: u32,
    reached: Option<Tick>,
    withdrew: Option<Withdrawal>,
    ended: Option<(Tick, End)>,
    /// The target's ships destroyed, its peripheral modules disabled and its outposts
    /// destroyed by this raid.
    ships_killed: u32,
    value_killed: u64,
    modules_disabled: u32,
    outposts_destroyed: u32,
    ships_lost: u32,
}

impl Raid {
    fn hits(&self) -> u32 {
        self.ships_killed + self.modules_disabled + self.outposts_destroyed
    }
}

/// A ship class's value as D-220 counts it, from the rules.
fn class_value(rules: &Rules, class: ShipClass) -> u64 {
    let raids = &rules.ai.raids;
    match class {
        ShipClass::RaiderStrikeCraft => raids.strike_craft_value,
        ShipClass::HeavyCruiser => raids.heavy_cruiser_value,
        _ => rules
            .economy
            .ships
            .get(&class)
            .map_or(0, |s| s.cost.alloy + raids.crystal_weight * s.cost.crystal),
    }
}

/// What a run observed of the raids, from the tick outputs (A3 plan §4.9).
#[derive(Default)]
pub(crate) struct Record {
    raids: Vec<Raid>,
    /// Each faction's timer firings, with whether it launched.
    firings: BTreeMap<FactionId, Vec<Tick>>,
    suppressed: u32,
    no_target: u32,
    launched_in_catch_up: u32,
    suppressed_outside_catch_up: u32,
    dome_damage_milli: u64,
    inner_damage: u64,
    /// Hull the raiders' weapons took off, by what they hit.
    dealt: BTreeMap<&'static str, u64>,
    errors: Vec<String>,
}

impl Record {
    /// The faction's raid on `target` that has not ended, or the latest one.
    fn raid_on(&mut self, faction: FactionId, target: CommanderId) -> Option<&mut Raid> {
        self.raids
            .iter_mut()
            .rev()
            .find(|r| r.faction == faction && r.target == target)
    }

    fn raid_of(&mut self, fleet: FleetId) -> Option<&mut Raid> {
        self.raids.iter_mut().rev().find(|r| r.fleet == fleet)
    }

    /// Records what one tick's outputs say about raids.
    pub(crate) fn observe(&mut self, state: &UniverseState, out: &nomad_sim::TickOutputs) {
        self.raid_events(state, out);
        self.losses(state, out);
        self.structures(state, out);
    }

    /// Launches, firings, the director's reports and raid endings.
    fn raid_events(&mut self, state: &UniverseState, out: &nomad_sim::TickOutputs) {
        let tick = out.tick;
        for event in &out.raid_events {
            match *event {
                RaidEvent::Launched {
                    faction,
                    fleet,
                    target,
                    system,
                    fleet_value,
                    strike_craft,
                    heavy_cruisers,
                    ..
                } => {
                    self.firings.entry(faction).or_default().push(tick);
                    if state.catching_up() {
                        self.launched_in_catch_up += 1;
                    }
                    self.raids.push(Raid {
                        faction,
                        fleet,
                        target,
                        system,
                        launched: tick,
                        fleet_value,
                        strike_craft,
                        heavy_cruisers,
                        reached: None,
                        withdrew: None,
                        ended: None,
                        ships_killed: 0,
                        value_killed: 0,
                        modules_disabled: 0,
                        outposts_destroyed: 0,
                        ships_lost: 0,
                    });
                }
                RaidEvent::Suppressed { faction } => {
                    self.firings.entry(faction).or_default().push(tick);
                    self.suppressed += 1;
                    if !state.catching_up() {
                        self.suppressed_outside_catch_up += 1;
                    }
                }
                RaidEvent::NoTarget { faction } => {
                    self.firings.entry(faction).or_default().push(tick);
                    self.no_target += 1;
                }
                RaidEvent::Returned { fleet, ships, .. } => {
                    if let Some(r) = self.raid_of(fleet) {
                        r.ended = Some((tick, End::Returned { ships }));
                    }
                }
                RaidEvent::Destroyed { fleet, .. } => {
                    if let Some(r) = self.raid_of(fleet) {
                        r.ended = Some((tick, End::Destroyed));
                    }
                }
                RaidEvent::Director { event, .. } => match event {
                    DirectorEvent::ClockStarted { fleet } => {
                        if let Some(r) = self.raid_of(fleet) {
                            r.reached = Some(tick);
                        }
                    }
                    DirectorEvent::Withdrew { fleet, reason } => {
                        if let Some(r) = self.raid_of(fleet) {
                            r.withdrew = Some(reason);
                        }
                    }
                },
            }
        }
    }

    /// Ships the raiders destroyed, and raiders lost.
    fn losses(&mut self, state: &UniverseState, out: &nomad_sim::TickOutputs) {
        for event in &out.combat_events {
            if let CombatEvent::ShipDamaged {
                by: Owner::Npc(_),
                amount,
                ..
            } = *event
            {
                *self.dealt.entry("ships").or_default() += u64::from(amount);
            }
            if let CombatEvent::ShipDestroyed {
                fleet,
                class,
                owner,
                by,
                ..
            } = *event
            {
                match (owner, by) {
                    (Owner::Commander(target), Owner::Npc(faction)) => {
                        let value = class_value(state.rules(), class);
                        if let Some(r) = self.raid_on(faction, target) {
                            r.ships_killed += 1;
                            r.value_killed += value;
                        }
                    }
                    (Owner::Npc(_), _) => {
                        if let Some(r) = self.raid_of(fleet) {
                            r.ships_lost += 1;
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    /// Raider damage to domes and to modules inside them, outposts destroyed, peripheral
    /// modules disabled.
    fn structures(&mut self, state: &UniverseState, out: &nomad_sim::TickOutputs) {
        // Modules the raiders hit this tick, for the disabled ones below.
        let mut hit: BTreeMap<nomad_sim::SimId, FactionId> = BTreeMap::new();
        for event in &out.defence_events {
            match *event {
                DefenceEvent::SectorDamaged {
                    by: Owner::Npc(_),
                    amount_milli,
                    ..
                } => self.dome_damage_milli += amount_milli,
                DefenceEvent::ModuleDamaged {
                    holding,
                    module,
                    by: Owner::Npc(faction),
                    amount,
                    ..
                } => {
                    hit.insert(module, faction);
                    let what = match state.module(module).map(|m| m.kind) {
                        Some(ModuleKind::Core) => "outpost cores",
                        Some(ModuleKind::Solar) => "solar arrays",
                        Some(ModuleKind::Harvester | ModuleKind::Extractor) => {
                            "harvesters and extractors"
                        }
                        _ => "other modules",
                    };
                    *self.dealt.entry(what).or_default() += u64::from(amount);
                    let inside = state.module(module).is_some_and(|m| {
                        let complete = state.holding(holding).is_some_and(|h| h.core_complete);
                        complete
                            && matches!(
                                (m.kind, m.slot),
                                (ModuleKind::Core, _) | (_, Slot::Inner(_) | Slot::Dock)
                            )
                    });
                    if inside {
                        self.inner_damage += u64::from(amount);
                    }
                }
                DefenceEvent::CoreDown {
                    holding,
                    by: Owner::Npc(faction),
                    ..
                } => {
                    // Only an outpost's core can go down to raiders (D-054, D-163).
                    let owner = state.holding(holding).map(|h| h.owner).or_else(|| {
                        self.raids
                            .iter()
                            .rev()
                            .find(|r| r.faction == faction && r.ended.is_none())
                            .map(|r| r.target)
                    });
                    if let Some(target) = owner
                        && let Some(r) = self.raid_on(faction, target)
                    {
                        r.outposts_destroyed += 1;
                    }
                }
                _ => {}
            }
        }
        for event in &out.economy_events {
            if let EconomyEvent::ModuleDisabled { module } = *event
                && let Some(&faction) = hit.get(&module)
                && let Some(owner) = state
                    .module(module)
                    .and_then(|m| state.holding(m.holding))
                    .map(|h| h.owner)
                && let Some(r) = self.raid_on(faction, owner)
            {
                r.modules_disabled += 1;
            }
        }
    }

    /// Writes the D-225 report and the outcomes to `text`, and records failed must-hold
    /// checks in its errors. The α runs (A3 plan §5) can measure raids the same way.
    #[expect(
        clippy::too_many_lines,
        reason = "the report reads best in D-225's order, in one place"
    )]
    pub(crate) fn report(&mut self, state: &UniverseState, days: u64, text: &mut String) {
        let rules = state.rules().ai.raids;
        let (lo, hi) = (
            rules.interval_min_s * TICKS_PER_SECOND,
            rules.interval_max_s * TICKS_PER_SECOND,
        );
        let spacing = rules.target_spacing_s * TICKS_PER_SECOND;
        // 1. Launch intervals (D-225 item 1).
        for (faction, firings) in &self.firings {
            let mut previous = 0;
            let mut gaps = Vec::new();
            for t in firings {
                gaps.push(t.get() - previous);
                previous = t.get();
            }
            let bad: Vec<_> = gaps.iter().filter(|&&g| g < lo || g > hi).collect();
            if !bad.is_empty() {
                self.errors.push(format!(
                    "faction {}: {} launch intervals outside 2-4 h",
                    faction.0,
                    bad.len()
                ));
            }
            let _ = writeln!(
                text,
                "1. faction {} timer: {} firings ({} launched, {} suppressed in catch-up, {} without a target); intervals {} to {} [{}]",
                faction.0,
                firings.len(),
                self.raids.iter().filter(|r| r.faction == *faction).count(),
                self.suppressed,
                self.no_target,
                gaps.iter()
                    .min()
                    .map_or_else(|| "-".to_owned(), |&g| hours(g)),
                gaps.iter()
                    .max()
                    .map_or_else(|| "-".to_owned(), |&g| hours(g)),
                if bad.is_empty() { "ok" } else { "FAIL" },
            );
        }
        if self.firings.is_empty() {
            self.errors.push("no haven launch timer fired".to_owned());
        }

        // 2. Raids per commander and the 6 h spacing (D-225 item 2).
        let days = days.max(1) as f64;
        let mut line = String::from("2. raids per commander:");
        let mut every_day = true;
        for c in COMMANDERS {
            let mut on: Vec<Tick> = self
                .raids
                .iter()
                .filter(|r| r.target == c)
                .map(|r| r.launched)
                .collect();
            on.sort_unstable();
            let close = on
                .windows(2)
                .filter(|w| w[1].get() - w[0].get() < spacing)
                .count();
            if close > 0 {
                self.errors.push(format!(
                    "C{}: {close} raids within 6 h of the one before",
                    c.0
                ));
            }
            let per_day = on.len() as f64 / days;
            every_day &= per_day >= 1.0;
            let _ = write!(line, " C{} {} ({per_day:.1}/day)", c.0, on.len());
        }
        let _ = writeln!(
            *text,
            "{line}; never twice within 6 h [checked]; at least one a day on average [{}]",
            if every_day { "ok" } else { "not met" }
        );

        // 3. Raids that reached their system and destroyed or disabled something (item 3).
        let reached: Vec<&Raid> = self.raids.iter().filter(|r| r.reached.is_some()).collect();
        let biting = reached.iter().filter(|r| r.hits() > 0).count();
        let _ = writeln!(
            *text,
            "3. {} raids launched, {} reached their system, {biting} of those destroyed or disabled something ({:.0}%) [{}]",
            self.raids.len(),
            reached.len(),
            100.0 * biting as f64 / reached.len().max(1) as f64,
            if 2 * biting >= reached.len() && !reached.is_empty() {
                "ok"
            } else {
                "not met"
            }
        );

        // 4. No raider damage to domes or inside them (item 4; D-056, D-163).
        if self.dome_damage_milli > 0 || self.inner_damage > 0 {
            self.errors.push(format!(
                "raiders damaged domes ({} milli-points) or modules inside them ({})",
                self.dome_damage_milli, self.inner_damage
            ));
        }
        let _ = writeln!(
            *text,
            "4. raider damage to domes {} and to modules inside them {} [{}]",
            self.dome_damage_milli,
            self.inner_damage,
            if self.dome_damage_milli == 0 && self.inner_damage == 0 {
                "ok"
            } else {
                "FAIL"
            }
        );

        // 5. No launch during catch-up (item 5; D-047).
        if self.launched_in_catch_up > 0 || self.suppressed_outside_catch_up > 0 {
            self.errors.push(format!(
                "{} launches during catch-up, {} firings suppressed outside it",
                self.launched_in_catch_up, self.suppressed_outside_catch_up
            ));
        }
        let _ = writeln!(
            *text,
            "5. launches during catch-up {}, firings suppressed {} [{}]",
            self.launched_in_catch_up,
            self.suppressed,
            if self.launched_in_catch_up == 0 && self.suppressed_outside_catch_up == 0 {
                "ok"
            } else {
                "FAIL"
            }
        );

        // Outcomes.
        let mut reasons: BTreeMap<String, usize> = BTreeMap::new();
        for r in &self.raids {
            let key = match (r.withdrew, r.ended) {
                (Some(w), _) => format!("{w:?}"),
                (None, Some((_, End::Destroyed))) => "destroyed".to_owned(),
                (None, Some((_, End::Returned { .. }))) => "returned".to_owned(),
                (None, None) => "still out".to_owned(),
            };
            *reasons.entry(key).or_default() += 1;
        }
        let spawned: u32 = self
            .raids
            .iter()
            .map(|r| r.strike_craft + r.heavy_cruisers)
            .sum();
        let lost: u32 = self.raids.iter().map(|r| r.ships_lost).sum();
        let destroyed = self
            .raids
            .iter()
            .filter(|r| matches!(r.ended, Some((_, End::Destroyed))))
            .count();
        let _ = writeln!(
            *text,
            "outcomes: {reasons:?}; raider ships lost {lost} of {spawned}; groups destroyed {destroyed}"
        );
        let killed: u32 = self.raids.iter().map(|r| r.ships_killed).sum();
        let value: u64 = self.raids.iter().map(|r| r.value_killed).sum();
        let disabled: u32 = self.raids.iter().map(|r| r.modules_disabled).sum();
        let share: Vec<f64> = reached
            .iter()
            .filter(|r| r.fleet_value > 0)
            .map(|r| r.value_killed as f64 / r.fleet_value as f64)
            .collect();
        let _ = writeln!(
            *text,
            "targets' losses: {killed} ships (value {value}), {disabled} peripheral modules disabled; mean share of the fleet value in the system lost per raid {:.0}%",
            100.0 * share.iter().sum::<f64>() / share.len().max(1) as f64
        );
        let mut by_size: BTreeMap<(u32, u32), (usize, u32)> = BTreeMap::new();
        for r in &self.raids {
            let entry = by_size
                .entry((r.heavy_cruisers, r.strike_craft))
                .or_default();
            entry.0 += 1;
            entry.1 += r.outposts_destroyed;
        }
        let sizes: Vec<String> = by_size
            .iter()
            .map(|(&(h, s), &(n, outposts))| {
                format!(
                    "{s} strike craft{} x{n}: {outposts} outposts",
                    if h > 0 { " + HSTL" } else { "" }
                )
            })
            .collect();
        let _ = writeln!(*text, "by group size: {}", sizes.join("; "));
        let _ = writeln!(*text, "hull the raiders took off: {:?}", self.dealt);
        let systems: BTreeSet<SystemId> = self.raids.iter().map(|r| r.system).collect();
        let _ = writeln!(*text, "systems raided: {}", systems.len());
        let counters = state
            .ai_counters()
            .iter()
            .filter(|((owner, layer), _)| {
                matches!(owner, Owner::Npc(_)) && *layer == Layer::Director
            })
            .map(|((owner, _), c)| {
                format!(
                    "{owner:?}: issued {}, applied {}, rejected {:?}, dropped {:?}",
                    c.issued, c.applied, c.rejected, c.dropped
                )
            })
            .collect::<Vec<_>>();
        let _ = writeln!(*text, "director commands: {}", counters.join("; "));
        for r in &self.raids {
            let _ = writeln!(
                text,
                "  {} faction {} -> C{} in system {} | fleet value {} | {} strike craft{} | reached {} | {} | hits {} (ships {}, modules {}, outposts {}) | lost {}",
                when(r.launched),
                r.faction.0,
                r.target.0,
                r.system.sim_id().get(),
                r.fleet_value,
                r.strike_craft,
                if r.heavy_cruisers > 0 { " + HSTL" } else { "" },
                r.reached.map_or_else(|| "-".to_owned(), when),
                match (r.withdrew, r.ended) {
                    (Some(w), Some((t, _))) => format!("withdrew ({w:?}), ended {}", when(t)),
                    (Some(w), None) => format!("withdrew ({w:?})"),
                    (None, Some((t, e))) => format!("{e:?} {}", when(t)),
                    (None, None) => "out".to_owned(),
                },
                r.hits(),
                r.ships_killed,
                r.modules_disabled,
                r.outposts_destroyed,
                r.ships_lost,
            );
        }
        let problems = state.fleet_problems();
        if !problems.is_empty() {
            self.errors
                .push(format!("fleet problems: {}", problems.join("; ")));
        }
    }
}

/// Runs `--raids` and prints the report and the checks.
pub fn run(out: &mut impl Write, cfg: RaidRun) -> Result<(), String> {
    let end = cfg.days.saturating_mul(DAY);
    let mut state =
        generate_stage1_with_rules(SeasonConfig::new(cfg.seed, 0), COMMANDERS, cfg.rules)
            .map_err(|e| format!("{e:?}"))?;
    let catch_up = (cfg.catch_up_hours > 0).then(|| {
        let from = cfg.catch_up_from.saturating_mul(HOUR);
        (
            from,
            from.saturating_add(cfg.catch_up_hours.saturating_mul(HOUR)),
        )
    });
    let mut record = Record::default();
    let mut claims = 0_usize;
    let mut round = 0_usize;
    let mut times = Vec::with_capacity(usize::try_from(end).unwrap_or(0));
    let started = Instant::now();
    while state.tick().get() < end {
        let t = state.tick().get();
        if Some(t) == cfg.restore_at {
            let bytes = encode(&state).map_err(|e| e.to_string())?;
            state = decode(&bytes).map_err(|e| e.to_string())?;
        }
        if cfg.claim_hours > 0 && t.is_multiple_of(HOUR) && t > 0 {
            round += 1;
            let interval = cfg.claim_hours.saturating_mul(HOUR);
            for i in 0..COMMANDERS.len() {
                let c = COMMANDERS[(i + round) % COMMANDERS.len()];
                if t.is_multiple_of(interval)
                    && let Some(site) = crate::season::next_claim(&state, c)
                    && scenario::place_outpost(&mut state, c, site).is_some()
                {
                    claims += 1;
                }
            }
        }
        let mut inputs = TickInputs::default();
        if let Some((from, to)) = catch_up {
            // Inputs given before tick t + 1 apply on it.
            if t + 1 == from {
                inputs.operator.push(OperatorInput::CatchUpStart);
            } else if t + 1 == to {
                inputs.operator.push(OperatorInput::CatchUpEnd);
            }
        }
        let tick_started = Instant::now();
        let outputs = step(&mut state, &inputs);
        times.push(tick_started.elapsed().as_micros());
        record.observe(&state, &outputs);
    }
    let elapsed = started.elapsed().as_secs_f64();
    times.sort_unstable();
    let mean = times.iter().sum::<u128>() as f64 / times.len().max(1) as f64;
    let p99 = times.get(times.len() * 99 / 100).copied().unwrap_or(0);

    let mut text = String::new();
    let _ = writeln!(
        text,
        "raids | seed {} | {} days | 4 AI commanders (Deputies; scripted claims every {} h) | catch-up {}",
        cfg.seed,
        cfg.days,
        cfg.claim_hours,
        catch_up.map_or_else(
            || "none".to_owned(),
            |(from, to)| format!("{} to {}", when(Tick::new(from)), when(Tick::new(to)))
        ),
    );

    record.report(&state, cfg.days, &mut text);
    let hash = state_hash(&state).map_err(|e| e.to_string())?;
    if record.errors.is_empty() {
        let _ = writeln!(
            text,
            "checks passed: launch intervals, the 6 h spacing, no raider damage to domes, no launch during catch-up"
        );
    } else {
        for e in &record.errors {
            let _ = writeln!(text, "CHECK FAILED: {e}");
        }
    }
    let _ = writeln!(
        text,
        "ticks {} | {claims} scripted claims | tick time: mean {mean:.0} µs, p99 {p99} µs | elapsed {elapsed:.1} s | hash {hash:016x}",
        state.tick().get()
    );
    out.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
    if record.errors.is_empty() {
        Ok(())
    } else {
        Err(format!("{} raid checks failed", record.errors.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_once(restore_at: Option<u64>) -> String {
        let mut out = Vec::new();
        let result = run(
            &mut out,
            RaidRun {
                seed: 3,
                days: 1,
                claim_hours: 6,
                catch_up_from: 8,
                catch_up_hours: 6,
                restore_at,
                rules: Rules::embedded(),
            },
        );
        let text = String::from_utf8_lossy(&out).into_owned();
        assert!(result.is_ok(), "{result:?}\n{text}");
        text
    }

    #[test]
    #[ignore = "long simulation: a day of a stage-1 Universe with raids; CI runs fast tests only (D-190), run with --ignored"]
    fn a_day_of_raids_holds_the_rules_and_survives_a_restore() {
        let text = run_once(None);
        assert!(text.contains("checks passed"), "{text}");
        assert!(text.contains("4. raider damage to domes 0"), "{text}");
        // The catch-up segment caught at least one firing.
        assert!(!text.contains("firings suppressed 0 "), "{text}");
        // A restore in the middle of a raid changes nothing.
        let hash = |t: &str| t.rsplit("hash ").next().map(str::trim).map(str::to_owned);
        let restored = run_once(Some(5 * HOUR + 1_234));
        assert_eq!(hash(&text), hash(&restored));
    }

    #[test]
    fn values_follow_the_rules() {
        let rules = Rules::embedded();
        assert_eq!(class_value(&rules, ShipClass::Lancer), 2_300);
        assert_eq!(class_value(&rules, ShipClass::HeavyCruiser), 17_000);
        assert_eq!(when(Tick::new(DAY + 90 * MINUTE)), "day  1 01:30");
        assert_eq!(hours(3 * HOUR / 2), "1.50 h");
    }
}
