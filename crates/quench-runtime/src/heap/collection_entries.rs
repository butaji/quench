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

/// Collections up to this size are searched by a linear scan: comparing a few keys costs
/// less than hashing the probe key and building an index.
const LINEAR_SCAN_ENTRIES: usize = 8;
/// Holes are compacted once they outnumber the live entries and reach this count, so a
/// compaction's linear cost is spread over at least as many deletes.
const MIN_COMPACTED_HOLES: usize = 16;

/// A slot count or position, which a collection keeps in 32 bits as its key index does.
fn slot_count(count: usize) -> u32 {
    u32::try_from(count).expect("a collection's slots fit 32 bits")
}

/// The key a deleted slot holds; no JavaScript value is the deleted sentinel.
const HOLE: Value = Value::DELETED;

mod index;
mod table;
mod lazy;
mod weak;
pub(crate) use lazy::Entries;
pub(crate) use weak::{WeakMapEntries, WeakSetEntries};

pub(crate) trait CollectionEntry: Copy {
    fn key(&self) -> Value;
    fn hole() -> Self;
}

impl CollectionEntry for Value {
    #[inline(always)]
    fn key(&self) -> Value {
        *self
    }
    fn hole() -> Self {
        HOLE
    }
}

impl CollectionEntry for (Value, Value) {
    #[inline(always)]
    fn key(&self) -> Value {
        self.0
    }
    fn hole() -> Self {
        (HOLE, Value::UNDEFINED)
    }
}

/// A key hash folded to the 32 bits an index entry keeps.
#[inline(always)]
fn folded_hash(hash: u64) -> u32 {
    (hash ^ (hash >> u32::BITS)) as u32
}

/// The multiplier of the Fx hash: one multiply per word, whose product carries the input's
/// low bits into its high bits, where the key index reads its tags.
const FX_MULTIPLIER: u64 = 0x51_7c_c1_b7_27_22_0a_95;

/// The hash of a key's 64-bit word: the word's high half folded into its low half, then one
/// multiply. The fold brings a value's tag bits into the bits the multiply spreads, so small
/// integers' doubles, whose low bits are zero, still spread over the index's buckets.
#[inline(always)]
fn word_key_hash(word: u64) -> u64 {
    (word ^ (word >> u32::BITS)).wrapping_mul(FX_MULTIPLIER)
}

/// The SameValueZero hash of a number key: both zeros and every NaN hash alike, and every
/// other number by its bits (`word_key_hash`).
#[inline(always)]
pub(crate) fn number_key_hash(number: f64) -> u64 {
    let bits = if number == 0.0 { 0 } else if number.is_nan() { f64::NAN.to_bits() } else { number.to_bits() };
    word_key_hash(bits)
}

