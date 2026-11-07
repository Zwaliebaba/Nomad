//! Filling the inbox from a tick's outputs (D-202; A1 plan §4.4).
//!
//! At the end of each step every owner receives what it may know of that tick: damage to its
//! own ships, sectors and modules, summed per entity and attacking owner; its own losses;
//! its own fleets' order and place events; its own economy events; the lapse of its orders.
//! Ownership is read after the tick, so an event about an entity that is gone and names no
//! owner (a convoy whose fleet was destroyed, for example) is left out: its loss is in the
//! inbox already. Entries older than the retention are dropped. The order is the outputs'
//! order, with summed damage last in `(owner, entity, attacker)` order, so it is
//! deterministic.

use std::collections::BTreeMap;

use nomad_ai::{FleetHappening, InboxEntry, InboxEvent};
use nomad_types::{FleetId, HoldingId, Owner, ShipId, SimId};

use crate::announce::AnnouncementKind;
use crate::combat::CombatEvent;
use crate::defence::DefenceEvent;
use crate::economy::{EconomyEvent, Module};
use crate::fleet::{Fleet, FleetEvent, Ship};
use crate::state::UniverseState;
use crate::step::TickOutputs;
use crate::time::{Tick, Ticks};

/// What an entity summed over one tick's damage is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Damaged {
    Ship(ShipId, FleetId),
    Sector(HoldingId, u8),
    Module(HoldingId, SimId),
}

/// One tick's entries, before they go into the inboxes.
struct Collector<'a> {
    state: &'a UniverseState,
    entries: Vec<(Owner, InboxEvent)>,
    damage: BTreeMap<(Owner, Damaged, Owner), u64>,
}

/// End of step: adds this tick's entries to every owner's inbox and drops old ones.
pub(crate) fn collect(state: &mut UniverseState, now: Tick, out: &TickOutputs) {
    let mut c = Collector {
        state,
        entries: Vec::new(),
        damage: BTreeMap::new(),
    };
    c.combat(&out.combat_events);
    c.defence(&out.defence_events);
    c.fleets(&out.fleet_events);
    c.economy(&out.economy_events);
    for announcement in &out.announcements {
        if let AnnouncementKind::OrdersLapsed { commander, fleets } = &announcement.kind {
            c.entries.push((
                Owner::Commander(*commander),
                InboxEvent::OrdersLapsed {
                    fleets: fleets.clone(),
                },
            ));
        }
    }
    let entries = c.finish();

    let inboxes = &mut state.ai_runtime.inboxes;
    for (owner, event) in entries {
        inboxes
            .entry(owner)
            .or_default()
            .push(InboxEntry { tick: now, event });
    }
    let retention = Ticks::from_secs(state.rules.ai.inbox_retention_s).get();
    // Entries are in tick order, so the old ones are a prefix.
    inboxes.retain(|_, inbox| {
        let old =
            inbox.partition_point(|entry| now.get().saturating_sub(entry.tick.get()) >= retention);
        inbox.drain(..old);
        !inbox.is_empty()
    });
}

