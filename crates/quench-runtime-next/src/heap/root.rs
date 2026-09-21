use crate::Value;

/// A generation-checked strong handle owned by a host/runtime scope.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RootId {
    slot: u32,
    generation: u32,
}

#[derive(Default)]
pub(crate) struct RootTable {
    entries: Vec<Entry>,
    free: Vec<u32>,
}

#[derive(Default)]
struct Entry {
    generation: u32,
    value: Option<Value>,
}

impl RootTable {
    pub(crate) fn insert(&mut self, value: Value) -> RootId {
        if let Some(slot) = self.free.pop() {
            let entry = &mut self.entries[slot as usize];
            entry.generation = entry.generation.wrapping_add(1).max(1);
            entry.value = Some(value);
            return RootId {
                slot,
                generation: entry.generation,
            };
        }
        let slot = self.entries.len() as u32;
        self.entries.push(Entry {
            generation: 1,
            value: Some(value),
        });
        RootId {
            slot,
            generation: 1,
        }
    }

    pub(crate) fn update(&mut self, root: RootId, value: Value) -> bool {
        let Some(entry) = self.entries.get_mut(root.slot as usize) else {
            return false;
        };
        if entry.generation != root.generation || entry.value.is_none() {
            return false;
        }
        entry.value = Some(value);
        true
    }

    pub(crate) fn remove(&mut self, root: RootId) -> bool {
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
        self.entries.clear();
        self.free.clear();
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
}
