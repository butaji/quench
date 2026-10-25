//! Memoized enumerable string keys of a shape: what `Vm::shape_enumerable_keys` reads from a
//! non-dictionary shape, with each key's slot, for objects whose attributes the shape alone
//! decides (no side-table descriptors). Such a shape never changes, so the answer holds
//! until a collection, which can renumber shapes and so clears every entry.

use super::Atom;
use std::cell::RefCell;
use std::rc::Rc;

/// Direct-mapped entries: a power of two. A request serializes and enumerates a few dozen
/// distinct object shapes; 256 entries hold them.
const ENUMERABLE_KEY_MEMO_ENTRIES: usize = 1 << 8;

/// A shape's enumerable string keys in OrdinaryOwnPropertyKeys order, with their slots.
pub(super) type EnumerableSlots = Rc<[(Atom, u32)]>;

pub(super) struct EnumerableKeyMemo {
    entries: RefCell<Box<[Option<(u32, EnumerableSlots)>]>>,
}

/// Empty until the first record, like `shape_memo::DirectMappedTable`: a program that never
/// enumerates a shape costs no entries.
impl Default for EnumerableKeyMemo {
    fn default() -> Self {
        Self {
            entries: RefCell::default(),
        }
    }
}

impl EnumerableKeyMemo {
    fn index(shape: u32) -> usize {
        shape as usize & (ENUMERABLE_KEY_MEMO_ENTRIES - 1)
    }

    pub(super) fn get(&self, shape: u32) -> Option<EnumerableSlots> {
        match self.entries.borrow().get(Self::index(shape)) {
            Some(Some((recorded, slots))) if *recorded == shape => Some(slots.clone()),
            _ => None,
        }
    }

    pub(super) fn record(&self, shape: u32, slots: EnumerableSlots) {
        let mut entries = self.entries.borrow_mut();
        if entries.is_empty() {
            *entries = (0..ENUMERABLE_KEY_MEMO_ENTRIES).map(|_| None).collect();
        }
        entries[Self::index(shape)] = Some((shape, slots));
    }

    pub(super) fn clear(&self) {
        self.entries
            .borrow_mut()
            .iter_mut()
            .for_each(|entry| *entry = None);
    }
}
