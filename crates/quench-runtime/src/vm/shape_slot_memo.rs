//! Memoized `property_shape_slot` answers. A non-dictionary shape never changes once created,
//! so the slot a key has in it (or its absence) is a fact of the pair: the memo answers a
//! repeated lookup with one probe instead of a walk down the shape's transition chain. A
//! symbol key a live shape names is kept alive by that shape (`append_shape_roots`), so no
//! entry can be reached through a reused symbol. Entries are dropped whenever shape ids or
//! atoms are reassigned (shape compaction, VM reset).

use super::property_key::PropertyKey;
use std::cell::Cell;
use std::hash::{Hash, Hasher};

/// Direct-mapped entries: a power of two, at 24 bytes each about 96 KiB, which holds the
/// working set of the Node HTTP stack's shaped objects and their prototypes.
const SHAPE_SLOT_MEMO_ENTRIES: usize = 1 << 12;
const NO_MEMO_SHAPE: u32 = u32::MAX;

#[derive(Clone, Copy)]
struct Entry {
    shape: u32,
    key: PropertyKey,
    slot: Option<u32>,
}

const EMPTY: Entry = Entry {
    shape: NO_MEMO_SHAPE,
    key: PropertyKey::String(0),
    slot: None,
};

pub(super) struct ShapeSlotMemo {
    entries: Box<[Cell<Entry>]>,
}

impl Default for ShapeSlotMemo {
    fn default() -> Self {
        Self {
            entries: (0..SHAPE_SLOT_MEMO_ENTRIES)
                .map(|_| Cell::new(EMPTY))
                .collect(),
        }
    }
}

fn index(shape: u32, key: PropertyKey) -> usize {
    let mut hasher = rustc_hash::FxHasher::default();
    (shape, key).hash(&mut hasher);
    hasher.finish() as usize & (SHAPE_SLOT_MEMO_ENTRIES - 1)
}

impl ShapeSlotMemo {
    /// The memoized answer: `Some(slot)` where `slot` is the key's slot or `None` for absence.
    #[inline(always)]
    pub(super) fn get(&self, shape: u32, key: PropertyKey) -> Option<Option<usize>> {
        let entry = self.entries[index(shape, key)].get();
        (entry.shape == shape && entry.key == key).then(|| entry.slot.map(|slot| slot as usize))
    }

    pub(super) fn record(&self, shape: u32, key: PropertyKey, slot: Option<usize>) {
        let Some(slot) = slot.map(u32::try_from).transpose().ok() else {
            return;
        };
        self.entries[index(shape, key)].set(Entry { shape, key, slot });
    }

    pub(super) fn clear(&self) {
        for entry in self.entries.iter() {
            entry.set(EMPTY);
        }
    }
}