impl super::Heap {
    /// The SameValueZero hash of a key that is no number: a string by its text hash, a
    /// BigInt by value, anything else by identity (its value word). Kept out of line, so a
    /// number key's hash stays a few inlined instructions.
    #[inline(never)]
    pub(crate) fn non_number_key_hash(&self, key: Value) -> u64 {
        use std::hash::{Hash, Hasher};
        if let Some(hash) = self.string_hash(key) {
            return u64::from(hash);
        }
        match self.get(key) {
            Some(super::Cell::BigInt(value)) => {
                let mut hasher = rustc_hash::FxHasher::default();
                value.hash(&mut hasher);
                hasher.finish()
            }
            _ => word_key_hash(key.to_bits()),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CollectionEntries<T> {
    /// The slots and, per slot, its key's folded hash (`folded_hash`); a hole keeps its key's.
    table: table::Table<T>,
    live: u32,
    /// No live entry sits before this slot: deleting the oldest entries (a queue, an LRU
    /// cache evicting `map.keys().next()`) leaves holes at the front, which a walk from the
    /// first slot would otherwise step over again on every iteration.
    head: u32,
    /// The key index and the iterators and walks, boxed while there are any: a collection of
    /// `LINEAR_SCAN_ENTRIES` or fewer has no index, and few collections are ever iterated
    /// (typescript-lib: 1K of 44K Maps).
    rare: Option<Box<Rare>>,
}

#[derive(Clone, Debug, Default)]
struct Rare {
    /// Present once the slots outnumber `LINEAR_SCAN_ENTRIES`.
    index: Option<index::KeyIndex>,
    walking: Walking,
}

/// Iterators over a collection, renumbered by compaction and reset by `clear` (not traced: an
/// entry whose cell is no longer such an iterator is dropped when visited), and its active
/// `forEach`-style walks, which hold slot positions outside any iterator; no compaction runs
/// while one is active.
#[derive(Clone, Debug, Default)]
struct Walking {
    iterators: Vec<Value>,
    walks: usize,
}

impl Walking {
    fn is_empty(&self) -> bool {
        self.walks == 0 && self.iterators.is_empty()
    }
}

// A written Map's or Set's table is its own 32-byte block (typescript-lib: 22K of them), with
// its slots and hashes in a second one; 112 bytes before slots and hashes shared a block.
const _: () = assert!(size_of::<CollectionEntries<(Value, Value)>>() == 32);

impl<T> Default for CollectionEntries<T> {
    fn default() -> Self {
        Self { table: table::Table::new(), live: 0, head: 0, rare: None }
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
        if self.walks() != 0 {
            return Err(crate::snapshot::SnapshotError::unsupported("a collection being walked"));
        }
        let iterators = self.rare.as_ref().map_or(&[][..], |rare| &rare.walking.iterators[..]);
        Ok((self.table.slots(), self.table.hashes(), self.live as usize, iterators))
    }

    /// The entries `snapshot_parts` gave; the key index is rebuilt from the hashes.
    pub(crate) fn from_snapshot_parts(slots: Vec<T>, hashes: Vec<u32>, live: usize, iterators: Vec<Value>) -> Self {
        let mut table = table::Table::with_capacity(slots.len());
        slots.into_iter().zip(hashes).for_each(|(slot, hash)| table.push(slot, hash));
        let mut entries = Self { table, live: slot_count(live), head: 0, rare: None };
        entries.reindex();
        if !iterators.is_empty() {
            entries.rare_mut().walking.iterators = iterators;
        }
        entries
    }

    /// Entries with distinct keys, each with its key's hash.
    pub(crate) fn from_hashed(entries: impl IntoIterator<Item = (T, u64)>) -> Self {
        let mut collection = Self::default();
        entries.into_iter().for_each(|(entry, hash)| collection.push(entry, hash));
        collection
    }

    /// The live entries as a new collection, slots and hashes copied as they are: no key is
    /// looked up or hashed again, and no iterator is registered with the copy.
    pub(crate) fn duplicate(&self) -> Self {
        let (slots, hashes) = (self.table.slots(), self.table.hashes());
        let mut table = table::Table::with_capacity(self.live as usize);
        (self.head as usize..slots.len())
            .filter(|&slot| slots[slot].key() != HOLE)
            .for_each(|slot| table.push(slots[slot], hashes[slot]));
        let mut copy = Self { live: slot_count(table.len()), table, head: 0, rare: None };
        copy.reindex();
        copy
    }

    fn rare_mut(&mut self) -> &mut Rare {
        self.rare.get_or_insert_with(Box::default)
    }

    #[inline(always)]
    fn index(&self) -> Option<&index::KeyIndex> {
        self.rare.as_ref()?.index.as_ref()
    }

    /// Rebuilds the key index from the hashes, or drops it for a linear scan.
    fn reindex(&mut self) {
        if self.table.len() > LINEAR_SCAN_ENTRIES {
            let index = index::KeyIndex::of(self.table.hashes());
            self.rare_mut().index = Some(index);
        } else if let Some(rare) = self.rare.as_mut() {
            rare.index = None;
            if rare.walking.is_empty() {
                self.rare = None;
            }
        }
    }

    /// The number of live entries.
    pub(crate) fn len(&self) -> usize {
        self.live as usize
    }

    pub(crate) fn capacity(&self) -> usize {
        self.table.capacity()
    }

    /// The entry storage this collection owns, which allocation and live weight charge
    /// (`Cell::payload_bytes`).
    pub(crate) fn payload_bytes(&self) -> usize {
        self.table.bytes() + self.index().map_or(0, index::KeyIndex::bytes)
    }

    /// The live entries in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = T> + '_ {
        self.table.slots()[self.head as usize..]
            .iter()
            .copied()
            .filter(|entry| entry.key() != HOLE)
    }

    pub(crate) fn to_vec(&self) -> Vec<T> {
        self.iter().collect()
    }

    /// The entry at `slot`, if it is live.
    #[inline(always)]
    pub(crate) fn get(&self, slot: usize) -> Option<T> {
        self.table
            .slots()
            .get(slot)
            .copied()
            .filter(|entry| entry.key() != HOLE)
    }

    /// The first live entry at or after `slot`, with its slot.
    pub(crate) fn next_live(&self, slot: usize) -> Option<(usize, T)> {
        let slot = slot.max(self.head as usize);
        let slots = self.table.slots();
        slots
            .get(slot..)?
            .iter()
            .position(|entry| entry.key() != HOLE)
            .map(|offset| (slot + offset, slots[slot + offset]))
    }

    /// The slot of the entry whose key is `key` or one `same` accepts. `key_hash` is `key`'s
    /// hash, under which SameValueZero-equal keys agree. Identical bits are SameValueZero (a
    /// NaN included), so `same` runs only for keys spelled differently.
    #[inline(always)]
    pub(crate) fn position(&self, key: Value, key_hash: u64, same: impl Fn(Value) -> bool) -> Option<usize> {
        let hash = folded_hash(key_hash);
        let slots = self.table.slots();
        let matches = |slot: usize| {
            let candidate = slots[slot].key();
            candidate == key || (candidate != HOLE && same(candidate))
        };
        match self.index() {
            Some(index) => index.find(hash, matches),
            None => self
                .table
                .hashes()
                .iter()
                .enumerate()
                .find(|&(slot, &slot_hash)| slot_hash == hash && matches(slot))
                .map(|(slot, _)| slot),
        }
    }

    /// Appends an entry whose key is absent; `key_hash` is its key's hash.
    pub(crate) fn push(&mut self, entry: T, key_hash: u64) {
        self.table.push(entry, folded_hash(key_hash));
        self.live += 1;
        let hashes = self.table.hashes();
        match self.rare.as_mut().and_then(|rare| rare.index.as_mut()) {
            Some(index) => index.push(hashes),
            None if hashes.len() > LINEAR_SCAN_ENTRIES => self.reindex(),
            None => {}
        }
    }

    /// Deletes the live entry at `slot`, leaving a hole the index skips (a hole's key matches
    /// no probe). Returns the compaction it triggered, which the caller applies to the
    /// registered iterators (`take_iterators`).
    pub(crate) fn remove(&mut self, slot: usize) -> Option<Compaction> {
        let slots = self.table.slots_mut();
        slots[slot] = T::hole();
        self.live -= 1;
        if slot == self.head as usize {
            self.head += slot_count(slots[slot..].iter().take_while(|entry| entry.key() == HOLE).count());
        }
        let holes = self.table.len() - self.live as usize;
        (self.walks() == 0 && holes >= MIN_COMPACTED_HOLES && holes > self.live as usize)
            .then(|| self.compact())
    }

    fn compact(&mut self) -> Compaction {
        let mut live_before = Vec::with_capacity(self.table.len() + 1);
        let mut live = 0;
        for entry in self.table.slots() {
            live_before.push(live);
            if entry.key() != HOLE {
                live += 1;
            }
        }
        live_before.push(live);
        self.table.retain(|entry| entry.key() != HOLE);
        self.reindex();
        self.head = 0;
        Compaction { live_before }
    }

    /// Empties the collection; the caller restarts registered iterators at the first slot.
    /// During a walk the slots become holes instead, so entries added later still follow
    /// the walk's position.
    pub(crate) fn clear(&mut self) {
        if self.walks() == 0 {
            self.table.clear();
            self.reindex();
        } else {
            // Holes match no probe, so the index stays valid for the slots that follow.
            self.table.slots_mut().fill(T::hole());
        }
        self.live = 0;
        self.head = slot_count(self.table.len());
    }

    /// Registers a new iterator (a fresh one, or one `take_iterators` returned), and returns
    /// how many are registered. A fresh iterator may reuse the record of a collected one that
    /// is still registered; `Heap::move_collection_iterators` keeps one of the two.
    pub(crate) fn register_iterator(&mut self, iterator: Value) -> usize {
        let walking = &mut self.rare_mut().walking;
        walking.iterators.push(iterator);
        walking.iterators.len()
    }

    /// The registered iterators, for the caller to renumber or reset and re-register.
    pub(crate) fn take_iterators(&mut self) -> Vec<Value> {
        let Some(rare) = self.rare.as_mut() else { return Vec::new() };
        let iterators = std::mem::take(&mut rare.walking.iterators);
        self.drop_empty_rare();
        iterators
    }

    fn drop_empty_rare(&mut self) {
        if self.rare.as_ref().is_some_and(|rare| rare.index.is_none() && rare.walking.is_empty()) {
            self.rare = None;
        }
    }

    fn walks(&self) -> usize {
        self.rare.as_ref().map_or(0, |rare| rare.walking.walks)
    }

    /// Starts a walk that holds slot positions; pair with `end_walk`.
    pub(crate) fn begin_walk(&mut self) {
        self.rare_mut().walking.walks += 1;
    }

    pub(crate) fn end_walk(&mut self) {
        let rare = self.rare.as_mut().expect("a walk ends after it began");
        rare.walking.walks -= 1;
        self.drop_empty_rare();
    }
}

impl CollectionEntries<(Value, Value)> {
    /// The value of the live Map entry at `slot`.
    #[inline(always)]
    pub(crate) fn value_at(&self, slot: usize) -> Value {
        self.get(slot).expect("a found slot holds a live entry").1
    }

    /// Replaces the value of the live Map entry at `slot`, keeping its key.
    pub(crate) fn set_value(&mut self, slot: usize, value: Value) {
        let entry = &mut self.table.slots_mut()[slot];
        debug_assert!(entry.0 != HOLE);
        entry.1 = value;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filled(count: i32) -> CollectionEntries<Value> {
        let mut entries = CollectionEntries::default();
        for key in 0..count {
            entries.push(Value::integer(key), number_key_hash(f64::from(key)));
        }
        entries
    }

    #[test]
    fn a_duplicate_keeps_the_live_entries_and_their_lookups_and_drops_iterators() {
        for count in [3, 40] {
            let mut original = filled(count);
            original.register_iterator(Value::integer(7));
            for key in (0..count).step_by(2) {
                let hash = number_key_hash(f64::from(key));
                let slot = original.position(Value::integer(key), hash, |_| false).expect("the key is present");
                original.remove(slot);
            }
            let mut copy = original.duplicate();
            assert_eq!(copy.to_vec(), original.to_vec());
            assert_eq!(copy.len(), original.len());
            for key in 0..count {
                let found = copy.position(Value::integer(key), number_key_hash(f64::from(key)), |_| false);
                assert_eq!(found.is_some(), key % 2 == 1, "key {key} of {count}");
            }
            assert!(copy.take_iterators().is_empty(), "a copy registers no iterator of the original");
            copy.push(Value::integer(count), number_key_hash(f64::from(count)));
            assert_eq!(copy.len(), original.len() + 1);
        }
    }
}
