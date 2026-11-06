//! Deadline timers (Design doc 11 §3).
//!
//! Every timer is an absolute deadline tick, never a countdown. Due timers fire in
//! `(deadline, SimId)` order, which is total and therefore deterministic. Ordered maps keep
//! the snapshot encoding canonical.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::id::{SimId, SimIdAllocator};
use crate::time::Tick;

/// A queue of timers carrying a payload of type `T`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimerQueue<T> {
    by_deadline: BTreeMap<(Tick, SimId), T>,
    deadline_of: BTreeMap<SimId, Tick>,
}

impl<T> Default for TimerQueue<T> {
    fn default() -> Self {
        Self {
            by_deadline: BTreeMap::new(),
            deadline_of: BTreeMap::new(),
        }
    }
}

impl<T> TimerQueue<T> {
    /// Schedules `payload` to fire at `deadline`. Returns the timer's identity.
    pub fn schedule(&mut self, ids: &mut SimIdAllocator, deadline: Tick, payload: T) -> SimId {
        let id = ids.allocate();
        self.by_deadline.insert((deadline, id), payload);
        self.deadline_of.insert(id, deadline);
        id
    }

    /// Cancels a timer. Returns its payload if it was still pending.
    pub fn cancel(&mut self, id: SimId) -> Option<T> {
        let deadline = self.deadline_of.remove(&id)?;
        self.by_deadline.remove(&(deadline, id))
    }

    /// Removes and returns the earliest timer due at or before `now`.
    pub fn pop_due(&mut self, now: Tick) -> Option<(SimId, Tick, T)> {
        let entry = self.by_deadline.first_entry()?;
        let (deadline, id) = *entry.key();
        if deadline > now {
            return None;
        }
        let payload = entry.remove();
        self.deadline_of.remove(&id);
        Some((id, deadline, payload))
    }

    /// The earliest pending deadline.
    pub fn next_deadline(&self) -> Option<Tick> {
        self.by_deadline
            .keys()
            .next()
            .map(|&(deadline, _)| deadline)
    }

    /// The deadline of a pending timer.
    pub fn deadline_of(&self, id: SimId) -> Option<Tick> {
        self.deadline_of.get(&id).copied()
    }

    /// Every pending timer's deadline and payload, in firing order.
    pub fn pending(&self) -> impl Iterator<Item = (Tick, &T)> {
        self.by_deadline
            .iter()
            .map(|(&(deadline, _), payload)| (deadline, payload))
    }

    /// Number of pending timers.
    pub fn len(&self) -> usize {
        self.by_deadline.len()
    }

    /// Whether no timers are pending.
    pub fn is_empty(&self) -> bool {
        self.by_deadline.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fires_in_deadline_then_id_order() {
        let mut ids = SimIdAllocator::default();
        let mut q = TimerQueue::default();
        let late = q.schedule(&mut ids, Tick::new(5), "late");
        let first = q.schedule(&mut ids, Tick::new(3), "first");
        let second = q.schedule(&mut ids, Tick::new(3), "second");
        assert_eq!(q.pop_due(Tick::new(2)), None);
        assert_eq!(
            q.pop_due(Tick::new(4)),
            Some((first, Tick::new(3), "first"))
        );
        assert_eq!(
            q.pop_due(Tick::new(4)),
            Some((second, Tick::new(3), "second"))
        );
        assert_eq!(q.pop_due(Tick::new(4)), None);
        assert_eq!(q.next_deadline(), Some(Tick::new(5)));
        assert_eq!(q.pop_due(Tick::new(9)), Some((late, Tick::new(5), "late")));
        assert!(q.is_empty());
    }

    #[test]
    fn cancel_removes_pending_timer() {
        let mut ids = SimIdAllocator::default();
        let mut q = TimerQueue::default();
        let a = q.schedule(&mut ids, Tick::new(1), 1);
        let b = q.schedule(&mut ids, Tick::new(2), 2);
        assert_eq!(q.cancel(a), Some(1));
        assert_eq!(q.cancel(a), None);
        assert_eq!(q.deadline_of(b), Some(Tick::new(2)));
        assert_eq!(q.len(), 1);
        assert_eq!(q.pop_due(Tick::new(10)), Some((b, Tick::new(2), 2)));
    }
}
