//! Memoized answers about non-dictionary shapes. Such a shape never changes once created, so
//! what it says about a key or a slot is a fact of the pair: the memo answers a repeated
//! question with one probe instead of a walk down the shape's transition chain. A symbol key
//! a live shape names is kept alive by that shape (`append_shape_roots`), so no entry can be
//! reached through a reused symbol. Entries are dropped whenever shape ids or atoms are
//! reassigned (shape compaction, VM reset).

use std::cell::Cell;
use std::hash::{Hash, Hasher};

/// Direct-mapped entries: a power of two. Every VM fills each memo when it starts, so the
/// table is resident from then on: memory-first (TOD-123), 1024 entries, a quarter of the
/// 4096 sized for the Node HTTP stack's working set, which cost 80 KiB of anon memory for
/// the two memos in every run.
const SHAPE_MEMO_ENTRIES: usize = 1 << 4;

/// One question kind about shapes: `K` names what is asked of a shape, `V` is the answer.
pub(super) struct ShapeMemo<K: Copy + Eq + Hash, V: Copy> {
    entries: Box<[Cell<Option<(u32, K, V)>>]>,
}

/// The slot a property key has in a shape, or `None` for its absence.
pub(super) type ShapeSlotMemo = ShapeMemo<super::property_key::PropertyKey, Option<u32>>;
/// The attributes a shape gives a slot, or `None` when no transition describes it.
pub(super) type ShapeAttributeMemo = ShapeMemo<u32, Option<super::ShapeAttributes>>;

impl<K: Copy + Eq + Hash, V: Copy> Default for ShapeMemo<K, V> {
    fn default() -> Self {
        Self {
            entries: (0..SHAPE_MEMO_ENTRIES).map(|_| Cell::new(None)).collect(),
        }
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
        match self.entries[Self::index(shape, key)].get() {
            Some((recorded, recorded_key, value)) if recorded == shape && recorded_key == key => {
                Some(value)
            }
            _ => None,
        }
    }

    pub(super) fn record(&self, shape: u32, key: K, value: V) {
        self.entries[Self::index(shape, key)].set(Some((shape, key, value)));
    }

    pub(super) fn clear(&self) {
        for entry in self.entries.iter() {
            entry.set(None);
        }
    }
}
