//! The Strategist: plays an AI commander while it is online (doc 13 §5; D-077 to D-080,
//! D-212 to D-216; A2 plan §4).
//!
//! **When** (D-079, D-203, D-080): the simulation calls [`evaluate`] in phase 3 while its
//! commander is online, every 30, 15 or 10 s by difficulty, staggered by commander, never
//! during catch-up.
//!
//! **Each evaluation** (A2 plan §4.1):
//! 1. reads its features from the view ([`model::Features`]);
//! 2. scores every goal as an integer utility (A2 plan §4.3): *Expand* ([`expand`]), *Build
//!    out* and *Logistics* ([`build`]), and the *Garrison* stand-in ([`garrison`], D-212),
//!    times the personality weight (neutral until A5), plus seeded decision noise (its own
//!    stream, A1 plan §4.6), plus the hysteresis bonus for goals with a running plan;
//! 3. keeps plans for the best three goals, and reserves each plan's next cost from the pool
//!    in score order, so a lower plan never spends what a higher plan needs;
//! 4. advances each plan by at most one step, sending ordinary commands (D-077);
//! 5. abandons a plan step that has made no progress for 10 min and releases its fleets
//!    (D-083 item 6, RK-10).
//!
//! Near the end of a session it signs off instead ([`signoff`], D-214).
//!
//! **Hysteresis** (doc 13 §12 item 4): a plan is kept at least 5 min, and its goal scores
//! +20% while it runs.
//!
//! **Fleets** (A1 plan §5.2): the Strategist's fleet orders put fleets under direct control;
//! when a plan ends it releases them to doctrine.
//!
//! *Proposed* readings:
//! - a plan step that waits for the pool to pay a queued item is waiting for the economy
//!   (D-068), not stuck, so its timeout does not run;
//! - time offline does not count towards a step's timeout: the first evaluation of a
//!   session restarts every step's clock;
//! - a plan whose fleet carries a core kit is kept until the kit is deployed or the plan
//!   times out, whatever its score;
//! - reservations are per resource ([`model::reservation`]).

mod build;
mod expand;
mod garrison;
mod model;
pub mod rules;
mod signoff;
#[cfg(test)]
mod tests;
mod view;

use std::collections::BTreeSet;

use nomad_types::{
    BuildItem, Command, DoctrineCommand, EconomyCommand, FleetId, FleetOrder, Queueing, Stock,
    Tick, Ticks,
};
use serde::{Deserialize, Serialize};

pub use rules::{BlueprintMode, StrategistRules, Weights};
pub use view::{
    DepositInfo, EconomyNumbers, EconomyStatus, HoldingStatus, ModuleStatus, MuleCargo, QueuedItem,
    SessionTimes, SiteHolder, SiteInfo, StrategicView, StrategistParams, SystemInfo,
};

use crate::inbox::{CommandOutcome, InboxEvent};
use crate::stream::Stream;
use crate::{Issue, Layer, Output};
use model::Features;

/// A goal (doc 13 §5; D-212). The order breaks score ties.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Goal {
    /// Claim a free site (D-213).
    Expand,
    /// Build a holding out by its blueprint (D-215).
    BuildOut,
    /// Depots and convoys (D-216).
    Logistics,
    /// The stand-in fleet sink until A4 (D-212).
    Garrison,
}

/// The Strategist's memory (A1 plan §4.2): part of the Universe state.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StrategistMemory {
    /// Its random stream (A1 plan §4.6), seeded by the simulation.
    pub(crate) stream: Option<Stream>,
    /// Running plans, by goal.
    pub(crate) plans: Vec<Plan>,
    /// The last evaluation.
    pub(crate) last_eval: Option<Tick>,
    /// Inbox entries up to this tick have been read.
    pub(crate) read_until: Option<Tick>,
    /// The holding chosen for the one forward dry dock (D-215).
    pub(crate) forward_dock: Option<nomad_types::HoldingId>,
    /// Holdings hostile fire has struck, for Hard's turrets (D-215).
    pub(crate) struck: BTreeSet<nomad_types::HoldingId>,
    /// The session end the last sign-off was for.
    pub(crate) signed_off_for: Option<Tick>,
    /// What it did, for the harness.
    pub(crate) stats: StrategistStats,
}

