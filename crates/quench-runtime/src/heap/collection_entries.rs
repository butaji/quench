//! The insertion-ordered entries of a Map or Set.
//!
//! Entries live in slots in insertion order. Deleting an entry leaves a hole, so the
//! position an iterator or `forEach` holds keeps naming the same entry; holes are dropped by
//! compaction, which renumbers the collection's registered iterators. A hash index maps each
//! key's SameValueZero hash to its slot. It is built by the first lookup in a collection
//! larger than a linear scan is worth and kept current by every mutation here. The
//! collector reads only the slots.

use crate::value::Value;
use rustc_hash::FxHashMap;
use std::cell::RefCell;

/// Collections up to this size are searched by a linear scan: comparing a few keys costs
/// less than hashing the probe key and building an index.
const LINEAR_SCAN_ENTRIES: usize = 8;
/// Holes are compacted once they outnumber the live entries and reach this count, so a
/// compaction's linear cost is spread over at least as many deletes.
const MIN_COMPACTED_HOLES: usize = 16;

/// The key a deleted slot holds; no JavaScript value is the deleted sentinel.
const HOLE: Value = Value::DELETED;

pub(crate) trait CollectionEntry: Copy {
    fn key(&self) -> Value;
    fn hole() -> Self;
}

impl CollectionEntry for Value {
    fn key(&self) -> Value {
        *self
    }
    fn hole() -> Self {
        HOLE
    }
}

impl CollectionEntry for (Value, Value) {
    fn key(&self) -> Value {
        self.0
    }
    fn hole() -> Self {
        (HOLE, Value::UNDEFINED)
    }
}

/// Slots of the entries whose keys share one hash.
#[derive(Clone, Debug)]
enum Slots {
    One(usize),
    Many(Vec<usize>),
}

impl Slots {
    fn as_slice(&self) -> &[usize] {
        match self {
            Self::One(slot) => std::slice::from_ref(slot),
            Self::Many(slots) => slots,
        }
    }
}

type KeyIndex = FxHashMap<u64, Slots>;

#[derive(Clone, Debug)]
pub(crate) struct CollectionEntries<T> {
    slots: Vec<T>,
    live: usize,
    index: RefCell<Option<KeyIndex>>,
    /// Iterators over this collection, renumbered by compaction and reset by `clear`. Not
    /// traced: an entry whose cell is no longer such an iterator is dropped when visited.
    iterators: Vec<Value>,
    /// Active `forEach`-style walks, which hold slot positions outside any iterator; no
    /// compaction runs while one is active.
    walks: usize,
}

impl<T> Default for CollectionEntries<T> {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            live: 0,
            index: RefCell::new(None),
            iterators: Vec::new(),
            walks: 0,
        }
    }
}

impl<T: CollectionEntry> From<Vec<T>> for CollectionEntries<T> {
    fn from(entries: Vec<T>) -> Self {
        Self {
            live: entries.len(),
            slots: entries,
            ..Self::default()
        }
    }
}

/// How a compaction moved slots: an iterator positioned at slot `old` continues from
/// `new_slot(old)`.
pub(crate) struct Compaction {
    /// For each old slot (and one past the end), the live entries before it.
    live_before: Vec<usize>,
}

impl Compaction {
    pub(crate) fn new_slot(&self, old: usize) -> usize {
        let past_end = self.live_before.len() - 1;
        self.live_before[old.min(past_end)]
    }
}

impl<T: CollectionEntry> CollectionEntries<T> {
    /// The number of live entries.
    pub(crate) fn len(&self) -> usize {
        self.live
    }

    #[cfg(feature = "profile-memory")]
    pub(crate) fn capacity(&self) -> usize {
        self.slots.capacity()
    }

