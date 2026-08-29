//! Live instances keyed by a per-thread id so funcrefs stay `Copy`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use super::Inner;

thread_local! {
    static NEXT: RefCell<u32> = const { RefCell::new(1) };
    static LIVE: RefCell<HashMap<u32, Weak<Inner>>> = RefCell::new(HashMap::new());
}

pub fn alloc_id() -> u32 {
    NEXT.with(|next| {
        let mut next = next.borrow_mut();
        let id = *next;
        *next = id.wrapping_add(1).max(1);
        id
    })
}

pub fn register(id: u32, inner: &Rc<Inner>) {
    LIVE.with(|live| {
        live.borrow_mut().insert(id, Rc::downgrade(inner));
    });
}

pub fn unregister(id: u32) {
    LIVE.with(|live| {
        live.borrow_mut().remove(&id);
    });
}

pub fn get(id: u32) -> Option<Rc<Inner>> {
    LIVE.with(|live| live.borrow().get(&id).and_then(Weak::upgrade))
}

thread_local! {
    static PINNED: RefCell<Vec<Rc<Inner>>> = RefCell::new(Vec::new());
}

/// Keep a failed instantiate alive so funcrefs written into imported tables remain.
pub fn pin(inner: Rc<Inner>) {
    PINNED.with(|pinned| pinned.borrow_mut().push(inner));
}