/// Counts for the harness and tests.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StrategistStats {
    /// Evaluations run.
    pub evaluations: u64,
    /// Plans started.
    pub plans_started: u64,
    /// Plans abandoned after their step timed out.
    pub plans_abandoned: u64,
    /// Core kits deployed.
    pub deployed: u64,
    /// Sign-offs.
    pub sign_offs: u64,
}

impl StrategistMemory {
    /// Gives the Strategist its stream, once (A1 plan §4.6).
    pub fn seed(&mut self, seed: [u8; 32]) {
        if self.stream.is_none() {
            self.stream = Some(Stream::from_seed(seed));
        }
    }

    /// What it did so far.
    pub const fn stats(&self) -> StrategistStats {
        self.stats
    }

    /// The running plans, described for a harness trace.
    pub fn describe(&self) -> String {
        self.plans
            .iter()
            .map(|p| match &p.expansion {
                Some(e) => format!(
                    "{:?}(site {:?} from {:?}, {:?}, carrier {:?})",
                    p.goal,
                    e.site.sim_id().get(),
                    e.holding.sim_id().get(),
                    e.stage,
                    e.carrier.map(|c| c.sim_id().get())
                ),
                None => format!("{:?}", p.goal),
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// The goals with a running plan, in plan order.
    pub fn goals(&self) -> Vec<Goal> {
        self.plans.iter().map(|p| p.goal).collect()
    }
}

/// A running plan.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Plan {
    pub(crate) goal: Goal,
    pub(crate) started: Tick,
    /// When its step last made progress.
    pub(crate) progressed: Tick,
    /// What its step looked like then.
    pub(crate) marker: u64,
    /// Whether its step waits for the pool (A2 plan: not stuck).
    pub(crate) waiting: bool,
    /// An expansion's state.
    pub(crate) expansion: Option<expand::Expansion>,
}

impl Plan {
    fn new(goal: Goal, now: Tick) -> Self {
        Self {
            goal,
            started: now,
            progressed: now,
            marker: 0,
            waiting: false,
            expansion: None,
        }
    }

    /// The fleets the plan commands.
    fn fleets(&self) -> Vec<FleetId> {
        self.expansion
            .as_ref()
            .map(expand::Expansion::fleets)
            .unwrap_or_default()
    }

    /// Records the step's observable state; a change is progress.
    fn observe(&mut self, marker: u64, waiting: bool, now: Tick) {
        if marker != self.marker {
            self.marker = marker;
            self.progressed = now;
        }
        self.waiting = waiting;
        if waiting {
            self.progressed = now;
        }
    }
}

/// One evaluation's working state: the pool not yet reserved, the commands, the fleets
/// plans use.
pub(crate) struct Turn<'a> {
    pub(crate) f: &'a Features,
    /// The pool after the waiting items and the reservations of higher plans.
    pub(crate) free: Stock,
    pub(crate) out: &'a mut Output,
    /// Fleets running plans.
    pub(crate) busy: BTreeSet<FleetId>,
    /// Command cost already planned this evaluation.
    pub(crate) command_planned: u32,
}

impl Turn<'_> {
    pub(crate) fn send(&mut self, command: Command) {
        self.out.commands.push(Issue {
            trigger: self.f.now,
            command,
        });
    }

    pub(crate) fn fleet_order(&mut self, fleet: FleetId, order: FleetOrder, queue: Queueing) {
        self.send(Command::Fleet {
            fleet,
            order,
            queue,
            retreat: false,
        });
    }

    pub(crate) fn build(&mut self, holding: nomad_types::HoldingId, item: BuildItem) {
        self.send(Command::Economy(EconomyCommand::Build { holding, item }));
    }

    /// Spends `price` from the free pool if it covers it; otherwise holds back what the price
    /// will still need (A2 plan §4.1) and returns `false`.
    pub(crate) fn spend_or_reserve(&mut self, price: Stock) -> bool {
        if self.free.covers(price) {
            self.free = self.free.minus(price);
            true
        } else {
            let held = model::reservation(price, self.free, self.f.economy.income_last_hour);
            self.free = self.free.minus(held);
            false
        }
    }
}