impl Collector<'_> {
    fn fleet_owner(&self, fleet: FleetId) -> Option<Owner> {
        self.state
            .entities
            .with::<Fleet, _>(fleet.sim_id(), |f| f.owner)
    }

    fn holding_owner(&self, holding: HoldingId) -> Option<Owner> {
        self.state
            .holdings
            .get(&holding)
            .map(|h| Owner::Commander(h.owner))
    }

    fn add_damage(&mut self, owner: Option<Owner>, what: Damaged, by: Owner, amount: u64) {
        if let Some(owner) = owner {
            *self.damage.entry((owner, what, by)).or_default() += amount;
        }
    }

    fn combat(&mut self, events: &[CombatEvent]) {
        for event in events {
            match *event {
                CombatEvent::ShipDamaged {
                    ship,
                    fleet,
                    by,
                    amount,
                } => {
                    let owner = self
                        .state
                        .entities
                        .get::<Ship>(ship.sim_id())
                        .map(|s| s.owner)
                        .or_else(|| self.fleet_owner(fleet));
                    self.add_damage(owner, Damaged::Ship(ship, fleet), by, u64::from(amount));
                }
                CombatEvent::ShipDestroyed {
                    ship,
                    fleet,
                    class,
                    owner,
                    by,
                } => self.entries.push((
                    owner,
                    InboxEvent::ShipLost {
                        ship,
                        class,
                        fleet,
                        by,
                    },
                )),
                CombatEvent::FleetDestroyed { fleet, owner } => {
                    self.entries.push((owner, InboxEvent::FleetLost { fleet }));
                }
            }
        }
    }

    fn defence(&mut self, events: &[DefenceEvent]) {
        for event in events {
            match *event {
                DefenceEvent::SectorDamaged {
                    holding,
                    sector,
                    by,
                    amount_milli,
                } => {
                    let owner = self.holding_owner(holding);
                    self.add_damage(owner, Damaged::Sector(holding, sector), by, amount_milli);
                }
                DefenceEvent::ModuleDamaged {
                    holding,
                    module,
                    by,
                    amount,
                } => {
                    let owner = self.holding_owner(holding);
                    self.add_damage(
                        owner,
                        Damaged::Module(holding, module),
                        by,
                        u64::from(amount),
                    );
                }
                _ => {}
            }
        }
    }

    fn fleets(&mut self, events: &[FleetEvent]) {
        for event in events {
            let (fleet, what) = match *event {
                FleetEvent::Launched { fleet, holding } => {
                    (fleet, FleetHappening::Launched { holding })
                }
                FleetEvent::Docked { fleet, holding } => {
                    (fleet, FleetHappening::Docked { holding })
                }
                FleetEvent::OrderCompleted { fleet } => (fleet, FleetHappening::OrderCompleted),
                FleetEvent::TargetLost { fleet } => (fleet, FleetHappening::TargetLost),
                FleetEvent::OrderDropped { fleet } => (fleet, FleetHappening::OrderDropped),
                FleetEvent::Arrived { fleet, system, .. } => {
                    (fleet, FleetHappening::Arrived { system })
                }
                FleetEvent::SpoolUpStarted { .. }
                | FleetEvent::SpoolUpCancelled { .. }
                | FleetEvent::Departed { .. } => continue,
            };
            if let Some(owner) = self.fleet_owner(fleet) {
                self.entries
                    .push((owner, InboxEvent::Fleet { fleet, what }));
            }
        }
    }

    fn economy(&mut self, events: &[EconomyEvent]) {
        for event in events {
            let module_holding =
                |module: SimId| self.state.entities.get::<Module>(module).map(|m| m.holding);
            let (holding, event) = match *event {
                EconomyEvent::ShipBuilt {
                    holding,
                    ship,
                    fleet,
                } => (
                    holding,
                    InboxEvent::ShipBuilt {
                        holding,
                        ship,
                        fleet,
                    },
                ),
                EconomyEvent::ItemCompleted { holding, item } => {
                    (holding, InboxEvent::ItemCompleted { holding, item })
                }
                EconomyEvent::ModuleDisabled { module } => match module_holding(module) {
                    Some(holding) => (holding, InboxEvent::ModuleDisabled { holding, module }),
                    None => continue,
                },
                EconomyEvent::ModuleRestored { module } => match module_holding(module) {
                    Some(holding) => (holding, InboxEvent::ModuleRestored { holding, module }),
                    None => continue,
                },
                EconomyEvent::ModuleDestroyed {
                    module,
                    holding,
                    kind,
                    slot,
                } => (
                    holding,
                    InboxEvent::ModuleDestroyed {
                        holding,
                        module,
                        kind,
                        slot,
                    },
                ),
                EconomyEvent::ConvoyEnded { fleet } => {
                    if let Some(owner) = self.fleet_owner(fleet) {
                        self.entries
                            .push((owner, InboxEvent::ConvoyEnded { fleet }));
                    }
                    continue;
                }
                _ => continue,
            };
            if let Some(owner) = self.holding_owner(holding) {
                self.entries.push((owner, event));
            }
        }
    }

    /// The entries, with the summed damage after the rest.
    fn finish(mut self) -> Vec<(Owner, InboxEvent)> {
        for ((owner, what, by), amount) in std::mem::take(&mut self.damage) {
            let event = match what {
                Damaged::Ship(ship, fleet) => InboxEvent::ShipDamaged {
                    ship,
                    fleet,
                    by,
                    hull: amount,
                },
                Damaged::Sector(holding, sector) => InboxEvent::SectorDamaged {
                    holding,
                    sector,
                    by,
                    amount_milli: amount,
                },
                Damaged::Module(holding, module) => InboxEvent::ModuleDamaged {
                    holding,
                    module,
                    by,
                    hull: amount,
                },
            };
            self.entries.push((owner, event));
        }
        self.entries
    }
}
