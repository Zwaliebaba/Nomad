//! Fleet roles and engagement (doc 12 §5.2, doc 13 §4 item 3; D-016, D-052, D-071, D-144,
//! D-145, D-207; A1 plan §5.5, §5.6).
//!
//! Each own fleet is evaluated on its slot (D-203). A fleet under direct control gets no
//! command (RK-10), except the retreat its direct order opted into; a fleet the convoy logic
//! looks after is left to it.
//!
//! - **Retreat:** a Deputy-run fleet in space whose hull (sum over its ships, as a share of
//!   their maximum) is below its doctrine's threshold retreats to the nearest own holding
//!   with room (`Retreat`, S3). *Proposed:* the maximum includes the ships it lost since it
//!   last docked; otherwise focused fire, which destroys one ship at a time, would leave the
//!   survivors at full hull and the fleet would never retreat.
//! - **Roles** (D-207, A1 plan §5.6), from the fleet's doctrine, or by default:
//!   - *Garrison* of a holding: docked there; on alert it launches (if its hull is at or above
//!     the retreat threshold), engages up to the leash from the holding, returns to the core
//!     when it has no target, and docks again when the alert ends. If docking is full or the
//!     holding is an outpost, it station-keeps by the core.
//!   - *Reserve* at a holding: docked there, never launched.
//!   - *Patrol* a route: a `Patrol` order on it; engages within the leash of the route.
//!   - *Escort* a fleet: a `Guard` order on it; engages the escortee's attackers within the
//!     leash of the escortee. If the escortee is gone, the fleet falls back to its default
//!     role.
//! - **Engagement** (A1 plan §5.5): target priority as explicit `Attack` orders (D-144):
//!   hostiles that damaged own structures in the last 30 s (inbox), then the highest
//!   multiplier of the fleet's main weapon against the target's size (D-052), then the
//!   nearest, then the lowest `SimId`. Inside a defended zone, garrisons and patrols take the
//!   zone's rules of engagement; under *return fire* (Trader) the Deputy attacks only hostiles
//!   that fired on own assets in the last 30 s (D-145).
//! - **Repair at dock** (D-071): a damaged fleet with no alert returns to dock, unless it
//!   patrols with its hull above the retreat threshold; a fleet docked away from its holding
//!   goes back once repaired.
//! - **Default role** (D-207): a fleet that a replacement formed takes its entry's role; one
//!   formed by a ship built at a holding is that holding's garrison; the start kit's fleets
//!   are the mother station's (D-286). A fleet of unarmed ships only is a reserve (MULEs not
//!   on a convoy).
//!
//! *Proposed* readings:
//! - a fleet's home is the holding where a ship that formed it was built, or else the own
//!   holding it first launched from or is first seen docked at; a fleet with none of these
//!   (formed in space by a split or a merge) has no default role and the Deputy leaves it
//!   alone, retreat included;
//! - a garrison launches on alert only with its hull at or above the retreat threshold, and a
//!   docked patrol or escort launches only once fully repaired (or with repair at dock off);
//! - a new target is chosen among sensed, unshrouded hostile ships within the leash of the
//!   anchor (the holding's site, the nearest route point, the escortee); the current target
//!   is kept while it is a candidate with the best target's priority and multiplier, so
//!   "nearest" does not flip the choice every evaluation and an `Attack` is issued only when
//!   the choice changes;
//! - the main weapon is the first weapon of the fleet's most numerous armed class;
//! - an escort's "no alert" for repairing is "not engaged" (D-114), as it has no holding;
//! - the Deputy issues a fleet nothing new while its last command is unanswered;
//! - station-keeping "by the core" is being within 1 km (rules) of the launch point.

use std::collections::{BTreeMap, BTreeSet};

use nomad_types::{
    Command, FleetDoctrine, FleetId, FleetOrder, FleetRole, HoldingId, Location, PositionMetres,
    Queueing, RulesOfEngagement, ShipClass, ShipId, SystemId, Ticks,
};
use serde::{Deserialize, Serialize};