/// The best `limit` expansion candidates with their scores (A2 plan §4.4), for a harness
/// trace: what *Expand* would choose now.
pub fn expansion_candidates(
    view: &impl StrategicView,
    limit: usize,
) -> Vec<(nomad_types::SiteId, i64)> {
    let Some(me) = view.observer().commander() else {
        return Vec::new();
    };
    let Some(f) = Features::read(view, me) else {
        return Vec::new();
    };
    expand::candidates(view, &f, &view.strategist())
        .into_iter()
        .take(limit)
        .map(|c| (c.site, c.score))
        .collect()
}

/// One evaluation of the Strategist of an AI commander.
#[expect(
    clippy::too_many_lines,
    reason = "the evaluation's order of work reads best in one place"
)]
pub fn evaluate(view: &impl StrategicView, memory: &mut StrategistMemory, out: &mut Output) {
    let Some(me) = view.observer().commander() else {
        return;
    };
    // A commander without holdings has nothing to play (and its memory stays as it was).
    if view.own_holdings().is_empty() {
        return;
    }
    let params = view.strategist();
    let now = view.now();
    memory.stats.evaluations += 1;
    // A new session: offline time is not a step's lack of progress.
    let gap = memory.last_eval.map(|t| now.since(t).get());
    if gap.is_none_or(|g| g > params.interval_ticks.saturating_mul(2)) {
        for plan in &mut memory.plans {
            plan.progressed = now;
        }
    }
    memory.last_eval = Some(now);
    let rejected = read_inbox(view, memory);
    let Some(f) = Features::read(view, me) else {
        return;
    };

    if signoff::due(view, &params, now) {
        signoff::run(view, &f, memory, out);
        return;
    }

    let mut turn = Turn {
        f: &f,
        free: f.economy.pool.minus(f.committed()),
        out,
        busy: memory.plans.iter().flat_map(Plan::fleets).collect(),
        command_planned: f.command_queued(),
    };

    // Plans whose step stalled are abandoned, with their fleets released (D-083 item 6).
    let timeout = Ticks::from_secs(params.rules.step_timeout_s);
    let mut kept = Vec::new();
    for plan in std::mem::take(&mut memory.plans) {
        let stalled = !plan.waiting && now.since(plan.progressed) >= timeout;
        let failed = plan.expansion.as_ref().is_some_and(|e| e.failed(&rejected));
        if stalled || failed {
            memory.stats.plans_abandoned += 1;
            release(&mut turn, view, &plan.fleets());
        } else {
            kept.push(plan);
        }
    }
    memory.plans = kept;

    // Scores (A2 plan §4.3), with noise and hysteresis.
    let mut scored = score_goals(view, &mut turn, memory, &params);
    let stream = memory
        .stream
        .get_or_insert_with(|| Stream::from_seed([0; 32]));
    for (goal, score) in &mut scored {
        if *score <= 0 {
            continue;
        }
        let noise = stream.spread(params.noise_permille);
        *score = scale(*score, 1_000 + noise);
        if memory.plans.iter().any(|p| p.goal == *goal) {
            *score = scale(
                *score,
                1_000 + i64::from(params.rules.hysteresis_bonus_permille),
            );
        }
    }
    scored.sort_by_key(|&(goal, score)| (std::cmp::Reverse(score), goal));

    // The best three goals keep or start plans; a younger plan is kept for its minimum
    // commitment, and an expansion carrying a kit until it is deployed.
    let min = Ticks::from_secs(params.rules.min_commitment_s);
    let chosen: Vec<(Goal, i64)> = scored
        .iter()
        .copied()
        .filter(|&(_, s)| s > 0)
        .take(params.rules.max_plans)
        .collect();
    let mut kept = Vec::new();
    for plan in std::mem::take(&mut memory.plans) {
        let wanted = chosen.iter().any(|&(g, _)| g == plan.goal);
        let committed = now.since(plan.started) < min
            || plan.expansion.as_ref().is_some_and(|e| e.carrying(view));
        if wanted || committed {
            kept.push(plan);
        } else {
            release(&mut turn, view, &plan.fleets());
        }
    }
    memory.plans = kept;
    for &(goal, _) in &chosen {
        if !memory.plans.iter().any(|p| p.goal == goal) {
            memory.plans.push(Plan::new(goal, now));
            memory.stats.plans_started += 1;
        }
    }
    // Advance in score order; each reserves its next cost before the next plan spends.
    let order: Vec<Goal> = scored.iter().map(|&(g, _)| g).collect();
    memory.plans.sort_by_key(|p| {
        order
            .iter()
            .position(|&g| g == p.goal)
            .unwrap_or(usize::MAX)
    });
    let mut ended = Vec::new();
    for (i, plan) in memory.plans.iter_mut().enumerate() {
        let done = match plan.goal {
            Goal::Expand => expand::advance(view, &mut turn, plan, &mut memory.stats),
            Goal::BuildOut => build::advance_build(
                view,
                &mut turn,
                plan,
                &mut memory.forward_dock,
                &memory.struck,
                &params,
            ),
            Goal::Logistics => build::advance_logistics(view, &mut turn, plan),
            Goal::Garrison => garrison::advance(view, &mut turn, plan, &params),
        };
        if done {
            ended.push(i);
        }
    }
    for i in ended.into_iter().rev() {
        let plan = memory.plans.remove(i);
        release(&mut turn, view, &plan.fleets());
    }
}

