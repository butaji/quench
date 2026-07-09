//! Slot-indexed object arena.
//!
//! The `Context` owns a single `Arena<Object>` so that the interpreter's hot
//! loop can access objects with a bounds-checked index lookup instead of
//! `Rc<RefCell>` borrows.

use std::cell::{Ref, RefCell, RefMut};

/// Opaque handle to an object slot in an `Arena`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ObjectId(pub usize);

/// A simple slot-indexed arena.
pub struct Arena<T> {
    slots: RefCell<Vec<T>>,
}

impl<T> Arena<T> {
    /// Create an empty arena.
    pub fn new() -> Self {
        Self {
            slots: RefCell::new(Vec::new()),
        }
    }

    /// Allocate a new slot and return its id.
    pub fn alloc(&self, value: T) -> ObjectId {
        let mut slots = self.slots.borrow_mut();
        let id = slots.len();
        slots.push(value);
        ObjectId(id)
    }

    /// Get an immutable reference to the value at `id`.
    pub fn get(&self, id: ObjectId) -> Option<Ref<'_, T>> {
        let borrow = self.slots.borrow();
        if id.0 >= borrow.len() {
            drop(borrow);
            return None;
        }
        Some(Ref::map(borrow, |slots| &slots[id.0]))
    }

    /// Get a mutable reference to the value at `id`.
    pub fn get_mut(&self, id: ObjectId) -> Option<RefMut<'_, T>> {
        let borrow = self.slots.borrow_mut();
        if id.0 >= borrow.len() {
            drop(borrow);
            return None;
        }
        Some(RefMut::map(borrow, |slots| &mut slots[id.0]))
    }

    /// Number of allocated slots.
    pub fn len(&self) -> usize {
        self.slots.borrow().len()
    }

    /// Whether the arena is empty.
    pub fn is_empty(&self) -> bool {
        self.slots.borrow().is_empty()
    }
}

impl<T> Default for Arena<T> {
    fn default() -> Self {
        Self::new()
    }
}