use super::Stagger;
use super::threat::{ThreatMemory, distance2, within};
use crate::inbox::{FleetHappening, InboxEntry, InboxEvent};
use crate::view::{FleetPlace, OwnFleet, SensedShip, View};
use crate::{Issue, Layer, Output};

/// What the Deputy keeps per own fleet.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct FleetMemory {
    fleets: BTreeMap<FleetId, Tracked>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct Tracked {
    /// The holding that built it, or where it was first seen docked.
    home: Option<HoldingId>,
    /// The last command the Deputy issued it, until its answer arrives.
    waiting: Option<Command>,
    /// The undamaged hull of its ships lost since it last docked: they count towards its
    /// maximum for the retreat threshold.
    lost_hull: u64,
}

/// What the evaluation of the fleets needs from the rest of the Deputy.
pub(super) struct Context<'a> {
    pub(super) threat: &'a ThreatMemory,
    /// Fleets the convoy logic looks after.
    pub(super) convoys: BTreeSet<FleetId>,
    /// A fleet that a replacement formed, with its entry's role.
    pub(super) successor_roles: BTreeMap<FleetId, FleetRole>,
}

/// Reads new fleets' homes, losses and the answers to the Deputy's fleet orders.
pub(super) fn read(view: &impl View, memory: &mut FleetMemory, fresh: &[&InboxEntry]) {
    for entry in fresh {
        match &entry.event {
            InboxEvent::ShipBuilt { holding, fleet, .. }
            | InboxEvent::Fleet {
                fleet,
                what: FleetHappening::Launched { holding },
            } => {
                let tracked = memory.fleets.entry(*fleet).or_default();
                tracked.home.get_or_insert(*holding);
            }
            InboxEvent::FleetLost { fleet } => {
                memory.fleets.remove(fleet);
            }
            InboxEvent::ShipLost { fleet, class, .. } => {
                if let Some(tracked) = memory.fleets.get_mut(fleet) {
                    tracked.lost_hull = tracked.lost_hull.saturating_add(view.class_hull(*class));
                }
            }
            InboxEvent::Fleet {
                fleet,
                what: FleetHappening::Docked { .. },
            } => {
                if let Some(tracked) = memory.fleets.get_mut(fleet) {
                    tracked.lost_hull = 0;
                }
            }
            InboxEvent::Outcome {
                layer: Layer::Deputy,
                command,
                ..
            } => {
                for tracked in memory.fleets.values_mut() {
                    if tracked.waiting.as_ref() == Some(command) {
                        tracked.waiting = None;
                    }
                }
            }
            _ => {}
        }
    }
}

/// Who fired on own assets in the last 30 s, by the inbox (D-145's window).
struct Attackers {
    /// Fleets that damaged own sectors or modules.
    structures: BTreeSet<FleetId>,
    /// Fleets that damaged each own fleet's ships.
    of_fleet: BTreeMap<FleetId, BTreeSet<FleetId>>,
}

impl Attackers {
    fn read(view: &impl View) -> Self {
        let now = view.now();
        let window = Ticks::from_secs(view.deputy_params().attacker_window_s);
        let mut attackers = Self {
            structures: BTreeSet::new(),
            of_fleet: BTreeMap::new(),
        };
        for entry in view.inbox() {
            if entry.tick.after(window) <= now {
                continue;
            }
            match entry.event {
                InboxEvent::SectorDamaged {
                    by_fleet: Some(by), ..
                }
                | InboxEvent::ModuleDamaged {
                    by_fleet: Some(by), ..
                } => {
                    attackers.structures.insert(by);
                }
                InboxEvent::ShipDamaged {
                    fleet,
                    by_fleet: Some(by),
                    ..
                } => {
                    attackers.of_fleet.entry(fleet).or_default().insert(by);
                }
                _ => {}
            }
        }
        attackers
    }

