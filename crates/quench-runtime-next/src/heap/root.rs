use super::Heap;
use crate::Value;
use std::sync::atomic::{AtomicU64, Ordering};

const FIRST_ROOT_TABLE_ID: u64 = 1;
static NEXT_ROOT_TABLE_ID: AtomicU64 = AtomicU64::new(FIRST_ROOT_TABLE_ID);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct RootTableId(u64);

impl RootTableId {
    fn fresh() -> Self {
        let id = NEXT_ROOT_TABLE_ID
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .expect("root table identity space exhausted");
        Self(id)
    }
}

/// A generation-checked strong handle owned by a host/runtime scope.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RootId {
    table: RootTableId,
    slot: u32,
    generation: u32,
}

/// A non-owning heap reference whose slot generation must still match.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct WeakHandle {
    pub(crate) slot: u32,
    pub(crate) generation: u32,
}

pub(crate) struct RootTable {
    id: RootTableId,
    entries: Vec<Entry>,
    free: Vec<u32>,
}

impl Default for RootTable {
    fn default() -> Self {
        Self {
            id: RootTableId::fresh(),
            entries: Vec::new(),
            free: Vec::new(),
        }
    }
}

#[derive(Default)]
struct Entry {
    generation: u32,
    value: Option<Value>,
}

impl RootTable {
    pub(crate) fn insert(&mut self, value: Value) -> RootId {
        while let Some(slot) = self.free.pop() {
            let entry = &mut self.entries[slot as usize];
            let Some(generation) = entry.generation.checked_add(1) else {
                continue;
            };
            entry.generation = generation;
            entry.value = Some(value);
            return RootId {
                table: self.id,
                slot,
                generation: entry.generation,
            };
        }
        let slot = u32::try_from(self.entries.len()).expect("root table exhausted");
        self.entries.push(Entry {
            generation: 1,
            value: Some(value),
        });
        RootId {
            table: self.id,
            slot,
            generation: 1,
        }
    }

    pub(crate) fn update(&mut self, root: RootId, value: Value) -> bool {
        if root.table != self.id {
            return false;
        }
        let Some(entry) = self.entries.get_mut(root.slot as usize) else {
            return false;
        };
        if entry.generation != root.generation || entry.value.is_none() {
            return false;
        }
        entry.value = Some(value);
        true
    }

    pub(crate) fn get(&self, root: RootId) -> Option<Value> {
        if root.table != self.id {
            return None;
        }
        let entry = self.entries.get(root.slot as usize)?;
        (entry.generation == root.generation)
            .then_some(entry.value)
            .flatten()
    }

    pub(crate) fn remove(&mut self, root: RootId) -> bool {
        if root.table != self.id {
            return false;
        }
        let Some(entry) = self.entries.get_mut(root.slot as usize) else {
            return false;
        };
        if entry.generation != root.generation || entry.value.is_none() {
            return false;
        }
        entry.value = None;
        self.free.push(root.slot);
        true
    }

    pub(crate) fn values(&self) -> impl Iterator<Item = Value> + '_ {
        self.entries.iter().filter_map(|entry| entry.value)
    }

    pub(crate) fn clear(&mut self) {
        self.free.clear();
        for (slot, entry) in self.entries.iter_mut().enumerate() {
            entry.value = None;
            if entry.generation.checked_add(1).is_some() {
                self.free.push(slot as u32);
            }
        }
    }
}

impl Heap {
    pub(crate) fn root_value(&self, root: RootId) -> Option<Value> {
        self.roots.get(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_handles_cannot_update_or_remove_reused_slots() {
        let mut roots = RootTable::default();
        let first = roots.insert(Value::number(1.0));
        assert!(roots.remove(first));
        let second = roots.insert(Value::number(2.0));
        assert_ne!(first, second);
        assert!(!roots.update(first, Value::number(3.0)));
        assert!(!roots.remove(first));
        assert!(roots.update(second, Value::number(4.0)));
        assert_eq!(roots.values().collect::<Vec<_>>(), [Value::number(4.0)]);
    }

    #[test]
    fn clearing_the_heap_invalidates_pre_reset_handles() {
        let mut roots = RootTable::default();
        let old = roots.insert(Value::number(1.0));
        roots.clear();
        let new = roots.insert(Value::number(2.0));
        assert_ne!(old, new);
        assert!(!roots.update(old, Value::number(3.0)));
        assert!(roots.update(new, Value::number(4.0)));
    }

    #[test]
    fn exhausted_generations_retire_root_slots_instead_of_wrapping() {
        let mut roots = RootTable::default();
        let first = roots.insert(Value::number(1.0));
        roots.entries[first.slot as usize].generation = u32::MAX;
        let last_generation = RootId {
            table: roots.id,
            slot: first.slot,
            generation: u32::MAX,
        };
        assert!(roots.remove(last_generation));

        let replacement = roots.insert(Value::number(2.0));
        assert_ne!(replacement.slot, last_generation.slot);
        assert_eq!(roots.get(last_generation), None);
        assert_eq!(roots.get(replacement), Some(Value::number(2.0)));
        assert!(!roots.free.contains(&last_generation.slot));
        assert!(roots.entries[last_generation.slot as usize].value.is_none());
    }

    #[test]
    fn handles_are_owned_by_their_root_table() {
        let mut first = RootTable::default();
        let mut second = RootTable::default();
        let first_root = first.insert(Value::number(1.0));
        let second_root = second.insert(Value::number(2.0));

        assert_eq!(first_root.slot, second_root.slot);
        assert_eq!(first_root.generation, second_root.generation);
        assert_ne!(first_root, second_root);
        assert_eq!(second.get(first_root), None);
        assert!(!second.update(first_root, Value::number(3.0)));
        assert!(!second.remove(first_root));
        assert_eq!(second.get(second_root), Some(Value::number(2.0)));
    }
}
