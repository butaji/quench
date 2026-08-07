//! Array method implementations
//!
//! All Array.prototype method implementations organized by category.

use crate::value::Value;

pub mod accessors;
pub mod mutation;
pub mod rearrange;
pub mod search;
pub mod transformation;

// Re-export helpers from transformation (they are used by other modules)
pub use transformation::{call_callback, flatten_array, get_this_array, make_array};

// Re-export from mutation
pub use mutation::{get_this_array_obj, set_elements};

// Re-export method implementations
pub use accessors::{proto_at, proto_concat, proto_join, proto_slice, proto_to_string};
pub use mutation::{proto_pop, proto_push, proto_shift, proto_splice, proto_unshift};
pub use rearrange::{proto_reverse, proto_sort};
pub use search::{
    proto_find, proto_find_last, proto_find_last_index, proto_includes, proto_index_of,
};
pub use transformation::{
    proto_every, proto_filter, proto_flat, proto_flat_map, proto_for_each, proto_map, proto_reduce,
    proto_some,
};

/// Setup all prototype methods on an array prototype object
pub fn setup_prototype_methods(proto: &std::cell::RefCell<crate::value::Object>) {
    use crate::value::{NativeFunction, Value};
    use std::rc::Rc;

    let m = |name: &str, f: fn(Vec<Value>) -> Result<Value, crate::JsError>| {
        proto.borrow_mut().set(
            name,
            Value::NativeFunction(Rc::new(NativeFunction::new_with_name(name, f))),
        );
    };

    setup_transformation_methods(&m);
    setup_mutation_methods(&m);
    setup_rearrange_methods(&m);
    setup_accessor_methods(&m);
    setup_search_methods(&m);
}

fn setup_transformation_methods(
    m: &impl Fn(&str, fn(Vec<Value>) -> Result<Value, crate::JsError>),
) {
    m("map", proto_map);
    m("filter", proto_filter);
    m("forEach", proto_for_each);
    m("reduce", proto_reduce);
    m("some", proto_some);
    m("every", proto_every);
    m("flat", proto_flat);
    m("flatMap", proto_flat_map);
}

fn setup_mutation_methods(m: &impl Fn(&str, fn(Vec<Value>) -> Result<Value, crate::JsError>)) {
    m("push", proto_push);
    m("pop", proto_pop);
    m("shift", proto_shift);
    m("unshift", proto_unshift);
    m("splice", proto_splice);
}

fn setup_rearrange_methods(m: &impl Fn(&str, fn(Vec<Value>) -> Result<Value, crate::JsError>)) {
    m("reverse", proto_reverse);
    m("sort", proto_sort);
}

fn setup_accessor_methods(m: &impl Fn(&str, fn(Vec<Value>) -> Result<Value, crate::JsError>)) {
    m("slice", proto_slice);
    m("concat", proto_concat);
    m("join", proto_join);
    m("toString", proto_to_string);
    m("at", proto_at);
}

fn setup_search_methods(m: &impl Fn(&str, fn(Vec<Value>) -> Result<Value, crate::JsError>)) {
    m("indexOf", proto_index_of);
    m("includes", proto_includes);
    m("find", proto_find);
    m("findLast", proto_find_last);
    m("findLastIndex", proto_find_last_index);
}
