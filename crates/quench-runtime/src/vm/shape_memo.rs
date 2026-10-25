//! Memoized answers about non-dictionary shapes. Such a shape never changes once created, so
//! what it says about a key or a slot is a fact of the pair: the memo answers a repeated
//! question with one probe instead of a walk down the shape's transition chain. A symbol key
//! a live shape names is kept alive by that shape (`append_shape_roots`), so no entry can be
//! reached through a reused symbol. Entries are dropped whenever shape ids or atoms are
//! reassigned (shape compaction, VM reset).

use std::cell::{Cell, OnceCell};
use std::hash::{Hash, Hasher};

/// Direct-mapped entries: a power of two. Memory-first (TOD-123): 1024 entries, a quarter
/// of the 4096 sized for the Node HTTP stack's working set.
const SHAPE_MEMO_ENTRIES: usize = 1 << 10;

/// The fewest entries a policy leaves a table (`DirectMappedTable::fit`): the index mask
/// needs a power of two above one.
const MINIMUM_FITTED_ENTRIES: usize = 2;

/// A direct-mapped memo of `ENTRIES` entries (a power of two) that allocates them on its
/// first record. A memo a program never records into costs no memory: built at VM
/// construction, the memos cost every run 48 to 254 KiB of peak RSS on V8-v7 whether or
/// not it used private names, symbol keys or enumeration
/// (`evidence/tod137-lazy-memo-tables-linux-2026-10-22`).
/// `ENTRIES` is the index space callers hash into; the runtime's memory policy picks how many
/// of them the first record allocates (`fit`), and an index lands on the allocated entries by
/// its low bits.
pub(super) struct DirectMappedTable<T: Copy, const ENTRIES: usize> {
    entries: OnceCell<Box<[Cell<Option<T>>]>>,
    /// The entries the first record allocates: `ENTRIES` scaled to the memory policy.
    allocated: usize,
}

impl<T: Copy, const ENTRIES: usize> Default for DirectMappedTable<T, ENTRIES> {
    fn default() -> Self {
        Self { entries: OnceCell::new(), allocated: ENTRIES }
    }
}

impl<T: Copy, const ENTRIES: usize> DirectMappedTable<T, ENTRIES> {
    const POWER_OF_TWO: () = assert!(ENTRIES.is_power_of_two() && ENTRIES > 1);

    /// The entry at `index`, if one was recorded since the last clear.
    #[inline(always)]
    pub(super) fn get(&self, index: usize) -> Option<T> {
        let entries = self.entries.get()?;
        entries[index & (entries.len() - 1)].get()
    }

    /// Records `value` at `index`, replacing what the entry held.
    #[inline(always)]
    pub(super) fn set(&self, index: usize, value: T) {
        let entries = match self.entries.get() {
            Some(entries) => entries,
            None => self.allocate(),
        };
        entries[index & (entries.len() - 1)].set(Some(value));
    }

    /// The first record's allocation, kept out of line so the record and probe paths that
    /// inline `set` and `get` stay as small as the eager table's: inlined, it cost DeltaBlue
    /// `--jit` about 8%.
    #[cold]
    #[inline(never)]
    fn allocate(&self) -> &[Cell<Option<T>>] {
        let () = Self::POWER_OF_TWO;
        self.entries.get_or_init(|| (0..self.allocated).map(|_| Cell::new(None)).collect())
    }

    /// Sizes the table for `policy` (`MemoryPolicy::memo_entry_shift`), dropping what it
    /// recorded: entries allocated before the policy was chosen were sized for the default.
    pub(super) fn fit(&mut self, policy: crate::MemoryPolicy) {
        let () = Self::POWER_OF_TWO;
        self.allocated = (ENTRIES >> policy.memo_entry_shift()).max(MINIMUM_FITTED_ENTRIES);
        self.entries = OnceCell::new();
    }

    pub(super) fn clear(&self) {
        for entry in self.entries.get().into_iter().flat_map(|entries| entries.iter()) {
            entry.set(None);
        }
    }
}

/// One question kind about shapes: `K` names what is asked of a shape, `V` is the answer.
pub(super) struct ShapeMemo<K: Copy + Eq + Hash, V: Copy> {
    entries: DirectMappedTable<(u32, K, V), SHAPE_MEMO_ENTRIES>,
}

