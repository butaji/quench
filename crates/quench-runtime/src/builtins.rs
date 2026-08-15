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
    static ASYNC_GENERATOR_PROTOTYPES:
        std::cell::RefCell<HashMap<crate::ops::RealmId, Value>> =
        std::cell::RefCell::new(HashMap::new());
}

pub(crate) fn async_generator_prototype() -> Value {
    let realm = crate::vm::current_context_or_default().realm();
    ASYNC_GENERATOR_PROTOTYPES.with(|cell| {
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

fn error_parts(builtin: Builtin) -> (&'static str, Builtin, Builtin) {
    match builtin {
        Builtin::RangeError => ("RangeError", Builtin::RangeError, Builtin::ErrorPrototype),
        Builtin::ReferenceError => (
            "ReferenceError",
            Builtin::ReferenceError,
            Builtin::ErrorPrototype,
        ),
        Builtin::SyntaxError => ("SyntaxError", Builtin::SyntaxError, Builtin::ErrorPrototype),
        Builtin::EvalError => ("EvalError", Builtin::EvalError, Builtin::ErrorPrototype),
        Builtin::URIError => ("URIError", Builtin::URIError, Builtin::ErrorPrototype),
        Builtin::AggregateError => (
            "AggregateError",
            Builtin::AggregateError,
            Builtin::ErrorPrototype,
        ),
        Builtin::TypeError => ("TypeError", Builtin::TypeError, Builtin::ErrorPrototype),
        Builtin::SuppressedError => (
            "SuppressedError",
            Builtin::SuppressedError,
            Builtin::ErrorPrototype,
        ),
        Builtin::Error => ("Error", Builtin::Error, Builtin::ErrorPrototype),
        _ => ("Error", Builtin::Error, Builtin::ErrorPrototype),
    }
}

pub(crate) fn suppressed_error(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let error = arguments.first().cloned().unwrap_or(Value::Undefined);
    let suppressed = arguments.get(1).cloned().unwrap_or(Value::Undefined);
    let message = arguments
        .get(2)
        .filter(|value| !matches!(value, Value::Undefined))
        .map(crate::conversion::to_string)
        .transpose()?;
    let mut properties = vec![
        (
            "name".to_string(),
            Value::String("SuppressedError".to_string()),
        ),
        (
            "\0prototype".to_string(),
            Value::Builtin(Builtin::SuppressedErrorPrototype),
        ),
    ];
    let mut data_properties = Vec::new();
    if let Some(message) = message {
        data_properties.push(("message".to_string(), Value::String(message)));
    }
    data_properties.push(("error".to_string(), error));
    data_properties.push(("suppressed".to_string(), suppressed));
    for (key, value) in data_properties {
        properties.push((descriptor_key(&key), non_enumerable_descriptor(&value)));
        properties.push((key, value));
    }
    properties.push((
        "constructor".to_string(),
        Value::Builtin(Builtin::SuppressedError),
    ));
    properties.push((
        crate::builtins::ERROR_SLOT.to_string(),
        Value::Boolean(true),
    ));
    Ok(Value::Object(Rc::new(ObjectData::new(properties))))
}

include!("builtins_descriptor_core.rs");
pub(crate) fn same_value(left: Option<&Value>, right: Option<&Value>) -> bool {
    let (Some(left), Some(right)) = (left, right) else {
        return matches!((left, right), (None, None));
    };
    if let (Value::Number(left), Value::Number(right)) = (left, right) {
        return (left.is_nan() && right.is_nan())
            || (left == right && left.is_sign_negative() == right.is_sign_negative());
    }
    same_value_objects(left, right)
}

pub(crate) fn set_property(target: Value, key: &str, value: Value) -> Value {
    if let Some(result) = crate::typed_array_prototype::set(&target, key, value.clone()) {
        return result;
    }
    if let Some(result) = crate::typed_array_ops::set_property(&target, key, &value) {
        return result.unwrap_or(target);
    }
    if let Some(result) = set_prototype_slot(&target, key, value.clone()) {
        return result;
    }
    if let Some(result) = set_promise_property(&target, key, value.clone()) {
        return result;
    }
    match target {
        Value::Object(properties) if boxed_string_immutable_key(&properties, key) => {
            Value::Object(properties)
        }
        Value::Object(properties)
            if descriptor_flag_in(&properties, key, "writable") == Some(false) =>
        {
            Value::Object(properties)
        }
        Value::Object(properties) => builtins_cells::set_object_property(properties, key, value),
        Value::ObjectAlias(alias) => set_object_alias_property(alias, key, value),
        Value::Array(values) if array_descriptor_flag(&values, key, "writable") == Some(false) => {
            Value::Array(values)
        }
        Value::Array(values) => set_array_property(values, key, value),
        Value::Function(function) => set_function_property(function, key, value),
        _ => set_property_tail(target, key, value),
    }
}

fn set_property_tail(target: Value, key: &str, value: Value) -> Value {
    match target {
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
fn set_object_alias_property(
    alias: crate::value::ObjectAliasValue,
    key: &str,
    value: Value,
) -> Value {
    let Some(properties) = alias.0.borrow().upgrade() else {
        return Value::ObjectAlias(alias);
    };
    let result = builtins_cells::set_object_property(properties, key, value);
    retarget_object_alias(&alias, &result);
    result
}

fn retarget_object_alias(alias: &crate::value::ObjectAliasValue, value: &Value) {
    let Value::Object(object) = value else { return };
    *alias.0.borrow_mut() = Rc::downgrade(object);
}

fn set_prototype_slot(target: &Value, key: &str, value: Value) -> Option<Value> {
    if key != "\0prototype" {
        return None;
    }
    Some(match target {
        Value::ArrayBuffer(buffer) => {
            buffer.set_prototype(value);
            Value::ArrayBuffer(buffer.clone())
        }
        Value::DataView(view) => {
            view.set_prototype(value);
            Value::DataView(view.clone())
        }
        Value::Map(data) => {
            data.set_prototype(value);
            Value::Map(data.clone())
        }
        Value::Set(data) => {
            data.set_prototype(value);
            Value::Set(data.clone())
        }
        Value::Promise(data) => {
            data.set_prototype(value);
            Value::Promise(data.clone())
        }
        _ => return None,
    })
}
pub(crate) fn define_property(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let Some(target) = arguments.first() else {
        return Ok(Value::Undefined);
    };
    let key = crate::conversion::to_property_key(arguments.get(1).unwrap_or(&Value::Undefined))?;
    if matches!(target, Value::Proxy(_)) {
        return crate::proxy::proxy_define_property(
            target,
            &key,
            arguments.get(2).unwrap_or(&Value::Undefined),
        );
    }
    let Some(Value::Object(descriptor)) = arguments.get(2) else {
        return Ok(target.clone());
    };
    let result = define_own_property(target, &key, descriptor)?;
    crate::locals::replace_value(target, &result);
    crate::super_scope::attach_home_objects(&result);
    Ok(result)
}
pub(crate) fn define_own_property(
    target: &Value,
    key: &str,
    descriptor: &[(String, Value)],
) -> Result<Value, crate::execute::VmError> {
    validate_descriptor_kind(descriptor)?;
    let key_value = Value::String(key.to_string());
    let current = crate::builtins::object::descriptor(Some(target), Some(&key_value))?;
    if matches!(current, Value::Undefined) && crate::properties::rejects_new_property(target, key) {
        return Err(crate::value::error::throw_type_error(
            "Cannot define a property on a non-extensible object",
        ));
    }
    validate_redefinition(&current, descriptor)?;
    let descriptor = complete_descriptor(descriptor, &current);
    let value = descriptor
        .iter()
        .rev()
        .find(|(name, _)| name == "value")
        .map_or(Value::Undefined, |(_, value)| value.clone());
    let accessor = descriptor
        .iter()
        .any(|(name, _)| matches!(name.as_str(), "get" | "set"));
    let mut result = if accessor {
        define_accessor_placeholder(target.clone(), key)
    } else {
        define_property_value(target.clone(), key, value)
    };
    store_descriptor_metadata(&mut result, key, &descriptor);
    define_array_descriptor(&mut result, key, descriptor);
    Ok(result)
}

fn validate_descriptor_kind(descriptor: &[(String, Value)]) -> Result<(), crate::execute::VmError> {
    let accessor = descriptor
        .iter()
        .any(|(name, _)| matches!(name.as_str(), "get" | "set"));
    let data = descriptor
        .iter()
        .any(|(name, _)| matches!(name.as_str(), "value" | "writable"));
    if accessor && data {
        return Err(crate::value::error::throw_type_error(
            "Invalid property descriptor",
        ));
    }
    Ok(())
}
fn store_descriptor_metadata(result: &mut Value, key: &str, descriptor: &[(String, Value)]) {
    let metadata = Value::Object(Rc::new(ObjectData::new(descriptor.to_vec())));
    let descriptor_key = descriptor_key(key);
    match result {
        Value::Object(properties) => {
            let properties = Rc::make_mut(properties);
            properties.retain(|(name, _)| name != &descriptor_key);
            properties.push((descriptor_key, metadata));
        }
        Value::Function(function) => {
            let mut properties = function.properties.borrow_mut();
            properties.retain(|(name, _)| name != &descriptor_key);
            properties.push((descriptor_key, metadata));
        }
        Value::Promise(promise) => {
            let mut properties = promise.properties.borrow_mut();
            properties.retain(|(name, _)| name != &descriptor_key);
            properties.push((descriptor_key, metadata));
        }
        Value::Builtin(builtin) => write_intrinsic_override(*builtin, key, metadata),
        Value::ArrayBuffer(buffer) => buffer.set_own_property(&descriptor_key, metadata),
        Value::DataView(view) => view.set_own_property(&descriptor_key, metadata),
        Value::BoundFunction(bound) => {
            let mut properties = bound.properties.borrow_mut();
            properties.retain(|(name, _)| name != &descriptor_key);
            properties.push((descriptor_key, metadata));
        }
        _ => {}
    }
}
fn define_accessor_placeholder(target: Value, key: &str) -> Value {
    if matches!(
        target,
        Value::Object(_)
            | Value::Function(_)
            | Value::Builtin(_)
            | Value::Promise(_)
            | Value::BoundFunction(_)
            | Value::ArrayBuffer(_)
    ) {
        return define_property_value(target, key, Value::Undefined);
    }
    target
}

include!("builtins_array.rs");
include!("builtins_descriptor.rs");
include!("builtins_define_properties.rs");

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