    /// Every fleet that fired on own assets.
    fn any(&self) -> BTreeSet<FleetId> {
        let mut all = self.structures.clone();
        for set in self.of_fleet.values() {
            all.extend(set.iter().copied());
        }
        all
    }
}

/// Evaluates the due fleets.
pub(super) fn act(
    view: &impl View,
    memory: &mut FleetMemory,
    ctx: &Context<'_>,
    stagger: Stagger,
    out: &mut Output,
) {
    let ids = view.own_fleet_ids();
    memory.fleets.retain(|id, _| ids.binary_search(id).is_ok());
    // Fleets the convoy logic or the rebuilding gave a command this evaluation.
    let commanded: BTreeSet<FleetId> = out
        .commands
        .iter()
        .filter_map(|issue| match &issue.command {
            Command::Fleet { fleet, .. }
            | Command::Economy(nomad_types::EconomyCommand::Convoy { fleet, .. }) => Some(*fleet),
            _ => None,
        })
        .collect();
    let mut attackers: Option<Attackers> = None;
    for id in ids {
        if !stagger.due(id.sim_id().get()) || commanded.contains(&id) {
            continue;
        }
        let Some(fleet) = view.own_fleet(id) else {
            continue;
        };
        let tracked = memory.fleets.entry(id).or_default();
        if tracked.home.is_none()
            && !ctx.convoys.contains(&id)
            && let FleetPlace::Docked(at) = fleet.place
            && view.holding_location(at).is_some()
        {
            tracked.home = Some(at);
        }
        if tracked.waiting.is_some() || fleet.place == FleetPlace::Travelling {
            continue;
        }
        let Some(doctrine) = view.fleet_doctrine(id) else {
            continue;
        };
        let control = view.direct_control(id);
        // A fleet on a claim trip goes on to its site as doctrine (D-291).
        let role = if control.is_some() || ctx.convoys.contains(&id) || view.on_claim_trip(id) {
            None
        } else {
            resolve(view, id, &fleet, &doctrine, tracked.home, ctx)
        };
        let now = view.now();
        let hull = Hull {
            left: fleet.hull,
            max: fleet.max_hull.saturating_add(tracked.lost_hull),
        };
        let commands = if hull.below(doctrine.retreat_below_permille)
            && matches!(fleet.place, FleetPlace::InSpace(_))
            && (control.is_some_and(|c| c.retreat) || role.is_some())
        {
            // Below the threshold it retreats, and its role waits until it has docked.
            if matches!(fleet.order, Some(FleetOrder::Retreat { .. })) {
                Vec::new()
            } else {
                vec![order(
                    id,
                    FleetOrder::Retreat { to: None },
                    Queueing::Replace,
                )]
            }
        } else if let Some(role) = &role {
            let attackers = attackers.get_or_insert_with(|| Attackers::read(view));
            let duty = Duty {
                view,
                fleet: &fleet,
                hull,
                doctrine: &doctrine,
                attackers,
            };
            duty.role(role, ctx)
        } else {
            Vec::new()
        };
        if let Some(last) = commands.last() {
            tracked.waiting = Some(last.clone());
        }
        // Alert actions count their latency from the alert's event (D-201).
        let trigger = match &role {
            Some(FleetRole::Garrison { holding }) => ctx.threat.alert(*holding).unwrap_or(now),
            _ => now,
        }
        .min(now);
        out.commands.extend(
            commands
                .into_iter()
                .map(|command| Issue { trigger, command }),
        );
    }
}

