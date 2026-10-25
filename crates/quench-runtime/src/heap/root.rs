use super::Heap;
use crate::Value;
use std::sync::atomic::{AtomicU64, Ordering};

const FIRST_ROOT_TABLE_ID: u64 = 1;
static NEXT_ROOT_TABLE_ID: AtomicU64 = AtomicU64::new(FIRST_ROOT_TABLE_ID);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct RootTableId(u64);

impl RootTableId {
    fn fresh() -> Self {
        // Keep the atomic update API available at the declared Rust 1.97 MSRV.
        #[allow(deprecated)]
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

/// A non-owning heap reference whose owner is pruned before its slot is reused.
/// Heap cells are pruned during collection; the VM prunes external weak caches
/// immediately after collection and before it can allocate into freed slots.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct WeakHandle {
    pub(super) value: Value,
}

pub(crate) struct RootTable {
    id: RootTableId,
    entries: Vec<Entry>,
    free: Vec<u32>,
    scope: ScopedRoots,
}

/// The roots native callback scopes make (`insert_scoped`): a stack, as those scopes nest,
/// whose end truncates it to the scope's mark at once instead of returning each slot to the
/// free list. Its handles name their own table, so a handle of either kind never reads the
/// other's entries.
struct ScopedRoots {
    id: RootTableId,
    entries: Vec<Entry>,
    /// The generation the next scoped entry takes. Each entry takes a new one, so a handle to
    /// a truncated entry does not name the value its slot holds next (until 2^32 scoped roots
    /// later, when it would have outlived the scope by that much).
    next_generation: u32,
}

impl Default for ScopedRoots {
    fn default() -> Self {
        Self {
            id: RootTableId::fresh(),
            entries: Vec::new(),
            next_generation: 1,
        }
    }
}

impl Default for RootTable {
    fn default() -> Self {
        Self {
            id: RootTableId::fresh(),
            entries: Vec::new(),
            free: Vec::new(),
            scope: ScopedRoots::default(),
        }
    }
}

#[derive(Default)]
struct Entry {
    generation: u32,
    value: Option<Value>,
}

impl RootTable {
    pub(crate) fn write_snapshot(&self, writer: &mut crate::snapshot::Writer) {
        writer.len(self.entries.len());
        for entry in &self.entries {
            writer.u32(entry.generation);
            writer.option_value(entry.value);
        }
        writer.u32s(&self.free);
    }

    /// A table `write_snapshot` wrote, under a fresh identity: handles the host kept across the
    /// snapshot come back through `restored_root`.
    pub(crate) fn read_snapshot(
        reader: &mut crate::snapshot::Reader<'_>,
    ) -> crate::snapshot::SnapshotResult<Self> {
        let entries = crate::snapshot::read_list(reader, |reader| {
            Ok(Entry {
                generation: reader.u32()?,
                value: reader.option_value()?,
            })
        })?;
        Ok(Self {
            id: RootTableId::fresh(),
            entries,
            free: reader.u32s()?,
            scope: ScopedRoots::default(),
        })
    }

    /// The parts of `root` a snapshot keeps: its slot and generation.
    pub(crate) fn snapshot_root(&self, root: RootId) -> Option<(u32, u32)> {
        (root.table == self.id).then_some((root.slot, root.generation))
    }

    /// The handle `snapshot_root` gave the parts of, in this restored table.
    pub(crate) fn restored_root(&self, slot: u32, generation: u32) -> Option<RootId> {
        let entry = self.entries.get(slot as usize)?;
        (entry.generation == generation && entry.value.is_some()).then_some(RootId {
            table: self.id,
            slot,
            generation,
        })
    }

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

    /// A root that lasts until the scope that made it ends (`release_scope`).
    pub(crate) fn insert_scoped(&mut self, value: Value) -> RootId {
        let scope = &mut self.scope;
        let generation = scope.next_generation;
        scope.next_generation = generation.wrapping_add(1).max(1);
        let slot = u32::try_from(scope.entries.len()).expect("scoped root stack exhausted");
        scope.entries.push(Entry {
            generation,
            value: Some(value),
        });
        RootId {
            table: scope.id,
            slot,
            generation,
        }
    }

    /// The scoped stack's height, where a scope's own scoped roots begin.
    pub(crate) fn scoped_height(&self) -> usize {
        self.scope.entries.len()
    }