/// The slot a property key has in a shape, or `None` for its absence.
pub(super) type ShapeSlotMemo = ShapeMemo<super::property_key::PropertyKey, Option<u32>>;
/// The attributes a shape gives a slot, or `None` when no transition describes it.
pub(super) type ShapeAttributeMemo = ShapeMemo<u32, Option<super::ShapeAttributes>>;
/// The shape an empty object of a shape reaches by a plain copy (`Vm::copy_plain_properties`)
/// of a source of the keyed shape: its enumerable data properties added in order.
pub(super) type PlainCopyShapeMemo = ShapeMemo<u32, u32>;
/// `PlainCopyShapeMemo` for `Set` (`Object.assign`): keyed also by the target's prototype,
/// whose chain the copy proved intercepts none of the keys, and valid while the
/// `prototype_layout_epoch` it was proved under holds.
pub(super) type PlainAssignShapeMemo = ShapeMemo<(u32, super::Value), (u32, u64)>;

impl<K: Copy + Eq + Hash, V: Copy> Default for ShapeMemo<K, V> {
    fn default() -> Self {
        Self { entries: DirectMappedTable::default() }
    }
}

/// The entry `key` maps to in a direct-mapped table of `entries`, a power of two above one:
/// the top bits of its Fx hash. An Fx hash's low bits depend only on its input's low bits,
/// so masking them would crowd keys built from small shape ids and heap indices into a few
/// entries, which then evict each other on every use.
pub(super) fn direct_mapped_index(key: impl Hash, entries: usize) -> usize {
    debug_assert!(entries.is_power_of_two() && entries > 1);
    direct_mapped_index_by_shift(key, direct_mapped_shift(entries))
}

/// The shift that keeps the top bits of a hash naming one of `entries`
/// (`direct_mapped_index`), for a table that derives it once for its size.
pub(super) const fn direct_mapped_shift(entries: usize) -> u32 {
    u64::BITS - entries.trailing_zeros()
}

/// `direct_mapped_index` of a table whose size `shift` names (`direct_mapped_shift`).
#[inline(always)]
pub(super) fn direct_mapped_index_by_shift(key: impl Hash, shift: u32) -> usize {
    let mut hasher = rustc_hash::FxHasher::default();
    key.hash(&mut hasher);
    (hasher.finish() >> shift) as usize
}

impl<K: Copy + Eq + Hash, V: Copy> ShapeMemo<K, V> {
    fn index(shape: u32, key: K) -> usize {
        direct_mapped_index((shape, key), SHAPE_MEMO_ENTRIES)
    }

    /// The memoized answer for `key` in `shape`, if one is recorded.
    #[inline(always)]
    pub(super) fn get(&self, shape: u32, key: K) -> Option<V> {
        match self.entries.get(Self::index(shape, key)) {
            Some((recorded, recorded_key, value)) if recorded == shape && recorded_key == key => {
                Some(value)
            }
            _ => None,
        }
    }

    pub(super) fn record(&self, shape: u32, key: K, value: V) {
        self.entries.set(Self::index(shape, key), (shape, key, value));
    }

    pub(super) fn clear(&self) {
        self.entries.clear();
    }

    pub(super) fn fit(&mut self, policy: crate::MemoryPolicy) {
        self.entries.fit(policy);
    }
}

#[cfg(test)]
mod tests {
    use super::DirectMappedTable;
    use crate::MemoryPolicy;

    const ENTRIES: usize = 1 << 6;

    #[test]
    fn a_fitted_table_allocates_its_policy_share_and_keeps_every_index_in_range() {
        for policy in MemoryPolicy::ALL {
            let mut table = DirectMappedTable::<usize, ENTRIES>::default();
            table.set(0, 1);
            table.fit(policy);
            assert_eq!(table.get(0), None, "fitting drops what the default size recorded");
            for index in 0..ENTRIES {
                table.set(index, index);
                assert_eq!(table.get(index), Some(index));
            }
            let allocated = table.entries.get().map_or(0, |entries| entries.len());
            assert_eq!(allocated, ENTRIES >> policy.memo_entry_shift());
        }
    }
}
