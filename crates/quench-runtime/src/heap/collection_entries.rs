//! The insertion-ordered entries of a Map or Set.
//!
//! Entries live in slots in insertion order. Deleting an entry leaves a hole, so the
//! position an iterator or `forEach` holds keeps naming the same entry; holes are dropped by
//! compaction, which renumbers the collection's registered iterators. Each slot's key hash,
//! folded to 32 bits, sits beside the slots in a parallel array: probes compare hashes before
//! keys, and the key index (`index::KeyIndex`: flat arrays of hash tags and slots, probed a
//! word of tags at a time) is built from the hashes once a collection outgrows a linear scan
//! and kept current by every mutation here. The collector reads only the slots.

use crate::value::Value;
use smallvec::SmallVec;

/// Collections up to this size are searched by a linear scan: comparing a few keys costs
/// less than hashing the probe key and building an index.
const LINEAR_SCAN_ENTRIES: usize = 8;
/// Holes are compacted once they outnumber the live entries and reach this count, so a
/// compaction's linear cost is spread over at least as many deletes.
const MIN_COMPACTED_HOLES: usize = 16;

/// The key a deleted slot holds; no JavaScript value is the deleted sentinel.
const HOLE: Value = Value::DELETED;

mod index;
mod lazy;
mod weak;
pub(crate) use lazy::Entries;
pub(crate) use weak::{WeakMapEntries, WeakSetEntries};

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

/// A key hash folded to the 32 bits an index entry keeps.
fn folded_hash(hash: u64) -> u32 {
    (hash ^ (hash >> u32::BITS)) as u32
}

/// The SameValueZero hash of a number key: both zeros and every NaN hash alike. A small
/// integer's bits end in dozens of zeros, which a multiplicative hash would keep at the
/// bottom, where the index picks buckets, so splitmix64's output function (Steele, Lea and
/// Flood, OOPSLA 2014) mixes every bit into every other.
pub(crate) fn number_key_hash(number: f64) -> u64 {
    let bits = if number == 0.0 { 0 } else if number.is_nan() { f64::NAN.to_bits() } else { number.to_bits() };
    let x = (bits ^ (bits >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    let x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

#[derive(Clone, Debug)]
pub(crate) struct CollectionEntries<T> {
    slots: Vec<T>,
    /// Per slot, its key's folded hash (`folded_hash`); a hole keeps its key's. Inline while
    /// a linear scan serves, so a small collection allocates its slots alone.
    hashes: SmallVec<[u32; LINEAR_SCAN_ENTRIES]>,
    live: usize,
    /// Present once the slots outnumber `LINEAR_SCAN_ENTRIES`.
    index: Option<index::KeyIndex>,
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
            hashes: SmallVec::new(),
            live: 0,
            index: None,
            iterators: Vec::new(),
            walks: 0,
        }
    }
}

/// What a snapshot keeps of a collection: slots, their hashes, the live count, iterators.
pub(crate) type SnapshotParts<'a, T> = (&'a [T], &'a [u32], usize, &'a [Value]);

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
    /// The slots (holes included, so registered iterators keep their positions), their
    /// hashes, the live count and the registered iterators, for a snapshot; refused while a
    /// walk holds slot positions outside any iterator.
    pub(crate) fn snapshot_parts(&self) -> crate::snapshot::SnapshotResult<SnapshotParts<'_, T>> {
        if self.walks != 0 {
            return Err(crate::snapshot::SnapshotError::unsupported("a collection being walked"));
        }
        Ok((&self.slots, &self.hashes, self.live, &self.iterators))
    }

    /// The entries `snapshot_parts` gave; the key index is rebuilt from the hashes.
    pub(crate) fn from_snapshot_parts(slots: Vec<T>, hashes: Vec<u32>, live: usize, iterators: Vec<Value>) -> Self {
        let index = (slots.len() > LINEAR_SCAN_ENTRIES).then(|| index::KeyIndex::of(&hashes));
        Self { slots, hashes: SmallVec::from_vec(hashes), live, index, iterators, walks: 0 }
    }

    /// Entries with distinct keys, each with its key's hash.
    pub(crate) fn from_hashed(entries: impl IntoIterator<Item = (T, u64)>) -> Self {
        let mut collection = Self::default();
        entries.into_iter().for_each(|(entry, hash)| collection.push(entry, hash));
        collection
    }

    /// The number of live entries.
    pub(crate) fn len(&self) -> usize {
        self.live
    }

    pub(crate) fn capacity(&self) -> usize {
        self.slots.capacity()
    }

    /// The entry storage this collection owns, which allocation and live weight charge
    /// (`Cell::payload_bytes`).
    pub(crate) fn payload_bytes(&self) -> usize {
        self.slots.capacity() * size_of::<T>()
            + if self.hashes.spilled() { self.hashes.capacity() * size_of::<u32>() } else { 0 }
            + self.index.as_ref().map_or(0, index::KeyIndex::bytes)
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

    /// The slot of the entry whose key is `key` or one `same` accepts. `key_hash` is `key`'s
    /// hash, under which SameValueZero-equal keys agree. Identical bits are SameValueZero (a
    /// NaN included), so `same` runs only for keys spelled differently.
    #[inline]
    pub(crate) fn position(&self, key: Value, key_hash: u64, same: impl Fn(Value) -> bool) -> Option<usize> {
        let hash = folded_hash(key_hash);
        let matches = |slot: usize| {
            let candidate = self.slots[slot].key();
            candidate == key || (candidate != HOLE && same(candidate))
        };
        match &self.index {
            Some(index) => index.find(hash, matches),
            None => (0..self.slots.len()).find(|&slot| self.hashes[slot] == hash && matches(slot)),
        }
    }

    /// Appends an entry whose key is absent; `key_hash` is its key's hash.
    pub(crate) fn push(&mut self, entry: T, key_hash: u64) {
        self.slots.push(entry);
        self.hashes.push(folded_hash(key_hash));
        self.live += 1;
        match &mut self.index {
            Some(index) => index.push(&self.hashes),
            None if self.slots.len() > LINEAR_SCAN_ENTRIES => self.index = Some(index::KeyIndex::of(&self.hashes)),
            None => {}
        }
    }

    /// Deletes the live entry at `slot`, leaving a hole the index skips (a hole's key matches
    /// no probe). Returns the compaction it triggered, which the caller applies to the
    /// registered iterators (`take_iterators`).
    pub(crate) fn remove(&mut self, slot: usize) -> Option<Compaction> {
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
        let mut kept = self.slots.iter().map(|entry| entry.key() != HOLE);
        self.hashes.retain(|_| kept.next().unwrap_or(false));
        self.slots.retain(|entry| entry.key() != HOLE);
        self.index = (self.slots.len() > LINEAR_SCAN_ENTRIES).then(|| index::KeyIndex::of(&self.hashes));
        Compaction { live_before }
    }

    /// Empties the collection; the caller restarts registered iterators at the first slot.
    /// During a walk the slots become holes instead, so entries added later still follow
    /// the walk's position.
    pub(crate) fn clear(&mut self) {
        if self.walks == 0 {
            self.slots.clear();
            self.hashes.clear();
            self.index = None;
        } else {
            // Holes match no probe, so the index stays valid for the slots that follow.
            self.slots.fill(T::hole());
        }
        self.live = 0;
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