    /// Releases the scoped roots made since `height`, as their scope ends.
    pub(crate) fn release_scope(&mut self, height: usize) {
        self.scope.entries.truncate(height);
    }

    /// Releases the scoped roots made since `height` except `keep`, which stay for the
    /// enclosing scope.
    pub(crate) fn release_scope_keeping(&mut self, height: usize, keep: &[RootId]) {
        let id = self.scope.id;
        for (slot, entry) in self.scope.entries.iter_mut().enumerate().skip(height) {
            let root = RootId {
                table: id,
                slot: slot as u32,
                generation: entry.generation,
            };
            if !keep.contains(&root) {
                entry.value = None;
            }
        }
        self.trim_scope();
    }

    /// Drops released entries from the top of the scoped stack.
    fn trim_scope(&mut self) {
        while self
            .scope
            .entries
            .last()
            .is_some_and(|entry| entry.value.is_none())
        {
            self.scope.entries.pop();
        }
    }

    /// The scoped entry `root` names, while it holds it.
    fn scoped_entry(&mut self, root: RootId) -> Option<&mut Entry> {
        let entry = self.scope.entries.get_mut(root.slot as usize)?;
        (entry.generation == root.generation && entry.value.is_some()).then_some(entry)
    }

    pub(crate) fn update(&mut self, root: RootId, value: Value) -> bool {
        if root.table == self.scope.id {
            return self
                .scoped_entry(root)
                .map(|entry| entry.value = Some(value))
                .is_some();
        }
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
        if root.table == self.scope.id {
            let entry = self.scope.entries.get(root.slot as usize)?;
            return (entry.generation == root.generation)
                .then_some(entry.value)
                .flatten();
        }
        if root.table != self.id {
            return None;
        }
        let entry = self.entries.get(root.slot as usize)?;
        (entry.generation == root.generation)
            .then_some(entry.value)
            .flatten()
    }

    pub(crate) fn remove(&mut self, root: RootId) -> bool {
        if root.table == self.scope.id {
            let Some(entry) = self.scoped_entry(root) else {
                return false;
            };
            entry.value = None;
            self.trim_scope();
            return true;
        }
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

    /// Entry slots, live or free: the most values `values` yields.
    pub(crate) fn slot_count(&self) -> usize {
        self.entries.len() + self.scope.entries.len()
    }

    pub(crate) fn values(&self) -> impl Iterator<Item = Value> + '_ {
        self.entries
            .iter()
            .chain(&self.scope.entries)
            .filter_map(|entry| entry.value)
    }

    pub(crate) fn clear(&mut self) {
        // A fresh scoped table, so no handle made before the reset names a later root.
        self.scope = ScopedRoots::default();
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
    fn scoped_roots_end_with_their_scope() {
        let mut roots = RootTable::default();
        let outer = roots.insert_scoped(Value::number(1.0));
        let height = roots.scoped_height();
        let inner = roots.insert_scoped(Value::number(2.0));
        assert_eq!(roots.get(inner), Some(Value::number(2.0)));
        assert_eq!(roots.values().count(), 2);
        roots.release_scope(height);
        assert_eq!(roots.get(inner), None);
        assert!(!roots.update(inner, Value::number(3.0)));
        let reused = roots.insert_scoped(Value::number(4.0));
        assert_eq!(reused.slot, inner.slot);
        assert_eq!(roots.get(inner), None);
        assert_eq!(roots.get(reused), Some(Value::number(4.0)));
        assert_eq!(roots.get(outer), Some(Value::number(1.0)));
    }

    #[test]
    fn scoped_release_keeps_escaped_roots() {
        let mut roots = RootTable::default();
        let height = roots.scoped_height();
        let dropped = roots.insert_scoped(Value::number(1.0));
        let kept = roots.insert_scoped(Value::number(2.0));
        let last = roots.insert_scoped(Value::number(3.0));
        roots.release_scope_keeping(height, &[kept]);
        assert_eq!(roots.get(dropped), None);
        assert_eq!(roots.get(last), None);
        assert_eq!(roots.get(kept), Some(Value::number(2.0)));
        assert_eq!(roots.scoped_height(), height + 2);
        assert!(roots.remove(kept));
        assert_eq!(roots.scoped_height(), height);
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