/// Each goal's raw score.
fn score_goals(
    view: &impl StrategicView,
    turn: &mut Turn<'_>,
    memory: &mut StrategistMemory,
    params: &StrategistParams<'_>,
) -> Vec<(Goal, i64)> {
    let expansion = memory
        .plans
        .iter()
        .find(|p| p.goal == Goal::Expand)
        .and_then(|p| p.expansion.as_ref());
    let expand = expand::score(view, turn.f, expansion, params);
    let build = build::score_build(
        view,
        turn.f,
        &mut memory.forward_dock,
        &memory.struck,
        params,
        expand,
    );
    let logistics = build::score_logistics(view, turn.f, params);
    let garrison = garrison::score(turn.f, params);
    vec![
        (Goal::Expand, expand),
        (Goal::BuildOut, build),
        (Goal::Logistics, logistics),
        (Goal::Garrison, garrison),
    ]
}

/// `score × permille ÷ 1000`.
fn scale(score: i64, permille: i64) -> i64 {
    i64::try_from(i128::from(score) * i128::from(permille.max(0)) / 1_000).unwrap_or(i64::MAX)
}

/// Releases the fleets under direct control to doctrine (RK-10).
fn release(turn: &mut Turn<'_>, view: &impl StrategicView, fleets: &[FleetId]) {
    for &fleet in fleets {
        if view.direct_control(fleet).is_some() {
            turn.send(Command::Doctrine(DoctrineCommand::Release { fleet }));
        }
    }
}

/// Reads the fresh inbox: the Strategist's refused commands, and hostile fire on own
/// holdings (Hard's turrets, D-215). Returns the refused commands.
fn read_inbox(view: &impl StrategicView, memory: &mut StrategistMemory) -> Vec<Command> {
    let now = view.now();
    let read = memory.read_until;
    let inbox = view.inbox();
    let from = inbox.partition_point(|e| read.is_some_and(|r| e.tick <= r));
    let to = inbox.partition_point(|e| e.tick < now).max(from);
    let mut rejected = Vec::new();
    for entry in &inbox[from..to] {
        match &entry.event {
            InboxEvent::Outcome {
                layer: Layer::Strategist,
                command,
                outcome: CommandOutcome::Rejected | CommandOutcome::Dropped(_),
            } => rejected.push(command.clone()),
            InboxEvent::SectorDamaged { holding, .. }
            | InboxEvent::ModuleDamaged { holding, .. } => {
                memory.struck.insert(*holding);
            }
            _ => {}
        }
    }
    if let Some(last) = inbox[from..to].last() {
        memory.read_until = Some(last.tick);
    }
    rejected
}
