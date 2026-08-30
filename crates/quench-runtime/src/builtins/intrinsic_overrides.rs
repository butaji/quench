//! Runtime-defined overrides for intrinsic prototype properties.
//! calls `Object.defineProperty(Object.prototype, "x", …)` to install getters
//! that should affect primitive lookups too; we record the descriptor here
//! so subsequent reads see it.
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
};

use crate::{ops::Builtin, value::Value};

thread_local! {
    static INTRINSIC_OVERRIDES: RefCell<HashMap<(Builtin, String), Value>> =
        RefCell::new(HashMap::new());
    static INTRINSIC_REMOVED: RefCell<HashSet<(Builtin, String)>> =
        RefCell::new(HashSet::new());
    static INTRINSIC_PROTOTYPE_OVERRIDES: RefCell<HashMap<Builtin, Value>> =
        RefCell::new(HashMap::new());
    static INTRINSIC_NON_EXTENSIBLE: RefCell<HashSet<Builtin>> = RefCell::new(HashSet::new());
    static ARRAY_PROTOTYPE_DIRTY: Cell<bool> = const { Cell::new(false) };
    static GENERATION: Cell<u64> = const { Cell::new(1) };
}

fn mark_array_prototype_dirty(builtin: Builtin) {
    if builtin == Builtin::ArrayPrototype {
        ARRAY_PROTOTYPE_DIRTY.with(|dirty| dirty.set(true));
    }
}

pub(crate) fn array_prototype_is_clean() -> bool {
    ARRAY_PROTOTYPE_DIRTY.with(|dirty| !dirty.get())
}

fn changed() {
    GENERATION.with(|generation| generation.set(generation.get().wrapping_add(1).max(1)));
}

pub(crate) fn generation() -> u64 {
    GENERATION.with(Cell::get)
}

/// Record a `[[Prototype]]` override for the intrinsic `builtin`. Subsequent
/// `get_prototype_of` lookups return the override instead of the default.
pub(crate) fn write_prototype(builtin: Builtin, value: Value) {
    changed();
    mark_array_prototype_dirty(builtin);
    INTRINSIC_PROTOTYPE_OVERRIDES.with(|overrides| {
        overrides.borrow_mut().insert(builtin, value);
    });
}

/// Look up a `[[Prototype]]` override for the intrinsic `builtin`.
pub(crate) fn read_prototype(builtin: Builtin) -> Option<Value> {
    INTRINSIC_PROTOTYPE_OVERRIDES.with(|overrides| overrides.borrow().get(&builtin).cloned())
}

/// Clear the recorded `[[Prototype]]` override for the intrinsic `builtin`.
#[allow(dead_code)]
pub(crate) fn clear_prototype(builtin: Builtin) {
    changed();
    mark_array_prototype_dirty(builtin);
    INTRINSIC_PROTOTYPE_OVERRIDES.with(|overrides| {
        overrides.borrow_mut().remove(&builtin);
    });
}

pub(crate) fn read(builtin: Builtin, key: &str) -> Option<Value> {
    INTRINSIC_OVERRIDES
        .with(|overrides| overrides.borrow().get(&(builtin, key.to_string())).cloned())
}

pub(crate) fn keys(builtin: Builtin) -> Vec<String> {
    INTRINSIC_OVERRIDES.with(|overrides| {
        overrides
            .borrow()
            .keys()
            .filter(|(owner, _)| *owner == builtin)
            .map(|(_, key)| key.clone())
            .collect()
    })
}

/// Whether runtime code has changed any observable state on this intrinsic.
/// Keeping the four mutation classes behind one query lets fast paths guard
/// against prototype drift without duplicating the override representation.
pub(crate) fn has_state(builtin: Builtin) -> bool {
    INTRINSIC_OVERRIDES
        .with(|overrides| overrides.borrow().keys().any(|(owner, _)| *owner == builtin))
        || INTRINSIC_REMOVED.with(|removed| removed.borrow().iter().any(|(owner, _)| *owner == builtin))
        || INTRINSIC_PROTOTYPE_OVERRIDES
            .with(|overrides| overrides.borrow().contains_key(&builtin))
        || INTRINSIC_NON_EXTENSIBLE.with(|values| values.borrow().contains(&builtin))
}

pub(crate) fn write(builtin: Builtin, key: &str, descriptor: Value) {
    changed();
    mark_array_prototype_dirty(builtin);
    INTRINSIC_OVERRIDES.with(|overrides| {
        overrides
            .borrow_mut()
            .insert((builtin, key.to_string()), descriptor);
    });
    INTRINSIC_REMOVED.with(|removed| {
        removed.borrow_mut().remove(&(builtin, key.to_string()));
    });
}

pub(crate) fn remove(builtin: Builtin, key: &str) {
    changed();
    mark_array_prototype_dirty(builtin);
    INTRINSIC_OVERRIDES.with(|overrides| {
        overrides.borrow_mut().remove(&(builtin, key.to_string()));
    });
    mark_removed(builtin, key);
}

/// Record that `key` was deleted from `builtin`'s prototype chain so a
/// future hardcoded prototype-chain lookup can observe the deletion.
pub(crate) fn mark_removed(builtin: Builtin, key: &str) {
    changed();
    mark_array_prototype_dirty(builtin);
    INTRINSIC_REMOVED.with(|removed| {
        removed.borrow_mut().insert((builtin, key.to_string()));
    });
}

/// Returns true if JS `delete` has previously removed `key` from `builtin`'s
/// prototype chain in this program.
pub(crate) fn is_removed(builtin: Builtin, key: &str) -> bool {
    INTRINSIC_REMOVED.with(|removed| removed.borrow().contains(&(builtin, key.to_string())))
}

pub(crate) fn mark_non_extensible(builtin: Builtin) {
    changed();
    mark_array_prototype_dirty(builtin);
    INTRINSIC_NON_EXTENSIBLE.with(|values| {
        values.borrow_mut().insert(builtin);
    });
}

pub(crate) fn is_non_extensible(builtin: Builtin) -> bool {
    INTRINSIC_NON_EXTENSIBLE.with(|values| values.borrow().contains(&builtin))
}

/// Drop every cached override and recorded deletion so a fresh program can
/// start with a clean prototype view.
pub(crate) fn reset() {
    changed();
    ARRAY_PROTOTYPE_DIRTY.with(|dirty| dirty.set(false));
    INTRINSIC_OVERRIDES.with(|overrides| overrides.borrow_mut().clear());
    INTRINSIC_REMOVED.with(|removed| removed.borrow_mut().clear());
    INTRINSIC_PROTOTYPE_OVERRIDES.with(|overrides| overrides.borrow_mut().clear());
    INTRINSIC_NON_EXTENSIBLE.with(|values| values.borrow_mut().clear());
}
