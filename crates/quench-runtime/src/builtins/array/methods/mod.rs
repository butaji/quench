//! Array method implementations
//!
//! All Array.prototype method implementations organized by category.

pub mod accessors;
pub mod mutation;
pub mod rearrange;
pub mod search;
pub mod transformation;

// Re-export helpers from transformation (they are used by other modules)
pub use transformation::{get_this_array, make_array, call_callback, flatten_array};

// Re-export from mutation
pub use mutation::{get_this_array_obj, set_elements};

// Re-export method implementations
pub use transformation::{proto_map, proto_filter, proto_for_each, proto_reduce, proto_some, proto_every, proto_flat, proto_flat_map};
pub use mutation::{proto_push, proto_pop, proto_shift, proto_unshift, proto_splice};
pub use rearrange::{proto_reverse, proto_sort};
pub use accessors::{proto_slice, proto_concat, proto_join, proto_to_string};
pub use search::{proto_index_of, proto_includes, proto_find};

/// Setup all prototype methods on an array prototype object
pub fn setup_prototype_methods(proto: &std::cell::RefCell<crate::value::Object>) {
    use std::rc::Rc;
    use crate::value::{NativeFunction, Value};

    let m = |name: &str, f: fn(Vec<Value>) -> Result<Value, crate::JsError>| {
        proto.borrow_mut().set(
            name,
            Value::NativeFunction(Rc::new(NativeFunction::new(f))),
        );
    };

    // Transformation methods
    m("map", proto_map);
    m("filter", proto_filter);
    m("forEach", proto_for_each);
    m("reduce", proto_reduce);
    m("some", proto_some);
    m("every", proto_every);
    m("flat", proto_flat);
    m("flatMap", proto_flat_map);

    // Mutation methods
    m("push", proto_push);
    m("pop", proto_pop);
    m("shift", proto_shift);
    m("unshift", proto_unshift);
    m("splice", proto_splice);

    // Rearrange methods
    m("reverse", proto_reverse);
    m("sort", proto_sort);

    // Accessor methods
    m("slice", proto_slice);
    m("concat", proto_concat);
    m("join", proto_join);
    m("toString", proto_to_string);

    // Search methods
    m("indexOf", proto_index_of);
    m("includes", proto_includes);
    m("find", proto_find);
}
