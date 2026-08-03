//! Array method implementations
//!
//! All Array.prototype method implementations organized by category.

use crate::value::Value;

pub mod accessors;
pub mod grouping;
pub mod mutation;
pub mod rearrange;
pub mod search;
pub mod transformation;

// Re-export helpers from transformation (they are used by other modules)
pub use transformation::{call_callback, flatten_array, get_this_array, make_array};

// Re-export from mutation
pub use mutation::{get_this_array_obj, set_elements};

// Re-export method implementations
pub use accessors::{
    proto_at, proto_concat, proto_entries, proto_join, proto_keys, proto_slice, proto_to_string,
    proto_values,
};
pub use grouping::{proto_group_by, proto_group_by_to_map};
pub use mutation::{
    proto_pop, proto_push, proto_shift, proto_splice, proto_to_spliced, proto_unshift,
};
pub use rearrange::{
    proto_copy_within, proto_fill, proto_reverse, proto_sort, proto_to_reversed, proto_to_sorted,
};
pub use search::{
    proto_find, proto_find_index, proto_find_last, proto_find_last_index, proto_includes,
    proto_index_of, proto_last_index_of,
};
pub use transformation::{
    proto_every, proto_filter, proto_flat, proto_flat_map, proto_for_each, proto_map, proto_reduce,
    proto_reduce_right, proto_some,
};

/// Setup all prototype methods on an array prototype object
pub fn setup_prototype_methods(proto: &std::cell::RefCell<crate::value::Object>) {
    use crate::value::{NativeFunction, Value};
    use std::rc::Rc;

    let m = |name: &str, f: fn(Vec<Value>) -> Result<Value, crate::JsError>| {
        proto.borrow_mut().set_builtin_method(
            name,
            Value::NativeFunction(Rc::new(NativeFunction::new_with_name(name, f))),
        );
    };

    setup_transformation_methods(&m);
    set_method_length(proto, "every", 1.0);
    setup_mutation_methods(&m);
    setup_rearrange_methods(&m);
    set_method_length(proto, "fill", 1.0);
    setup_accessor_methods(proto, &m);
    setup_search_methods(&m);
    setup_grouping_methods(&m);
}

fn set_method_length(proto: &std::cell::RefCell<crate::value::Object>, name: &str, length: f64) {
    use crate::value::{PropertyFlags, Value};
    if let Some(Value::NativeFunction(function)) = proto.borrow().get(name) {
        function.define_property(
            "length",
            Value::Number(length),
            PropertyFlags {
                value: Some(Value::Number(length)),
                writable: false,
                enumerable: false,
                configurable: true,
            },
        );
    }
}

fn setup_transformation_methods(
    m: &impl Fn(&str, fn(Vec<Value>) -> Result<Value, crate::JsError>),
) {
    m("map", proto_map);
    m("filter", proto_filter);
    m("forEach", proto_for_each);
    m("reduce", proto_reduce);
    m("reduceRight", proto_reduce_right);
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
    m("toSpliced", proto_to_spliced);
}

fn setup_rearrange_methods(m: &impl Fn(&str, fn(Vec<Value>) -> Result<Value, crate::JsError>)) {
    m("copyWithin", proto_copy_within);
    m("fill", proto_fill);
    m("reverse", proto_reverse);
    m("toReversed", proto_to_reversed);
    m("toSorted", proto_to_sorted);
    m("sort", proto_sort);
}

fn setup_accessor_methods(
    proto: &std::cell::RefCell<crate::value::Object>,
    m: &impl Fn(&str, fn(Vec<Value>) -> Result<Value, crate::JsError>),
) {
    use crate::value::{NativeFunction, PropertyFlags, Value};
    use std::rc::Rc;

    m("slice", proto_slice);
    m("concat", proto_concat);
    m("join", proto_join);
    m("toString", proto_to_string);
    let at = Rc::new(NativeFunction::new_with_name("at", proto_at));
    at.define_property(
        "length",
        Value::Number(1.0),
        PropertyFlags {
            value: Some(Value::Number(1.0)),
            writable: false,
            enumerable: false,
            configurable: true,
        },
    );
    proto
        .borrow_mut()
        .set_builtin_method("at", Value::NativeFunction(at));
    if let Some(Value::NativeFunction(concat)) = proto.borrow().get("concat") {
        concat.define_property(
            "length",
            Value::Number(1.0),
            PropertyFlags {
                value: Some(Value::Number(1.0)),
                writable: false,
                enumerable: false,
                configurable: true,
            },
        );
    }
    m("entries", proto_entries);
    m("keys", proto_keys);
    let values = Value::NativeFunction(Rc::new(NativeFunction::new_with_name(
        "values",
        proto_values,
    )));
    proto.borrow_mut().set("values", values.clone());
    if let Some(crate::Value::Symbol(iterator)) =
        crate::builtins::symbol::get_well_known_symbol_no_ctx("iterator")
    {
        proto
            .borrow_mut()
            .set_symbol(&iterator.property_key(), values);
    }
}

fn setup_search_methods(m: &impl Fn(&str, fn(Vec<Value>) -> Result<Value, crate::JsError>)) {
    m("indexOf", proto_index_of);
    m("lastIndexOf", proto_last_index_of);
    m("includes", proto_includes);
    m("find", proto_find);
    m("findIndex", proto_find_index);
    m("findLast", proto_find_last);
    m("findLastIndex", proto_find_last_index);
}

fn setup_grouping_methods(m: &impl Fn(&str, fn(Vec<Value>) -> Result<Value, crate::JsError>)) {
    m("groupBy", proto_group_by);
    m("groupByToMap", proto_group_by_to_map);
}
