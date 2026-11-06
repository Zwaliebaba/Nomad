//! Entity storage on `hecs` (D-030), keyed and ordered by [`SimId`] (D-049; S3 plan §4.2).
//!
//! A `hecs::World` holds the components; a `SimId → Entity` index finds them. The `hecs`
//! [`Entity`] is a storage handle only: it is never persisted, never sent and never used for
//! ordering (`CODING_STANDARDS.md` §8). Everything that iterates entities goes through the
//! index, in `SimId` order, so results never depend on archetype layout or on the order in
//! which `hecs` reuses handles.
//!
//! **Snapshots:** each component type is written as a table of `(SimId, component)` rows
//! sorted by `SimId` ([`EntityTables`]). Loading spawns the entities again in `SimId` order
//! and rebuilds the index, so equal states give equal bytes whatever their storage history.

use std::collections::BTreeMap;
use std::fmt;

use hecs::{Component, DynamicBundle, Entity, World};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::fleet::{Fleet, Motion, Ship, Shroud};
use crate::id::SimId;

/// The simulation's entities: ships and fleets, with their components.
#[derive(Default)]
pub(crate) struct Entities {
    world: World,
    index: BTreeMap<SimId, Entity>,
}

impl Entities {
    /// Creates an entity with identity `id`.
    ///
    /// # Panics
    ///
    /// If `id` already has an entity: identities are allocated once and never reused.
    pub(crate) fn spawn(&mut self, id: SimId, components: impl DynamicBundle) {
        let entity = self.world.spawn(components);
        let previous = self.index.insert(id, entity);
        assert!(
            previous.is_none(),
            "invariant: a SimId names at most one entity"
        );
    }

    /// Removes an entity and all its components. Does nothing if there is none.
    pub(crate) fn despawn(&mut self, id: SimId) {
        if let Some(entity) = self.index.remove(&id) {
            // The index and the world agree, so the entity exists.
            let _ = self.world.despawn(entity);
        }
    }

    /// Whether `id` has an entity.
    pub(crate) fn contains(&self, id: SimId) -> bool {
        self.index.contains_key(&id)
    }

    /// Whether `id` has a component of type `T`.
    pub(crate) fn has<T: Component>(&self, id: SimId) -> bool {
        self.index
            .get(&id)
            .is_some_and(|&e| self.world.satisfies::<&T>(e))
    }

    /// A copy of `id`'s component of type `T`.
    pub(crate) fn get<T: Component + Clone>(&self, id: SimId) -> Option<T> {
        let entity = *self.index.get(&id)?;
        self.world.get::<&T>(entity).ok().map(|c| (*c).clone())
    }

    /// Calls `f` with `id`'s component of type `T`, without copying it.
    pub(crate) fn with<T: Component, R>(&self, id: SimId, f: impl FnOnce(&T) -> R) -> Option<R> {
        let entity = *self.index.get(&id)?;
        self.world.get::<&T>(entity).ok().map(|c| f(&c))
    }

    /// `id`'s component of type `T`, for changing it in place.
    pub(crate) fn get_mut<T: Component>(&mut self, id: SimId) -> Option<&mut T> {
        let entity = *self.index.get(&id)?;
        self.world.query_one_mut::<&mut T>(entity).ok()
    }

    /// Adds or replaces `id`'s component of type `T`. Does nothing if `id` has no entity.
    pub(crate) fn insert<T: Component>(&mut self, id: SimId, component: T) {
        if let Some(&entity) = self.index.get(&id) {
            let _ = self.world.insert_one(entity, component);
        }
    }

    /// Removes `id`'s component of type `T`.
    pub(crate) fn remove<T: Component>(&mut self, id: SimId) -> Option<T> {
        let entity = *self.index.get(&id)?;
        self.world.remove_one::<T>(entity).ok()
    }