/// The role that applies to a Deputy-run fleet, if any.
fn resolve(
    view: &impl View,
    id: FleetId,
    fleet: &OwnFleet,
    doctrine: &FleetDoctrine,
    home: Option<HoldingId>,
    ctx: &Context<'_>,
) -> Option<FleetRole> {
    match &doctrine.role {
        // The escortee is gone: the fleet is released to its default role (A1 plan §5.6).
        Some(FleetRole::Escort { fleet: escortee }) if view.own_fleet(*escortee).is_none() => {}
        Some(role) => return Some(role.clone()),
        None => {}
    }
    if let Some(role) = ctx.successor_roles.get(&id) {
        return Some(role.clone());
    }
    let holding = home?;
    view.holding_location(holding)?;
    if fleet
        .ships
        .iter()
        .any(|&(_, class)| view.class_armed(class))
    {
        Some(FleetRole::Garrison { holding })
    } else {
        Some(FleetRole::Reserve { holding })
    }
}

/// A fleet's hull for its retreat threshold (A1 plan §5.5): what its ships have left, out of
/// their undamaged hull plus that of the ships it lost since it last docked.
#[derive(Clone, Copy, Debug)]
struct Hull {
    left: u64,
    max: u64,
}

impl Hull {
    /// Whether it is below `permille` of the maximum.
    fn below(self, permille: u32) -> bool {
        u128::from(self.left) * 1_000 < u128::from(permille) * u128::from(self.max)
    }
}

fn order(fleet: FleetId, order: FleetOrder, queue: Queueing) -> Command {
    Command::Fleet {
        fleet,
        order,
        queue,
        retreat: false,
    }
}

/// One fleet's evaluation.
struct Duty<'a, V: View> {
    view: &'a V,
    fleet: &'a OwnFleet,
    hull: Hull,
    doctrine: &'a FleetDoctrine,
    attackers: &'a Attackers,
}

