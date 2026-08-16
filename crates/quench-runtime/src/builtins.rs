mod builtins_cells;
mod intrinsic_overrides;
pub mod object;
pub mod object_alias;
pub mod props;
use crate::{
    ops::Builtin,
    value::{ObjectData, Value},
};
use intrinsic_overrides as overrides;
use std::collections::HashMap;
use std::rc::Rc;
const DESCRIPTOR_PREFIX: &str = "\0quench:descriptor:\0";
const DELETED_PREFIX: &str = "\0quench:deleted:\0";
pub(crate) const ERROR_SLOT: &str = "\0error_slot";

thread_local! {
    static GENERATOR_PROTOTYPES:
        std::cell::RefCell<HashMap<crate::ops::RealmId, Value>> =
        std::cell::RefCell::new(HashMap::new());
    static ASYNC_ITERATOR_PROTOTYPES:
        std::cell::RefCell<HashMap<crate::ops::RealmId, Value>> =
        std::cell::RefCell::new(HashMap::new());
    static ASYNC_GENERATOR_PROTOTYPES:
        std::cell::RefCell<HashMap<crate::ops::RealmId, Value>> =
        std::cell::RefCell::new(HashMap::new());
}

pub(crate) fn async_iterator_prototype() -> Value {
    let realm = crate::vm::current_context_or_default().realm();
    ASYNC_ITERATOR_PROTOTYPES.with(|cell| {
        if let Some(value) = cell.borrow().get(&realm) {
            return value.clone();
        }
        let mut value = Value::Object(Rc::new(ObjectData::new(vec![
            (
                "Symbol.asyncIterator".to_string(),
                Value::Builtin(Builtin::AsyncIteratorSelf),
            ),
            (
                "Symbol.asyncDispose".to_string(),
                Value::Builtin(Builtin::AsyncIteratorDispose),
            ),
            (
                "\0prototype".to_string(),
                Value::Builtin(Builtin::ObjectPrototype),
            ),
        ])));
        for (key, method) in [
            ("Symbol.asyncIterator", Builtin::AsyncIteratorSelf),
            ("Symbol.asyncDispose", Builtin::AsyncIteratorDispose),
        ] {
            store_descriptor_metadata(
                &mut value,
                key,
                &async_iterator_descriptor(Value::Builtin(method)),
            );
        }
        cell.borrow_mut().insert(realm, value.clone());
        value
    })
}

fn async_iterator_descriptor(value: Value) -> Vec<(String, Value)> {
    vec![
        ("value".to_string(), value),
        ("writable".to_string(), Value::Boolean(true)),
        ("enumerable".to_string(), Value::Boolean(false)),
        ("configurable".to_string(), Value::Boolean(true)),
    ]
}

pub(crate) fn generator_prototype() -> Value {
    let realm = crate::vm::current_context_or_default().realm();
    GENERATOR_PROTOTYPES.with(|cell| {
        if let Some(value) = cell.borrow().get(&realm) {
            return value.clone();
        }
        let mut value = Value::Object(Rc::new(ObjectData::new(vec![
            (
                "constructor".to_string(),
                Value::Builtin(Builtin::GeneratorFunctionPrototype),
            ),
            ("next".to_string(), Value::Builtin(Builtin::GeneratorNext)),
            (
                "return".to_string(),
                Value::Builtin(Builtin::GeneratorReturn),
            ),
            ("throw".to_string(), Value::Builtin(Builtin::GeneratorThrow)),
            (
                "Symbol.toStringTag".to_string(),
                Value::String("Generator".to_string()),
            ),
            (
                "\0prototype".to_string(),
                Value::Builtin(Builtin::ObjectPrototype),
            ),
        ])));
        store_descriptor_metadata(
            &mut value,
            "constructor",
            &[
                (
                    "value".to_string(),
                    Value::Builtin(Builtin::GeneratorFunctionPrototype),
                ),
                ("writable".to_string(), Value::Boolean(false)),
                ("enumerable".to_string(), Value::Boolean(false)),
                ("configurable".to_string(), Value::Boolean(true)),
            ],
        );
        for (key, method) in [
            ("next", Builtin::GeneratorNext),
            ("return", Builtin::GeneratorReturn),
            ("throw", Builtin::GeneratorThrow),
        ] {
            store_descriptor_metadata(
                &mut value,
                key,
                &[
                    ("value".to_string(), Value::Builtin(method)),
                    ("writable".to_string(), Value::Boolean(true)),
                    ("enumerable".to_string(), Value::Boolean(false)),
                    ("configurable".to_string(), Value::Boolean(true)),
                ],
            );
        }
        store_descriptor_metadata(
            &mut value,
            "Symbol.toStringTag",
            &[
                ("value".to_string(), Value::String("Generator".to_string())),
                ("writable".to_string(), Value::Boolean(false)),
                ("enumerable".to_string(), Value::Boolean(false)),
                ("configurable".to_string(), Value::Boolean(true)),
            ],
        );
        cell.borrow_mut().insert(realm, value.clone());
        value
    })
}