    /// The live entries in insertion order.
    pub(crate) fn iter(&self) -> impl Iterator<Item = T> + '_ {
        self.slots
            .iter()
            .copied()
            .filter(|entry| entry.key() != HOLE)
    }

    pub(crate) fn to_vec(&self) -> Vec<T> {
        self.iter().collect()
    }

    /// The entry at `slot`, if it is live.
    pub(crate) fn get(&self, slot: usize) -> Option<T> {
        self.slots
            .get(slot)
            .copied()
            .filter(|entry| entry.key() != HOLE)
    }

    /// The first live entry at or after `slot`, with its slot.
    pub(crate) fn next_live(&self, slot: usize) -> Option<(usize, T)> {
        self.slots
            .get(slot..)?
            .iter()
            .position(|entry| entry.key() != HOLE)
            .map(|offset| (slot + offset, self.slots[slot + offset]))
    }

    /// The slot of the entry whose key `same` accepts. `hash` gives SameValueZero-equal keys
    /// equal hashes; `key_hash` is the probe key's.
    pub(crate) fn position(
        &self,
        key_hash: u64,
        hash: impl Fn(Value) -> u64,
        same: impl Fn(Value) -> bool,
    ) -> Option<usize> {
        if self.slots.len() <= LINEAR_SCAN_ENTRIES {
            return self
                .slots
                .iter()
                .position(|entry| entry.key() != HOLE && same(entry.key()));
        }
        let mut index = self.index.borrow_mut();
        let index = index.get_or_insert_with(|| {
            let mut index = KeyIndex::default();
            for (slot, entry) in self.slots.iter().enumerate() {
                if entry.key() != HOLE {
                    insert_slot(&mut index, hash(entry.key()), slot);
                }
            }
            index
        });
        index
            .get(&key_hash)?
            .as_slice()
            .iter()
            .copied()
            .find(|slot| same(self.slots[*slot].key()))
    }

    /// Appends an entry whose key is absent; `key_hash` is its key's hash.
    pub(crate) fn push(&mut self, entry: T, key_hash: u64) {
        if let Some(index) = self.index.get_mut() {
            insert_slot(index, key_hash, self.slots.len());
        }
        self.slots.push(entry);
        self.live += 1;
    }

    /// Deletes the live entry at `slot`, whose key hashes to `key_hash`, leaving a hole.
    /// Returns the compaction it triggered, which the caller applies to the registered
    /// iterators (`take_iterators`).
    pub(crate) fn remove(&mut self, slot: usize, key_hash: u64) -> Option<Compaction> {
        if let Some(index) = self.index.get_mut() {
            let emptied = match index.get_mut(&key_hash) {
                Some(Slots::One(_)) => true,
                Some(Slots::Many(slots)) => {
                    slots.retain(|candidate| *candidate != slot);
                    slots.is_empty()
                }
                None => false,
            };
            if emptied {
                index.remove(&key_hash);
            }
        }
        self.slots[slot] = T::hole();
        self.live -= 1;
        let holes = self.slots.len() - self.live;
        (self.walks == 0 && holes >= MIN_COMPACTED_HOLES && holes > self.live)
            .then(|| self.compact())
    }

    fn compact(&mut self) -> Compaction {
        let mut live_before = Vec::with_capacity(self.slots.len() + 1);
        let mut live = 0;
        for entry in &self.slots {
            live_before.push(live);
            if entry.key() != HOLE {
                live += 1;
            }
        }
        live_before.push(live);
        self.slots.retain(|entry| entry.key() != HOLE);
        *self.index.get_mut() = None;
        Compaction { live_before }
    }

    /// Empties the collection; the caller restarts registered iterators at the first slot.
    /// During a walk the slots become holes instead, so entries added later still follow
    /// the walk's position.
    pub(crate) fn clear(&mut self) {
        if self.walks == 0 {
            self.slots.clear();
        } else {
            self.slots.fill(T::hole());
        }
        self.live = 0;
        *self.index.get_mut() = None;
    }

    pub(crate) fn register_iterator(&mut self, iterator: Value) {
        if !self.iterators.contains(&iterator) {
            self.iterators.push(iterator);
        }
    }

    /// The registered iterators, for the caller to renumber or reset and re-register.
    pub(crate) fn take_iterators(&mut self) -> Vec<Value> {
        std::mem::take(&mut self.iterators)
    }

    /// Starts a walk that holds slot positions; pair with `end_walk`.
    pub(crate) fn begin_walk(&mut self) {
        self.walks += 1;
    }

    pub(crate) fn end_walk(&mut self) {
        self.walks -= 1;
    }
}

impl CollectionEntries<(Value, Value)> {
    /// The value of the live Map entry at `slot`.
    pub(crate) fn value_at(&self, slot: usize) -> Value {
        self.get(slot).expect("a found slot holds a live entry").1
    }

    /// Replaces the value of the live Map entry at `slot`, keeping its key.
    pub(crate) fn set_value(&mut self, slot: usize, value: Value) {
        debug_assert!(self.slots[slot].0 != HOLE);
        self.slots[slot].1 = value;
    }
}

fn insert_slot(index: &mut KeyIndex, hash: u64, slot: usize) {
    match index.get_mut(&hash) {
        None => {
            index.insert(hash, Slots::One(slot));
        }
        Some(Slots::One(first)) => {
            let first = *first;
            index.insert(hash, Slots::Many(vec![first, slot]));
        }
        Some(Slots::Many(slots)) => slots.push(slot),
    }
}