    /// Every identity with a component of type `T`, in increasing order.
    pub(crate) fn ids_with<T: Component>(&self) -> Vec<SimId> {
        self.index
            .iter()
            .filter(|&(_, &e)| self.world.satisfies::<&T>(e))
            .map(|(&id, _)| id)
            .collect()
    }

    /// The canonical tables of every component type.
    fn tables(&self) -> EntityTables {
        EntityTables {
            ships: self.table(),
            fleets: self.table(),
            motions: self.table(),
            shrouds: self.table(),
        }
    }

    fn table<T: Component + Clone>(&self) -> Vec<(SimId, T)> {
        self.index
            .iter()
            .filter_map(|(&id, &e)| Some((id, (*self.world.get::<&T>(e).ok()?).clone())))
            .collect()
    }

    /// Rebuilds the entities from their tables, in `SimId` order.
    fn from_tables(tables: EntityTables) -> Result<Self, TableError> {
        let EntityTables {
            ships,
            fleets,
            motions,
            shrouds,
        } = tables;
        check_sorted(&ships, "ships")?;
        check_sorted(&fleets, "fleets")?;
        check_sorted(&motions, "motions")?;
        check_sorted(&shrouds, "shrouds")?;

        let mut entities = Self::default();
        // Ships and fleets are the entities; the other tables add components to them.
        for (id, ship) in ships {
            entities.spawn(id, (ship,));
        }
        for (id, fleet) in fleets {
            if entities.contains(id) {
                return Err(TableError::Duplicate(id));
            }
            entities.spawn(id, (fleet,));
        }
        for (id, motion) in motions {
            entities.attach(id, motion)?;
        }
        for (id, shroud) in shrouds {
            if !entities.has::<Ship>(id) {
                return Err(TableError::Orphan(id));
            }
            entities.attach(id, shroud)?;
        }
        Ok(entities)
    }

    fn attach<T: Component>(&mut self, id: SimId, component: T) -> Result<(), TableError> {
        let entity = *self.index.get(&id).ok_or(TableError::Orphan(id))?;
        self.world
            .insert_one(entity, component)
            .map_err(|_| TableError::Orphan(id))
    }
}

/// Every component, as tables of rows sorted by `SimId`: the snapshot form of [`Entities`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct EntityTables {
    ships: Vec<(SimId, Ship)>,
    fleets: Vec<(SimId, Fleet)>,
    motions: Vec<(SimId, Motion)>,
    shrouds: Vec<(SimId, Shroud)>,
}

/// Why entity tables from a snapshot were rejected. Snapshots come from outside the process,
/// so malformed ones are errors, never panics (`CODING_STANDARDS.md` §5).
#[derive(Debug)]
enum TableError {
    Unsorted(&'static str),
    Duplicate(SimId),
    Orphan(SimId),
}

impl fmt::Display for TableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsorted(table) => write!(f, "entity table {table} is not sorted by SimId"),
            Self::Duplicate(id) => write!(f, "entity {id:?} appears twice"),
            Self::Orphan(id) => write!(f, "component of {id:?}, which is no ship or fleet"),
        }
    }
}

fn check_sorted<T>(rows: &[(SimId, T)], table: &'static str) -> Result<(), TableError> {
    if rows.windows(2).all(|pair| pair[0].0 < pair[1].0) {
        Ok(())
    } else {
        Err(TableError::Unsorted(table))
    }
}

impl Serialize for Entities {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.tables().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Entities {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let tables = EntityTables::deserialize(deserializer)?;
        Self::from_tables(tables).map_err(serde::de::Error::custom)
    }
}

impl Clone for Entities {
    fn clone(&self) -> Self {
        Self::from_tables(self.tables())
            .expect("invariant: the tables of live entities are well formed")
    }
}

impl PartialEq for Entities {
    fn eq(&self, other: &Self) -> bool {
        self.tables() == other.tables()
    }
}

impl fmt::Debug for Entities {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.tables().fmt(f)
    }
}