impl<V: View> Duty<'_, V> {
    fn id(&self) -> FleetId {
        self.fleet.id
    }

    fn full(&self) -> bool {
        self.fleet.hull >= self.fleet.max_hull
    }

    fn repaired(&self) -> bool {
        self.full() || !self.doctrine.repair_at_dock
    }

    fn cmd(&self, o: FleetOrder, queue: Queueing) -> Command {
        order(self.id(), o, queue)
    }

    /// Whether its current order is `o`.
    fn doing(&self, o: &FleetOrder) -> bool {
        self.fleet.order.as_ref() == Some(o)
    }

    fn role(&self, role: &FleetRole, ctx: &Context<'_>) -> Vec<Command> {
        match role {
            FleetRole::Garrison { holding } => self.garrison(*holding, ctx),
            FleetRole::Reserve { holding } => self.reserve(*holding),
            FleetRole::Patrol { route } => self.patrol(route),
            FleetRole::Escort { fleet } => self.escort(*fleet),
        }
    }

    fn garrison(&self, holding: HoldingId, ctx: &Context<'_>) -> Vec<Command> {
        let Some(home) = self.view.holding_location(holding) else {
            return Vec::new();
        };
        let alert = ctx.threat.alert(holding).is_some();
        match self.fleet.place {
            FleetPlace::Docked(at) if at == holding => {
                let ready = !self.hull.below(self.doctrine.retreat_below_permille);
                if alert && ready {
                    vec![self.cmd(FleetOrder::Launch, Queueing::Replace)]
                } else {
                    Vec::new()
                }
            }
            FleetPlace::Docked(_) => {
                if self.repaired() {
                    self.go_home(holding, home)
                } else {
                    Vec::new()
                }
            }
            FleetPlace::InSpace(at) if alert => {
                if at.system != home.system {
                    return self.jump(home.system, Vec::new());
                }
                let centre = self.centre(holding).unwrap_or(home.position);
                let zone = self.view.holding_doctrine(holding);
                let mut commands = Vec::new();
                let mut rules = self.fleet.rules;
                if let Some(zone) = zone
                    && within(at.position, centre, f64::from(zone.zone_radius_m))
                    && rules != zone.zone_rules
                {
                    rules = zone.zone_rules;
                    commands.push(self.cmd(
                        FleetOrder::SetRulesOfEngagement { rules },
                        Queueing::Replace,
                    ));
                }
                let allowed =
                    (rules == RulesOfEngagement::ReturnFire).then(|| self.attackers.any());
                match self.engage(at, &[centre], allowed.as_ref()) {
                    Engage::Keep => {}
                    Engage::Attack(target) => commands.push(self.cmd(
                        FleetOrder::Attack {
                            target: target.sim_id(),
                        },
                        Queueing::Replace,
                    )),
                    Engage::None => commands.extend(self.to_core(at.position, home)),
                }
                commands
            }
            FleetPlace::InSpace(_) => self.go_home(holding, home),
            FleetPlace::Travelling => Vec::new(),
        }
    }

    fn reserve(&self, holding: HoldingId) -> Vec<Command> {
        let Some(home) = self.view.holding_location(holding) else {
            return Vec::new();
        };
        match self.fleet.place {
            FleetPlace::Docked(at) if at == holding => Vec::new(),
            FleetPlace::Docked(_) if !self.repaired() => Vec::new(),
            FleetPlace::Docked(_) | FleetPlace::InSpace(_) => self.go_home(holding, home),
            FleetPlace::Travelling => Vec::new(),
        }
    }

    fn patrol(&self, route: &[Location]) -> Vec<Command> {
        let Some(first) = route.first() else {
            return Vec::new();
        };
        let patrol = FleetOrder::Patrol {
            route: route.to_vec(),
        };
        match self.fleet.place {
            FleetPlace::Docked(_) => {
                if !self.repaired() {
                    return Vec::new();
                }
                let mut commands = vec![self.cmd(FleetOrder::Launch, Queueing::Replace)];
                if self.docked_system() != Some(first.system) {
                    commands.push(self.cmd(
                        FleetOrder::Jump {
                            system: first.system,
                        },
                        Queueing::Append,
                    ));
                }
                commands.push(self.cmd(patrol, Queueing::Append));
                commands
            }
            FleetPlace::InSpace(at) if at.system != first.system => {
                self.jump(first.system, vec![patrol])
            }
            FleetPlace::InSpace(at) => {
                let points: Vec<PositionMetres> = route.iter().map(|p| p.position).collect();
                let mut commands = Vec::new();
                let mut rules = self.fleet.rules;
                if let Some(zone_rules) = self.zone_rules_at(at)
                    && zone_rules != rules
                {
                    rules = zone_rules;
                    commands.push(self.cmd(
                        FleetOrder::SetRulesOfEngagement { rules },
                        Queueing::Replace,
                    ));
                }
                let allowed =
                    (rules == RulesOfEngagement::ReturnFire).then(|| self.attackers.any());
                match self.engage(at, &points, allowed.as_ref()) {
                    Engage::Keep => {}
                    Engage::Attack(target) => commands.push(self.cmd(
                        FleetOrder::Attack {
                            target: target.sim_id(),
                        },
                        Queueing::Replace,
                    )),
                    Engage::None => {
                        if !self.doing(&patrol) {
                            commands.push(self.cmd(patrol, Queueing::Replace));
                        }
                    }
                }
                commands
            }
            FleetPlace::Travelling => Vec::new(),
        }
    }

    fn escort(&self, escortee: FleetId) -> Vec<Command> {
        let Some(other) = self.view.own_fleet(escortee) else {
            return Vec::new();
        };
        let FleetPlace::InSpace(them) = other.place else {
            return Vec::new();
        };
        let guard = FleetOrder::Guard {
            object: escortee.sim_id(),
        };
        let theirs = self.attackers.of_fleet.get(&escortee);
        match self.fleet.place {
            FleetPlace::Docked(_) => {
                if !self.repaired() {
                    return Vec::new();
                }
                let mut commands = vec![self.cmd(FleetOrder::Launch, Queueing::Replace)];
                if self.docked_system() != Some(them.system) {
                    commands.push(self.cmd(
                        FleetOrder::Jump {
                            system: them.system,
                        },
                        Queueing::Append,
                    ));
                }
                commands.push(self.cmd(guard, Queueing::Append));
                commands
            }
            FleetPlace::InSpace(at) => {
                // Repair at dock when not engaged and nobody fires on the escortee (D-071).
                if self.doctrine.repair_at_dock
                    && !self.full()
                    && !self.fleet.engaged
                    && theirs.is_none()
                {
                    return vec![self.cmd(FleetOrder::Retreat { to: None }, Queueing::Replace)];
                }
                if at.system != them.system {
                    return self.jump(them.system, vec![guard]);
                }
                let empty = BTreeSet::new();
                match self.engage(at, &[them.position], Some(theirs.unwrap_or(&empty))) {
                    Engage::Keep => Vec::new(),
                    Engage::Attack(target) => vec![self.cmd(
                        FleetOrder::Attack {
                            target: target.sim_id(),
                        },
                        Queueing::Replace,
                    )],
                    Engage::None if self.doing(&guard) => Vec::new(),
                    Engage::None => vec![self.cmd(guard, Queueing::Replace)],
                }
            }
            FleetPlace::Travelling => Vec::new(),
        }
    }

    /// The system of the holding it is docked at.
    fn docked_system(&self) -> Option<SystemId> {
        match self.fleet.place {
            FleetPlace::Docked(at) => self.view.holding_location(at).map(|l| l.system),
            _ => None,
        }
    }

    fn centre(&self, holding: HoldingId) -> Option<PositionMetres> {
        self.view
            .own_holdings()
            .into_iter()
            .find(|h| h.id == holding)
            .map(|h| h.centre)
    }

    /// The rules of the defended zone `at` lies in, if any (the lowest holding identity).
    fn zone_rules_at(&self, at: Location) -> Option<RulesOfEngagement> {
        self.view
            .own_holdings()
            .into_iter()
            .filter(|h| h.system == at.system)
            .find_map(|h| {
                let d = self.view.holding_doctrine(h.id)?;
                within(at.position, h.centre, f64::from(d.zone_radius_m)).then_some(d.zone_rules)
            })
    }

    /// A jump to `system`, then `then`; nothing if it is already on its way.
    fn jump(&self, system: SystemId, then: Vec<FleetOrder>) -> Vec<Command> {
        let jump = FleetOrder::Jump { system };
        if self.doing(&jump) {
            return Vec::new();
        }
        let mut commands = vec![self.cmd(jump, Queueing::Replace)];
        commands.extend(then.into_iter().map(|o| self.cmd(o, Queueing::Append)));
        commands
    }

    /// Back to the core: a move to the launch point unless it is there or on its way.
    fn to_core(&self, at: PositionMetres, home: Location) -> Vec<Command> {
        let radius = f64::from(self.view.deputy_params().anchor_radius_m);
        let go = FleetOrder::Move { to: home };
        if within(at, home.position, radius) || self.doing(&go) {
            Vec::new()
        } else {
            vec![self.cmd(go, Queueing::Replace)]
        }
    }

    /// Home to dock at `holding`, or by its core if it cannot dock there.
    fn go_home(&self, holding: HoldingId, home: Location) -> Vec<Command> {
        if matches!(self.fleet.order, Some(FleetOrder::Retreat { .. })) {
            return Vec::new();
        }
        let docked = matches!(self.fleet.place, FleetPlace::Docked(_));
        let here = match self.fleet.place {
            FleetPlace::InSpace(at) => Some(at.system),
            _ => self.docked_system(),
        };
        let mut commands = Vec::new();
        if docked {
            commands.push(self.cmd(FleetOrder::Launch, Queueing::Replace));
        }
        let queue = if docked {
            Queueing::Append
        } else {
            Queueing::Replace
        };
        let can_dock = self.view.can_dock(self.id(), holding);
        let next = match (can_dock, here == Some(home.system)) {
            (true, true) => FleetOrder::Dock { at: holding },
            (true, false) => FleetOrder::Retreat { to: Some(holding) },
            (false, true) => {
                // Station-keeping by the core (D-102: docking full, or an outpost).
                if docked {
                    return Vec::new();
                }
                return match self.fleet.place {
                    FleetPlace::InSpace(at) => self.to_core(at.position, home),
                    _ => Vec::new(),
                };
            }
            (false, false) => FleetOrder::Jump {
                system: home.system,
            },
        };
        if self.doing(&next) {
            return Vec::new();
        }
        commands.push(self.cmd(next, queue));
        commands
    }

    /// The target choice (A1 plan §5.5) among sensed hostiles within the leash of the
    /// nearest of `anchors`; `allowed`, if given, limits it to those fleets.
    fn engage(
        &self,
        at: Location,
        anchors: &[PositionMetres],
        allowed: Option<&BTreeSet<FleetId>>,
    ) -> Engage {
        let Some(main) = self.main_class() else {
            return Engage::None;
        };
        if !self.view.anything_sensed_in(at.system) {
            return Engage::None;
        }
        let leash = f64::from(self.doctrine.leash_m);
        let mut best: Option<(Rank, ShipId)> = None;
        let mut current: Option<Rank> = None;
        let target = match &self.fleet.order {
            Some(FleetOrder::Attack { target }) => Some(*target),
            _ => None,
        };
        for sensed in self.view.sensed_fleets_in(at.system) {
            if allowed.is_some_and(|a| !a.contains(&sensed.id)) {
                continue;
            }
            let first = self.attackers.structures.contains(&sensed.id);
            for ship in &sensed.ships {
                if !candidate(ship, anchors, leash) {
                    continue;
                }
                let rank = Rank {
                    first,
                    multiplier: self.view.multiplier_percent(main, ship.class),
                    distance2: distance2(ship.position, at.position),
                    id: ship.id,
                };
                if target == Some(ship.id.sim_id()) {
                    current = Some(rank.clone());
                }
                if best.as_ref().is_none_or(|(b, _)| rank.better(b)) {
                    best = Some((rank, ship.id));
                }
            }
        }
        match (best, current) {
            (None, _) => Engage::None,
            (Some((b, _)), Some(c)) if (c.first, c.multiplier) == (b.first, b.multiplier) => {
                Engage::Keep
            }
            (Some((_, ship)), _) => Engage::Attack(ship),
        }
    }

    /// The class whose first weapon is the fleet's main weapon: its most numerous armed
    /// class, the first in class order on a tie.
    fn main_class(&self) -> Option<ShipClass> {
        let mut counts: BTreeMap<ShipClass, usize> = BTreeMap::new();
        for &(_, class) in &self.fleet.ships {
            if self.view.class_armed(class) {
                *counts.entry(class).or_default() += 1;
            }
        }
        counts
            .into_iter()
            .fold(
                None,
                |best: Option<(ShipClass, usize)>, (class, n)| match best {
                    Some((_, m)) if m >= n => best,
                    _ => Some((class, n)),
                },
            )
            .map(|(class, _)| class)
    }
}

/// Whether a sensed ship may be chosen: targetable and within the leash of an anchor.
fn candidate(ship: &SensedShip, anchors: &[PositionMetres], leash_m: f64) -> bool {
    !ship.shrouded && anchors.iter().any(|&a| within(ship.position, a, leash_m))
}

/// What the target choice says.
enum Engage {
    /// Keep the current target.
    Keep,
    /// Attack this ship.
    Attack(ShipId),
    /// No target.
    None,
}

#[derive(Clone, Debug)]
struct Rank {
    first: bool,
    multiplier: u32,
    distance2: f64,
    id: ShipId,
}

impl Rank {
    /// Priority 1, then the multiplier, then the nearer, then the lower `SimId`.
    fn better(&self, other: &Self) -> bool {
        (self.first, self.multiplier)
            .cmp(&(other.first, other.multiplier))
            .then_with(|| other.distance2.total_cmp(&self.distance2))
            .then_with(|| other.id.cmp(&self.id))
            .is_gt()
    }
}
