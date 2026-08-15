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
use std::rc::Rc;
const DESCRIPTOR_PREFIX: &str = "\0quench:descriptor:\0";
const DELETED_PREFIX: &str = "\0quench:deleted:\0";
pub(crate) const ERROR_SLOT: &str = "\0error_slot";

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
    crate::json::method_property(builtin, key)
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

pub(crate) fn array(arguments: &[Value]) -> Value {
    if let [Value::Number(length)] = arguments {
        if *length >= 0.0 && length.fract() == 0.0 && *length <= u32::MAX as f64 {
            let mut values = Value::array(Vec::new());
            if let Value::Array(values) = &mut values {
                Rc::make_mut(values).set_length(*length as usize);
            }
            return values;
        }
    }
    Value::array(arguments.to_vec())
}

pub(crate) fn array_map(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let Some(receiver) = receiver else {
        return Ok(Value::array(Vec::new()));
    };
    let Some(callback) = arguments.first() else {
        return Ok(Value::array(Vec::new()));
    };
    let length = map_length(receiver)?;
    if length > u32::MAX as usize {
        return Err(crate::value::error::throw_range_error(
            "Invalid array length",
        ));
    }
    let mut mapped = Value::array(Vec::new());
    if let Value::Array(values) = &mut mapped {
        Rc::make_mut(values).set_length(length);
    }
    for index in 0..length {
        let Some(value) = map_value(receiver, index)? else {
            continue;
        };
        let args = [value, Value::Number(index as f64), receiver.clone()];
        let this_arg = arguments.get(1).map_or(&Value::Undefined, |value| value);
        let result = crate::functions::execute_target(callback, this_arg, &args)?;
        if let Value::Array(values) = &mut mapped {
            Rc::make_mut(values).set_index(index, result);
        }
    }
    Ok(mapped)
}
fn map_length(receiver: &Value) -> Result<usize, crate::execute::VmError> {
    if let Value::Array(values) = receiver {
        return Ok(values.logical_len());
    }
    let length = crate::execute::get_property_result(receiver, "length")?;
    let number = crate::conversion::to_number(&length)?;
    if number.is_nan() || number <= 0.0 {
        return Ok(0);
    }
    Ok(number.floor().min(9_007_199_254_740_991.0) as usize)
}
fn map_value(receiver: &Value, index: usize) -> Result<Option<Value>, crate::execute::VmError> {
    if let Value::Array(values) = receiver {
        return Ok(values
            .has_index(index)
            .then(|| values.get_index(index))
            .flatten());
    }
    let key = index.to_string();
    if !crate::with_scope::has_property(receiver, &key)? {
        return Ok(None);
    }
    crate::execute::get_property_result(receiver, &key).map(Some)
}
pub(crate) fn array_for_each(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let Some(Value::Array(values)) = receiver else {
        return Ok(Value::Undefined);
    };
    if let Some(callback) = arguments.first() {
        let this_arg = arguments.get(1).map_or(&Value::Undefined, |value| value);
        for (index, value) in values.iter().enumerate() {
            crate::functions::execute_target(
                callback,
                this_arg,
                &[value.clone(), Value::Number(index as f64), Value::Undefined],
            )?;
        }
    }
    Ok(Value::Undefined)
}
pub(crate) fn array_filter(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let Some(Value::Array(values)) = receiver else {
        return Ok(Value::array(Vec::new()));
    };
    let Some(callback) = arguments.first() else {
        return Ok(Value::Array(values.clone()));
    };
    let mut filtered = Vec::new();
    let this_arg = arguments.get(1).map_or(&Value::Undefined, |value| value);
    for (index, value) in values.iter().enumerate() {
        let args = [value.clone(), Value::Number(index as f64), Value::Undefined];
        let result = crate::functions::execute_target(callback, this_arg, &args)?;
        if crate::execute::is_truthy(&result) {
            filtered.push(value.clone());
        }
    }
    Ok(Value::array(filtered))
}

pub(crate) fn array_join(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Some(Value::Array(values)) = receiver else {
        return Value::String(String::new());
    };
    let separator = arguments
        .first()
        .map_or_else(|| ",".to_string(), value_to_string);
    Value::String(
        values
            .iter()
            .map(value_to_string)
            .collect::<Vec<_>>()
            .join(&separator),
    )
}

pub(crate) fn array_push(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Some(receiver @ Value::Array(values)) = receiver else {
        return Value::Number(f64::NAN);
    };
    let mut result = values.to_vec();
    result.extend_from_slice(arguments);
    let length = result.len();
    crate::locals::replace_value(receiver, &Value::array(result));
    Value::Number(length as f64)
}
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

fn pow(base: f64, exponent: f64) -> f64 {
    if exponent == 0.0 {
        return 1.0;
    }
    if exponent.is_nan() || base.is_nan() || base.abs() == 1.0 && exponent.is_infinite() {
        return f64::NAN;
    }
    if base.is_infinite() {
        return infinite_pow(base, exponent);
    }
    if base == 0.0 {
        return zero_pow(base, exponent);
    }
    base.powf(exponent)
}

fn infinite_pow(base: f64, exponent: f64) -> f64 {
    if exponent.is_sign_positive() {
        if base.is_sign_negative() && is_odd_integer(exponent) {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        }
    } else if base.is_sign_negative() && is_odd_integer(exponent) {
        -0.0
    } else {
        0.0
    }
}

fn zero_pow(base: f64, exponent: f64) -> f64 {
    if exponent.is_sign_positive() {
        if base.is_sign_negative() && is_odd_integer(exponent) {
            -0.0
        } else {
            0.0
        }
    } else if base.is_sign_negative() && is_odd_integer(exponent) {
        f64::NEG_INFINITY
    } else {
        f64::INFINITY
    }
}

