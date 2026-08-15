//! Runtime-defined overrides for intrinsic prototype properties.
//! calls `Object.defineProperty(Object.prototype, "x", …)` to install getters
//! that should affect primitive lookups too; we record the descriptor here
//! so subsequent reads see it.
use std::{cell::RefCell, collections::HashMap};

use crate::{ops::Builtin, value::Value};

thread_local! {
    static INTRINSIC_OVERRIDES: RefCell<HashMap<(Builtin, String), Value>> =
        RefCell::new(HashMap::new());
    static INTRINSIC_REMOVED: RefCell<HashMap<(Builtin, String), ()>> =
        RefCell::new(HashMap::new());
}

pub(crate) fn read(builtin: Builtin, key: &str) -> Option<Value> {
    let key = canonical_key(key);
    INTRINSIC_OVERRIDES
        .with(|overrides| overrides.borrow().get(&(builtin, key.to_string())).cloned())
}

pub(crate) fn write(builtin: Builtin, key: &str, descriptor: Value) {
    let key = canonical_key(key);
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
    INTRINSIC_OVERRIDES.with(|overrides| {
        overrides.borrow_mut().remove(&(builtin, key.to_string()));
    });
    mark_removed(builtin, key);
}

/// Record that `key` was deleted from `builtin`'s prototype chain so a
/// future hardcoded prototype-chain lookup can observe the deletion.
pub(crate) fn mark_removed(builtin: Builtin, key: &str) {
    let key = canonical_key(key);
    INTRINSIC_REMOVED.with(|removed| {
        removed.borrow_mut().insert((builtin, key.to_string()), ());
    });
}

/// Returns true if JS `delete` has previously removed `key` from `builtin`'s
/// prototype chain in this program.
pub(crate) fn is_removed(builtin: Builtin, key: &str) -> bool {
    let key = canonical_key(key);
    INTRINSIC_REMOVED.with(|removed| removed.borrow().contains_key(&(builtin, key.to_string())))
}

fn canonical_key(key: &str) -> &str {
    key.split_once('\0').map_or(key, |(name, _)| name)
}

/// Drop every cached override and recorded deletion so a fresh program can
/// start with a clean prototype view.
pub(crate) fn reset() {
    INTRINSIC_OVERRIDES.with(|overrides| overrides.borrow_mut().clear());
    INTRINSIC_REMOVED.with(|removed| removed.borrow_mut().clear());
}
