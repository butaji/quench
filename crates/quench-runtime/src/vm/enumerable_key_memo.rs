//! Memoized enumerable string keys of a shape: what `Vm::shape_enumerable_keys` reads from a
//! non-dictionary shape, with each key's slot, for objects whose attributes the shape alone
//! decides (no side-table descriptors). Such a shape never changes, so the answer holds
//! for the shape's life, as V8's enum cache lives on its map: only shape compaction, which
//! renumbers shapes, clears the entries (`Vm::invalidate_field_caches`). Its keys are string
//! atoms, which outlive every collection.
//!
//! Beside the atoms an entry may hold each key's string, as `Object.keys` returns them
//! (`Vm::plain_object_keys`), so a call copies them into its array in one pass. Those strings
//! are weak, no collection root: a string lives at least until the next collection, so the
//! entry records the collection epoch (`Heap::collection_epoch`) it was made in and answers
//! only within it. After a collection the strings are made again from the atoms.

use super::Atom;
use crate::Value;
use std::cell::RefCell;
use std::rc::Rc;

/// Sets of `ENUMERABLE_KEY_MEMO_WAYS` entries: a power of two. A request serializes and
/// enumerates a few dozen distinct object shapes; 256 sets hold them.
const ENUMERABLE_KEY_MEMO_SETS: usize = 1 << 8;
/// Entries per set. A for-in reads the receiver's shape and each prototype's, so two shapes
/// meeting in one set read each other out on every loop under one way: a `{a, b, c}` literal
/// whose shape shared `Object.prototype`'s set walked both keys lists per for-in (15.4k Ir for
/// three keys).
const ENUMERABLE_KEY_MEMO_WAYS: usize = 2;

/// A shape's enumerable string keys in OrdinaryOwnPropertyKeys order, with their slots.
pub(super) type EnumerableSlots = Rc<[(Atom, u32)]>;

/// One shape's memoized keys: their atoms and slots, and their strings once a call made them.
struct MemoEntry {
    shape: u32,
    slots: EnumerableSlots,
    key_strings: Option<KeyStrings>,
}

/// The strings of an entry's keys, in its order, live while the collection epoch is `epoch`.
struct KeyStrings {
    epoch: u64,
    strings: Box<[Value]>,
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

    /// `read` of `shape`'s key strings, where an entry holds them made in collection epoch
    /// `epoch`; `None` where none does, or they were made before a collection since.
    #[inline]
    pub(super) fn with_key_strings<R>(&self, shape: u32, epoch: u64, read: impl FnOnce(&[Value]) -> R) -> Option<R> {
        let entries = self.entries.borrow();
        let entry = entries.get(Self::set(shape))?.iter().flatten().find(|entry| entry.shape == shape)?;
        let key_strings = entry.key_strings.as_ref().filter(|key_strings| key_strings.epoch == epoch)?;
        Some(read(&key_strings.strings[..]))
    }

    /// Records `strings`, made in collection epoch `epoch`, as the key strings of `shape`'s
    /// entry, in its order; nothing where the entry has left its set.
    pub(super) fn record_key_strings(&self, shape: u32, epoch: u64, strings: Box<[Value]>) {
        let mut entries = self.entries.borrow_mut();
        let Some(set) = entries.get_mut(Self::set(shape)) else {
            return;
        };
        if let Some(entry) = set.iter_mut().flatten().find(|entry| entry.shape == shape) {
            entry.key_strings = Some(KeyStrings { epoch, strings });
        }
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
        set[0] = Some(MemoEntry { shape, slots, key_strings: None });
    }

    pub(super) fn clear(&self) {
        self.entries.borrow_mut().iter_mut().for_each(|entry| *entry = None);
    }
}
