//! Snapshot/restore of every thread-local realm intrinsic cache.
//!
//! `Context::new()` and harness injection overwrite shared thread-local
//! intrinsic caches (prototype pointers, well-known symbols, harness globals).
//! `$262.createRealm()` builds a sub-realm on the same thread, so without a
//! snapshot the main realm's caches end up pointing at sub-realm objects.
//! `IntrinsicSnapshot::save()`/`restore()` bracket sub-realm creation;
//! `clear_intrinsics()` empties all of them for `Context::reset`.

use crate::builtins;
use crate::value::{Object, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

type Proto = Option<Rc<RefCell<Object>>>;

/// All thread-local intrinsic caches that a fresh realm overwrites.
pub struct IntrinsicSnapshot {
    array: Proto,
    object: Proto,
    string: Proto,
    regexp: Proto,
    typed_array: Proto,
    promise: Proto,
    iterator: Proto,
    functions: builtins::function::FunctionPrototypes,
    errors: crate::value::error::ErrorIntrinsics,
    well_known_symbols: HashMap<&'static str, Value>,
    regex_cache: rustc_hash::FxHashMap<char, Value>,
    throw_type_error: Option<Value>,
}

impl IntrinsicSnapshot {
    /// Snapshot every thread-local intrinsic cache.
    pub fn save() -> Self {
        IntrinsicSnapshot {
            array: builtins::array::save_array_prototype(),
            object: builtins::object::save_object_prototype(),
            string: builtins::string::save_string_prototype(),
            regexp: builtins::regex::save_regexp_prototype(),
            typed_array: builtins::typed_array::save_typed_array_prototype(),
            promise: builtins::promise::save_promise_proto(),
            iterator: builtins::iterator::save_iterator_prototype(),
            functions: builtins::function::save_function_prototypes(),
            errors: crate::value::error::save_error_intrinsics(),
            well_known_symbols: builtins::symbol::save_well_known_symbols(),
            regex_cache: super::helpers::save_regex_cache(),
            throw_type_error: crate::eval::function::save_throw_type_error(),
        }
    }

    /// Restore every thread-local intrinsic cache from the snapshot.
    pub fn restore(self) {
        builtins::array::restore_array_prototype(self.array);
        builtins::object::restore_object_prototype(self.object);
        builtins::string::restore_string_prototype(self.string);
        builtins::regex::restore_regexp_prototype(self.regexp);
        builtins::typed_array::restore_typed_array_prototype(self.typed_array);
        builtins::promise::restore_promise_proto(self.promise);
        builtins::iterator::restore_iterator_prototype(self.iterator);
        builtins::function::restore_function_prototypes(self.functions);
        crate::value::error::restore_error_intrinsics(self.errors);
        builtins::symbol::restore_well_known_symbols(self.well_known_symbols);
        super::helpers::restore_regex_cache(self.regex_cache);
        crate::eval::function::restore_throw_type_error(self.throw_type_error);
    }
}

/// Clear every thread-local intrinsic cache (called by `Context::reset`).
pub(crate) fn clear_intrinsics() {
    IntrinsicSnapshot {
        array: None,
        object: None,
        string: None,
        regexp: None,
        typed_array: None,
        promise: None,
        iterator: None,
        functions: Default::default(),
        errors: (None, None, None, None),
        well_known_symbols: HashMap::new(),
        regex_cache: rustc_hash::FxHashMap::default(),
        throw_type_error: None,
    }
    .restore();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::ObjectKind;

    #[test]
    fn test_snapshot_restore_round_trip() {
        let saved = IntrinsicSnapshot::save();
        let replacement = Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary)));
        builtins::array::restore_array_prototype(Some(Rc::clone(&replacement)));
        assert!(Rc::ptr_eq(
            &builtins::array::get_array_prototype().unwrap(),
            &replacement
        ));
        saved.restore();
        assert_ne!(
            builtins::array::get_array_prototype()
                .as_ref()
                .map(Rc::as_ptr),
            Some(Rc::as_ptr(&replacement))
        );
    }

    #[test]
    fn test_clear_intrinsics_empties_caches() {
        let mut ctx = crate::Context::new().unwrap();
        assert!(builtins::array::get_array_prototype().is_some());
        clear_intrinsics();
        assert!(builtins::array::get_array_prototype().is_none());
        assert!(builtins::object::get_object_prototype().is_none());
        assert!(builtins::string::get_string_prototype().is_none());
        assert!(builtins::function::get_function_prototype().is_none());
        assert!(crate::value::error::get_host_error().is_none());
        // Rebuild so later tests on this thread see initialized caches.
        ctx.reset().unwrap();
        assert!(builtins::array::get_array_prototype().is_some());
    }
}
