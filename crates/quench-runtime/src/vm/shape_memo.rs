//! Memoized answers about non-dictionary shapes. Such a shape never changes once created, so
//! what it says about a key or a slot is a fact of the pair: the memo answers a repeated
//! question with one probe instead of a walk down the shape's transition chain. A symbol key
//! a live shape names is kept alive by that shape (`append_shape_roots`), so no entry can be
//! reached through a reused symbol. Entries are dropped whenever shape ids or atoms are
//! reassigned (shape compaction, VM reset).

use std::cell::Cell;
use std::hash::{Hash, Hasher};

/// Direct-mapped entries: a power of two, at most 32 bytes each about 128 KiB, which holds
/// the working set of the Node HTTP stack's shaped objects and their prototypes.
const SHAPE_MEMO_ENTRIES: usize = 1 << 12;

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

impl<K: Copy + Eq + Hash, V: Copy> ShapeMemo<K, V> {
    fn index(shape: u32, key: K) -> usize {
        let mut hasher = rustc_hash::FxHasher::default();
        (shape, key).hash(&mut hasher);
        hasher.finish() as usize & (SHAPE_MEMO_ENTRIES - 1)
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
