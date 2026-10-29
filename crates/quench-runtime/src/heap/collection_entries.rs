//! The insertion-ordered entries of a Map or Set.
//!
//! Entries live in slots in insertion order. Deleting an entry leaves a hole, so the
//! position an iterator or `forEach` holds keeps naming the same entry; holes are dropped by
//! compaction, which renumbers the collection's registered iterators. A hash index maps each
//! key's SameValueZero hash to its slot. It is built by the first lookup in a collection
//! larger than a linear scan is worth and kept current by every mutation here. The
//! collector reads only the slots.

use crate::value::Value;
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

/// One indexed entry: its slot and its key's hash folded to 32 bits, which also rehashes
/// the index as it grows without reading the keys.
#[derive(Clone, Copy, Debug)]
struct IndexedSlot {
    hash: u32,
    slot: u32,
}

/// Maps each live key's hash to its slot; keys sharing a hash hold separate entries.
type KeyIndex = hashbrown::HashTable<IndexedSlot>;

/// A key hash folded to the 32 bits an index entry keeps.
fn folded_hash(hash: u64) -> u32 {
    (hash ^ (hash >> u32::BITS)) as u32
}

/// The table hash of a folded hash: hashbrown picks buckets from the low bits and tags
/// from the high bits, so the folded bits fill both halves.
fn table_hash(folded: u32) -> u64 {
    u64::from(folded) | (u64::from(folded) << u32::BITS)
}

fn index_slot(slot: usize) -> u32 {
    u32::try_from(slot).expect("a collection's slots fit its index")
}

fn insert_slot(index: &mut KeyIndex, hash: u64, slot: usize) {
    let hash = folded_hash(hash);
    index.insert_unique(table_hash(hash), IndexedSlot { hash, slot: index_slot(slot) }, |entry| {
        table_hash(entry.hash)
    });
}

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
    /// The slots (holes included, so registered iterators keep their positions), the live
    /// count and the registered iterators, for a snapshot; refused while a walk holds slot
    /// positions outside any iterator. The key index is rebuilt on first lookup.
    pub(crate) fn snapshot_parts(&self) -> crate::snapshot::SnapshotResult<(&[T], usize, &[Value])> {
        if self.walks != 0 {
            return Err(crate::snapshot::SnapshotError::unsupported("a collection being walked"));
        }
        Ok((&self.slots, self.live, &self.iterators))
    }

    /// The entries `snapshot_parts` gave.
    pub(crate) fn from_snapshot_parts(slots: Vec<T>, live: usize, iterators: Vec<Value>) -> Self {
        Self { slots, live, iterators, ..Self::default() }
    }

    /// The number of live entries.
    pub(crate) fn len(&self) -> usize {
        self.live
    }

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
            let mut index = KeyIndex::with_capacity(self.live);
            for (slot, entry) in self.slots.iter().enumerate() {
                if entry.key() != HOLE {
                    insert_slot(&mut index, hash(entry.key()), slot);
                }
            }
            index
        });
        let folded = folded_hash(key_hash);
        index
            .find(table_hash(folded), |entry| {
                entry.hash == folded && same(self.slots[entry.slot as usize].key())
            })
            .map(|entry| entry.slot as usize)
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
            let indexed = index_slot(slot);
            if let Ok(entry) = index.find_entry(table_hash(folded_hash(key_hash)), |entry| entry.slot == indexed) {
                entry.remove();
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

    /// Registers a new iterator, each registered once (a fresh one, or one `take_iterators`
    /// returned), and returns how many are registered.
    pub(crate) fn register_iterator(&mut self, iterator: Value) -> usize {
        debug_assert!(!self.iterators.contains(&iterator));
        self.iterators.push(iterator);
        self.iterators.len()
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
