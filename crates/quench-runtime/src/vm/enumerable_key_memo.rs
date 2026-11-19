//! Memoized enumerable string keys of a shape: what `Vm::shape_enumerable_keys` reads from a
//! non-dictionary shape, with each key's slot, for objects whose attributes the shape alone
//! decides (no side-table descriptors). Such a shape never changes, so the answer holds
//! for the shape's life, as V8's enum cache lives on its map: only shape compaction, which
//! renumbers shapes, clears the entries (`Vm::invalidate_field_caches`). Its keys are string
//! atoms, which outlive every collection. Key strings live in the per-shape value memo
//! (`Vm::own_enumerable_key_values_cached`).

use super::Atom;
use std::cell::RefCell;
use std::ops::Deref;
use std::rc::Rc;

/// Sets of `ENUMERABLE_KEY_MEMO_WAYS` entries: a power of two. A request serializes and
/// enumerates a few dozen distinct object shapes; 256 sets hold them.
const ENUMERABLE_KEY_MEMO_SETS: usize = 1 << 8;
/// Entries per set. A for-in reads the receiver's shape and each prototype's, so two shapes
/// meeting in one set read each other out on every loop under one way: a `{a, b, c}` literal
/// whose shape shared `Object.prototype`'s set walked both keys lists per for-in (15.4k Ir for
/// three keys).
const ENUMERABLE_KEY_MEMO_WAYS: usize = 2;

/// A shape's enumerable string keys in OrdinaryOwnPropertyKeys order, with their slots, and
/// whether they are the shape's whole storage in order (`contiguous`): slot `i` is key `i`,
/// so a copy reads the storage's first values in order with no slot lookup. Derefs to the
/// `(key, slot)` pairs.
#[derive(Clone)]
pub(super) struct EnumerableSlots {
    entries: Rc<[(Atom, u32)]>,
    contiguous: bool,
}

impl EnumerableSlots {
    /// The memo fact for `entries` of a shape whose storage holds `storage_len` values: contiguous
    /// when the entries are exactly slots `0..storage_len`, in order.
    pub(super) fn new(entries: Rc<[(Atom, u32)]>, storage_len: usize) -> Self {
        let contiguous = entries.len() == storage_len
            && entries.iter().enumerate().all(|(index, &(_, slot))| slot as usize == index);
        Self { entries, contiguous }
    }

    /// Whether the entries are the shape's first storage slots in order (`new`).
    pub(super) fn contiguous(&self) -> bool {
        self.contiguous
    }
}

impl Deref for EnumerableSlots {
    type Target = [(Atom, u32)];

    /// The `(key, slot)` pairs in order.
    fn deref(&self) -> &Self::Target {
        &self.entries
    }
}

/// One shape's memoized keys: their atoms and slots.
struct MemoEntry {
    shape: u32,
    slots: EnumerableSlots,
}

pub(super) struct EnumerableKeyMemo {
    entries: RefCell<Box<[Option<MemoEntry>]>>,
}

/// Empty until the first record, like `shape_memo::DirectMappedTable`: a program that never
/// enumerates a shape costs no entries.
impl Default for EnumerableKeyMemo {
    fn default() -> Self {
        Self { entries: RefCell::default() }
    }
}

impl EnumerableKeyMemo {
    /// The entries of `shape`'s set.
    fn set(shape: u32) -> std::ops::Range<usize> {
        let first = (shape as usize & (ENUMERABLE_KEY_MEMO_SETS - 1)) * ENUMERABLE_KEY_MEMO_WAYS;
        first..first + ENUMERABLE_KEY_MEMO_WAYS
    }

    pub(super) fn get(&self, shape: u32) -> Option<EnumerableSlots> {
        let entries = self.entries.borrow();
        entries.get(Self::set(shape))?.iter().flatten().find(|entry| entry.shape == shape).map(|entry| entry.slots.clone())
    }

    /// Records `shape` first in its set; the entries before it move one way along and the
    /// last leaves.
    pub(super) fn record(&self, shape: u32, slots: EnumerableSlots) {
        let mut entries = self.entries.borrow_mut();
        if entries.is_empty() {
            *entries = (0..ENUMERABLE_KEY_MEMO_SETS * ENUMERABLE_KEY_MEMO_WAYS).map(|_| None).collect();
        }
        let set = &mut entries[Self::set(shape)];
        set.rotate_right(1);
        set[0] = Some(MemoEntry { shape, slots });
    }

    pub(super) fn clear(&self) {
        self.entries.borrow_mut().iter_mut().for_each(|entry| *entry = None);
    }
}
