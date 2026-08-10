//! Runtime-defined overrides for intrinsic prototype properties. Test262
//! calls `Object.defineProperty(Object.prototype, "x", …)` to install getters
//! that should affect primitive lookups too; we record the descriptor here
//! so subsequent reads see it.
use std::{cell::RefCell, collections::HashMap};

use crate::{ops::Builtin, value::Value};

thread_local! {
    static INTRINSIC_OVERRIDES: RefCell<HashMap<(Builtin, String), Value>> =
        RefCell::new(HashMap::new());
}

pub(crate) fn read(builtin: Builtin, key: &str) -> Option<Value> {
    INTRINSIC_OVERRIDES
        .with(|overrides| overrides.borrow().get(&(builtin, key.to_string())).cloned())
}

pub(crate) fn write(builtin: Builtin, key: &str, descriptor: Value) {
    INTRINSIC_OVERRIDES.with(|overrides| {
        overrides
            .borrow_mut()
            .insert((builtin, key.to_string()), descriptor);
    });
}
