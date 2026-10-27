//! Memoized enumerable string keys of a shape: what `Vm::shape_enumerable_keys` reads from a
//! non-dictionary shape, with each key's slot, for objects whose attributes the shape alone
//! decides (no side-table descriptors). Such a shape never changes, so the answer holds
//! until a collection, which can renumber shapes and so clears every entry.

use super::Atom;
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

pub(super) struct EnumerableKeyMemo {
    entries: RefCell<Box<[Option<(u32, EnumerableSlots)>]>>,
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
        entries.get(Self::set(shape))?.iter().find_map(|entry| match entry {
            Some((recorded, slots)) if *recorded == shape => Some(slots.clone()),
            _ => None,
        })
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
        set[0] = Some((shape, slots));
    }

    pub(super) fn clear(&self) {
        self.entries.borrow_mut().iter_mut().for_each(|entry| *entry = None);
    }
}