fn is_odd_integer(value: f64) -> bool {
    value.is_finite()
        && value == value.trunc()
        && value.abs() < 9_007_199_254_740_992.0
        && value as i64 % 2 != 0
}

pub(crate) fn is_array(value: Option<&Value>) -> Value {
    Value::Boolean(matches!(value, Some(Value::Array(_))))
}

fn value_to_number(value: &Value) -> f64 {
    match value {
        Value::Number(value) => *value,
        Value::String(value) => value.parse().unwrap_or(f64::NAN),
        _ => f64::NAN,
    }
}

pub(crate) fn object(arguments: &[Value]) -> Value {
    match arguments.first() {
        Some(Value::Array(_))
        | Some(Value::ArrayBuffer(_))
        | Some(Value::DataView(_))
        | Some(Value::Float32Array(_))
        | Some(Value::Float64Array(_))
        | Some(Value::Int16Array(_))
        | Some(Value::Int8Array(_))
        | Some(Value::Int32Array(_))
        | Some(Value::Uint16Array(_))
        | Some(Value::Uint8Array(_))
        | Some(Value::Uint8ClampedArray(_))
        | Some(Value::Uint32Array(_))
        | Some(Value::Object(_))
        | Some(Value::Function(_))
        | Some(Value::Builtin(_)) => arguments[0].clone(),
        Some(
            value @ (Value::String(_) | Value::Number(_) | Value::Boolean(_) | Value::BigInt(_)),
        ) => boxed_object(value),
        _ => Value::Object(Rc::new(ObjectData::new(vec![(
            "constructor".to_string(),
            Value::Builtin(Builtin::Object),
        )]))),
    }
}

fn boxed_object(value: &Value) -> Value {
    let constructor = object::boxed_constructor(value);
    let mut properties = vec![
        ("_value".to_string(), value.clone()),
        ("constructor".to_string(), Value::Builtin(constructor)),
    ];
    if let Some(prototype) = crate::builtin_meta::instance_prototype(constructor) {
        properties.push(("\0prototype".to_string(), Value::Builtin(prototype)));
    }
    Value::Object(Rc::new(ObjectData::new(properties)))
}

pub(crate) fn error(builtin: Builtin, arguments: &[Value]) -> Value {
    let (name, constructor, prototype) = match builtin {
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
    };
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

fn non_enumerable_descriptor(value: &Value) -> Value {
    Value::Object(Rc::new(ObjectData::new(vec![
        ("value".to_string(), value.clone()),
        ("writable".to_string(), Value::Boolean(true)),
        ("enumerable".to_string(), Value::Boolean(false)),
        ("configurable".to_string(), Value::Boolean(true)),
    ])))
}
pub(crate) fn same_value(left: Option<&Value>, right: Option<&Value>) -> bool {
    let (Some(left), Some(right)) = (left, right) else {
        return matches!((left, right), (None, None));
    };
    if let (Value::Number(left), Value::Number(right)) = (left, right) {
        return (left.is_nan() && right.is_nan())
            || (left == right && left.is_sign_negative() == right.is_sign_negative());
    }
    match (left, right) {
        (Value::Array(left), Value::Array(right)) => Rc::ptr_eq(left, right),
        (Value::Object(left), Value::Object(right)) => Rc::ptr_eq(left, right),
        (Value::ObjectAlias(left), Value::Object(right))
        | (Value::Object(right), Value::ObjectAlias(left)) => left
            .0
            .borrow()
            .upgrade()
            .is_some_and(|left| Rc::ptr_eq(&left, right)),
        (Value::ArrayBuffer(left), Value::ArrayBuffer(right)) => Rc::ptr_eq(left, right),
        (Value::DataView(left), Value::DataView(right)) => Rc::ptr_eq(left, right),
        (Value::Float32Array(left), Value::Float32Array(right)) => Rc::ptr_eq(left, right),
        (Value::Float64Array(left), Value::Float64Array(right)) => Rc::ptr_eq(left, right),
        (Value::Int16Array(left), Value::Int16Array(right)) => Rc::ptr_eq(left, right),
        (Value::Int8Array(left), Value::Int8Array(right)) => Rc::ptr_eq(left, right),
        (Value::Int32Array(left), Value::Int32Array(right)) => Rc::ptr_eq(left, right),
        (Value::Uint16Array(left), Value::Uint16Array(right)) => Rc::ptr_eq(left, right),
        (Value::Uint8Array(left), Value::Uint8Array(right)) => Rc::ptr_eq(left, right),
        (Value::Uint8ClampedArray(left), Value::Uint8ClampedArray(right)) => {
            Rc::ptr_eq(left, right)
        }
        (Value::Uint32Array(left), Value::Uint32Array(right)) => Rc::ptr_eq(left, right),
        (Value::Function(left), Value::Function(right)) => Rc::ptr_eq(left, right),
        (Value::Generator(left), Value::Generator(right)) => Rc::ptr_eq(left, right),
        (Value::BoundFunction(left), Value::BoundFunction(right)) => Rc::ptr_eq(left, right),
        (Value::StringUnits(_), Value::String(_)) | (Value::String(_), Value::StringUnits(_)) => {
            crate::strings::units_equal(left, right)
        }
        _ => left == right,
    }
}
pub(crate) fn same_value_zero(left: &Value, right: &Value) -> bool {
    if let (Value::Number(left), Value::Number(right)) = (left, right) {
        return left.is_nan() && right.is_nan() || left == right;
    }
    same_value(Some(left), Some(right))
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

fn boxed_string_immutable_key(properties: &ObjectData, key: &str) -> bool {
    let is_string = properties
        .iter()
        .any(|(name, value)| name == "_value" && matches!(value, Value::String(_)));
    is_string && (key == "length" || key.parse::<usize>().is_ok())
}

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