pub(crate) fn async_generator_prototype() -> Value {
    let realm = crate::vm::current_context_or_default().realm();
    ASYNC_GENERATOR_PROTOTYPES.with(|cell| {
        if let Some(value) = cell.borrow().get(&realm) {
            return value.clone();
        }
        let mut value = Value::Object(Rc::new(ObjectData::new(vec![
            (
                "next".to_string(),
                Value::Builtin(Builtin::AsyncGeneratorNext),
            ),
            (
                "return".to_string(),
                Value::Builtin(Builtin::AsyncGeneratorReturn),
            ),
            (
                "throw".to_string(),
                Value::Builtin(Builtin::AsyncGeneratorThrow),
            ),
            (
                "Symbol.asyncIterator".to_string(),
                Value::Builtin(Builtin::AsyncIteratorSelf),
            ),
            (
                "Symbol.asyncDispose".to_string(),
                Value::Builtin(Builtin::AsyncIteratorDispose),
            ),
            (
                "Symbol.toStringTag".to_string(),
                Value::String("AsyncGenerator".to_string()),
            ),
            (
                "constructor".to_string(),
                crate::vm::realm_intrinsic(Builtin::AsyncGeneratorFunctionPrototype),
            ),
            ("\0prototype".to_string(), async_iterator_prototype()),
        ])));
        for (key, method) in [
            ("next", Builtin::AsyncGeneratorNext),
            ("return", Builtin::AsyncGeneratorReturn),
            ("throw", Builtin::AsyncGeneratorThrow),
        ] {
            store_descriptor_metadata(
                &mut value,
                key,
                &[
                    ("value".to_string(), Value::Builtin(method)),
                    ("writable".to_string(), Value::Boolean(true)),
                    ("enumerable".to_string(), Value::Boolean(false)),
                    ("configurable".to_string(), Value::Boolean(true)),
                ],
            );
        }
        for key in ["Symbol.asyncIterator", "Symbol.asyncDispose"] {
            let method = match key {
                "Symbol.asyncIterator" => Value::Builtin(Builtin::AsyncIteratorSelf),
                _ => Value::Builtin(Builtin::AsyncIteratorDispose),
            };
            store_descriptor_metadata(
                &mut value,
                key,
                &[
                    ("value".to_string(), method),
                    ("writable".to_string(), Value::Boolean(true)),
                    ("enumerable".to_string(), Value::Boolean(false)),
                    ("configurable".to_string(), Value::Boolean(true)),
                ],
            );
        }
        store_descriptor_metadata(
            &mut value,
            "Symbol.toStringTag",
            &[
                (
                    "value".to_string(),
                    Value::String("AsyncGenerator".to_string()),
                ),
                ("writable".to_string(), Value::Boolean(false)),
                ("enumerable".to_string(), Value::Boolean(false)),
                ("configurable".to_string(), Value::Boolean(true)),
            ],
        );
        store_descriptor_metadata(
            &mut value,
            "constructor",
            &[
                (
                    "value".to_string(),
                    crate::vm::realm_intrinsic(Builtin::AsyncGeneratorFunctionPrototype),
                ),
                ("writable".to_string(), Value::Boolean(false)),
                ("enumerable".to_string(), Value::Boolean(false)),
                ("configurable".to_string(), Value::Boolean(true)),
            ],
        );
        cell.borrow_mut().insert(realm, value.clone());
        value
    })
}

pub(crate) fn deleted_key(key: &str) -> String {
    format!("{DELETED_PREFIX}{key}")
}

pub(crate) fn descriptor_key(key: &str) -> String {
    format!("{DESCRIPTOR_PREFIX}{key}")
}
pub(crate) fn is_descriptor_key(key: &str) -> bool {
    key.starts_with(DESCRIPTOR_PREFIX)
}
pub(crate) fn read_intrinsic_override(builtin: Builtin, key: &str) -> Option<Value> {
    overrides::read(builtin, key)
}

pub(crate) fn intrinsic_override_keys(builtin: Builtin) -> Vec<String> {
    overrides::keys(builtin)
}

/// Read the data value of a runtime-defined intrinsic property override, if
/// the recorded descriptor carries one. Accessor descriptors are left to the
/// caller to invoke.
pub(crate) fn read_descriptor_value(builtin: Builtin, key: &str) -> Option<Value> {
    let Value::Object(properties) = read_intrinsic_override(builtin, key)? else {
        return None;
    };
    properties
        .iter()
        .rev()
        .find(|(name, _)| name == "value")
        .map(|(_, value)| value.clone())
}

pub(crate) fn write_intrinsic_override(builtin: Builtin, key: &str, descriptor: Value) {
    overrides::write(builtin, key, descriptor)
}

pub(crate) fn remove_intrinsic_override(builtin: Builtin, key: &str) {
    overrides::remove(builtin, key)
}

/// Mark `key` as deleted from `builtin`'s prototype chain so a future
/// hardcoded lookup for that combination observes the removal.
pub(crate) fn mark_builtin_prototype_property_removed(builtin: Builtin, key: &str) {
    overrides::mark_removed(builtin, key)
}

