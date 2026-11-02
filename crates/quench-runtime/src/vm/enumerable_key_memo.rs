//! Enumerable string-key projections indexed by immutable shape ID. Each projection is
//! derived once per shape epoch; shape mutation creates a new ID, and existing shape
//! invalidation transitions clear the table before IDs can be reused or renumbered.

use super::{Atom, Value};
use std::cell::RefCell;
use std::rc::Rc;

/// A shape's enumerable string keys in OrdinaryOwnPropertyKeys order, with their slots.
pub(super) type EnumerableSlots = Rc<[(Atom, u32)]>;

/// Hot slot metadata and compiler-private read-only views are separate contiguous columns.
/// Each column grows only through its highest populated shape ID; undefined marks a view
/// that has not been materialized. Collection retires weak views; shape invalidation
/// retires both columns together.
#[derive(Default)]
pub(super) struct EnumerableKeyMemo {
    entries: RefCell<Vec<Option<EnumerableSlots>>>,
    snapshots: RefCell<Vec<Value>>,
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

    pub(super) fn snapshot(&self, shape: u32) -> Option<Value> {
        let value = *self.snapshots.borrow().get(shape as usize)?;
        (!value.is_undefined()).then_some(value)
    }

    pub(super) fn record_snapshot(&self, shape: u32, snapshot: Value) {
        let mut snapshots = self.snapshots.borrow_mut();
        let index = shape as usize;
        if snapshots.len() <= index {
            snapshots.resize(index + 1, Value::UNDEFINED);
        }
        snapshots[index] = snapshot;
    }

    /// No lookup can observe a swept handle: collection retires the entire weak view column.
    pub(super) fn clear_snapshots(&self) {
        self.snapshots.borrow_mut().clear();
    }

    pub(super) fn clear(&self) {
        self.entries.borrow_mut().clear();
        self.clear_snapshots();
    }
}
