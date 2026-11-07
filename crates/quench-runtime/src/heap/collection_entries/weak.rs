//! The entries of a WeakMap or WeakSet: keys held weakly, found by identity in O(1).
//!
//! Weak collections are not iterable, so entries keep no insertion order: a delete moves the
//! last entry into the hole. Collection prunes entries whose keys died (`retain`).

use super::{CollectionEntry, Value};
use rustc_hash::FxHashMap;

#[derive(Clone, Debug)]
pub(crate) struct WeakEntries<T> {
    entries: Vec<T>,
    /// Each key's position in `entries`.
    positions: FxHashMap<Value, u32>,
}

/// A WeakMap's entries: key and value.
pub(crate) type WeakMapEntries = WeakEntries<(Value, Value)>;
/// A WeakSet's entries: its values.
pub(crate) type WeakSetEntries = WeakEntries<Value>;

impl<T> Default for WeakEntries<T> {
    fn default() -> Self {
        Self { entries: Vec::new(), positions: FxHashMap::default() }
    }
}

fn position(index: usize) -> u32 {
    u32::try_from(index).expect("a weak collection's entries fit u32 positions")
}

impl<T: CollectionEntry> WeakEntries<T> {
    /// Entries as a snapshot or `retain` left them: keys distinct.
    pub(crate) fn from_entries(entries: Vec<T>) -> Self {
        let positions = entries.iter().enumerate().map(|(index, entry)| (entry.key(), position(index))).collect();
        Self { entries, positions }
    }

    fn find(&self, key: Value) -> Option<usize> {
        self.positions.get(&key).map(|&index| index as usize)
    }

    pub(crate) fn contains(&self, key: Value) -> bool {
        self.positions.contains_key(&key)
    }

    /// Adds `entry` when its key is absent, or replaces the entry holding its key.
    pub(crate) fn insert(&mut self, entry: T) {
        match self.find(entry.key()) {
            Some(index) => self.entries[index] = entry,
            None => {
                self.positions.insert(entry.key(), position(self.entries.len()));
                self.entries.push(entry);
            }
        }
    }

    /// Deletes the entry holding `key`; `false` when there is none.
    pub(crate) fn remove(&mut self, key: Value) -> bool {
        let Some(index) = self.positions.remove(&key).map(|index| index as usize) else {
            return false;
        };
        self.entries.swap_remove(index);
        if let Some(moved) = self.entries.get(index) {
            self.positions.insert(moved.key(), position(index));
        }
        true
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &T> {
        self.entries.iter()
    }

    pub(crate) fn to_vec(&self) -> Vec<T> {
        self.entries.clone()
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(crate) fn retain(&mut self, keep: impl FnMut(&T) -> bool) {
        self.entries.retain(keep);
        *self = Self::from_entries(std::mem::take(&mut self.entries));
    }

    pub(crate) fn allocated_bytes(&self) -> usize {
        self.entries.capacity() * size_of::<T>() + self.positions.capacity() * size_of::<(Value, u32)>()
    }
}

impl WeakMapEntries {
    pub(crate) fn get(&self, key: Value) -> Option<Value> {
        self.find(key).map(|index| self.entries[index].1)
    }
}
