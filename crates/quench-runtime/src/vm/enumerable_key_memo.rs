//! Enumerable string-key projections indexed by immutable shape ID. Each projection is
//! derived once per shape epoch; shape mutation creates a new ID, and existing shape
//! invalidation transitions clear the table before IDs can be reused or renumbered.

use super::Atom;
use std::cell::RefCell;
use std::rc::Rc;

/// A shape's enumerable string keys in OrdinaryOwnPropertyKeys order, with their slots.
pub(super) type EnumerableSlots = Rc<[(Atom, u32)]>;

/// One optional pointer per shape ID through the highest projected ID. No allocation
/// before the first projection, no collision eviction, and no duplicated shape tags.
#[derive(Default)]
pub(super) struct EnumerableKeyMemo {
    entries: RefCell<Vec<Option<EnumerableSlots>>>,
}

impl EnumerableKeyMemo {
    pub(super) fn get(&self, shape: u32) -> Option<EnumerableSlots> {
        self.entries.borrow().get(shape as usize)?.clone()
    }

    pub(super) fn record(&self, shape: u32, slots: EnumerableSlots) {
        let mut entries = self.entries.borrow_mut();
        let index = shape as usize;
        if entries.len() <= index {
            entries.resize_with(index + 1, || None);
        }
        entries[index] = Some(slots);
    }

    pub(super) fn clear(&self) {
        self.entries.borrow_mut().clear();
    }
}