/// Returns true if JS `delete` has previously removed `key` from `builtin`'s
/// prototype chain in the current program.
pub(crate) fn builtin_prototype_property_is_removed(builtin: Builtin, key: &str) -> bool {
    overrides::is_removed(builtin, key)
}

/// Drop every cached intrinsic-property override and recorded deletion so a
/// fresh program can start with a clean prototype view.
pub fn reset_intrinsic_prototype_state() {
    overrides::reset()
}

pub(crate) fn property(builtin: Builtin, key: &str) -> Value {
    let value = props::lookup(builtin, key);
    if !matches!(value, Value::Undefined) {
        return value;
    }
    let json = crate::json::method_property(builtin, key);
    if !matches!(json, Value::Undefined) {
        return json;
    }
    crate::builtin_meta::prototype(builtin)
        .filter(|prototype| *prototype != builtin)
        .map_or(Value::Undefined, |prototype| property(prototype, key))
}

pub(crate) fn special_property(builtin: Builtin, key: &str) -> Option<Value> {
    props::special_property(builtin, key).or_else(|| {
        match crate::json::method_property(builtin, key) {
            Value::Undefined => None,
            value => Some(value),
        }
    })
}

pub(crate) fn callable_property(builtin: Builtin, key: &str) -> Option<Value> {
    props::callable(builtin, key)
}

pub(crate) fn own_property_names(builtin: Builtin) -> &'static [&'static str] {
    props::own_property_names(builtin)
}

pub(crate) fn builtin_name(builtin: Builtin) -> &'static str {
    props::builtin_name(builtin)
}

include!("builtins_escape.rs");
include!("builtins_uri.rs");
include!("builtins_core.rs");
include!("builtins_array_shift.rs");
include!("builtins_array_reverse.rs");
include!("builtins_array_pop.rs");
include!("builtins_array_unshift.rs");
include!("builtins_array_fill.rs");
include!("builtins_array_copy_within.rs");
include!("builtins_array_find_last.rs");
include!("builtins_array_to_sorted.rs");
pub(crate) fn math_pow(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let base = arguments
        .first()
        .map_or(Ok(f64::NAN), crate::conversion::to_number)?;
    let exponent = arguments
        .get(1)
        .map_or(Ok(f64::NAN), crate::conversion::to_number)?;
    Ok(Value::Number(pow(base, exponent)))
}

include!("builtins_object_core.rs");
pub(crate) fn error(builtin: Builtin, arguments: &[Value]) -> Value {
    let (name, constructor, prototype) = error_parts(builtin);
    let constructor_builtin = constructor;
    let constructor = crate::vm::realm_intrinsic(constructor_builtin);
    let prototype_builtin =
        crate::builtin_meta::instance_prototype(constructor_builtin).unwrap_or(prototype);
    let prototype = crate::vm::realm_intrinsic(prototype_builtin);
    let message = arguments.first().map_or_else(String::new, value_to_string);
    let mut properties = vec![
        ("name".to_string(), Value::String(name.to_string())),
        ("message".to_string(), Value::String(message)),
        ("constructor".to_string(), constructor),
        (ERROR_SLOT.to_string(), Value::Boolean(true)),
        ("\0prototype".to_string(), prototype),
    ];
    if let Some(Value::Object(existing)) = arguments.first() {
        properties.extend(existing.properties.clone());
    }
    Value::Object(Rc::new(ObjectData::new(properties)))
}

include!("builtins_error_helpers.rs");
fn set_property_tail(target: Value, key: &str, value: Value) -> Value {
    match target {
        Value::BoundFunction(bound)
            if bound.target == Value::Builtin(crate::ops::Builtin::AbstractModuleSource)
                && matches!(key, "length" | "name" | "prototype") =>
        {
            Value::BoundFunction(bound)
        }
        Value::BoundFunction(bound) => {
            {
                let mut properties = bound.properties.borrow_mut();
                properties.retain(|(name, _)| name != key);
                properties.push((key.to_string(), value));
            }
            Value::BoundFunction(bound)
        }
        Value::DataView(view) => {
            view.set_own_property(key, value);
            Value::DataView(view)
        }
        Value::ArrayBuffer(buffer) => {
            let value = match value {
                Value::Object(object) => crate::builtins::object_alias::alias(&object),
                value => value,
            };
            buffer.set_own_property(key, value);
            Value::ArrayBuffer(buffer)
        }
        other => other,
    }
}

include!("builtins_string_core.rs");
include!("builtins_property_helpers.rs");
fn set_function_property(
    function: Rc<crate::value::FunctionValue>,
    key: &str,
    value: Value,
) -> Value {
    if descriptor_flag_in(&function.properties.borrow(), key, "writable") == Some(false) {
        return Value::Function(function);
    }
    {
        let mut properties = function.properties.borrow_mut();
        if let Some((_, current)) = properties.iter_mut().rev().find(|(name, _)| name == key) {
            *current = value;
        } else {
            properties.push((key.to_string(), value));
        }
    }
    Value::Function(function)
}

include!("builtins/function_name.rs");
include!("builtins_prototype.rs");
include!("builtins_value_string.rs");
