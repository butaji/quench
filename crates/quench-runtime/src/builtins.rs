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

pub(crate) fn aggregate_error_prototype() -> Value {
    if let Some(value) = read_descriptor_value(Builtin::AggregateError, "prototype") {
        return value;
    }
    let constructor = crate::vm::current_realm_intrinsic(Builtin::AggregateError)
        .unwrap_or(Value::Builtin(Builtin::AggregateError));
    let value = Value::Object(Rc::new(ObjectData::new(vec![
        ("constructor".to_string(), constructor.clone()),
        (
            descriptor_key("constructor"),
            Value::Object(Rc::new(ObjectData::new(vec![
                ("value".to_string(), constructor),
                ("writable".to_string(), Value::Boolean(true)),
                ("enumerable".to_string(), Value::Boolean(false)),
                ("configurable".to_string(), Value::Boolean(true)),
            ]))),
        ),
        (
            "name".to_string(),
            Value::String("AggregateError".to_string()),
        ),
        (
            descriptor_key("name"),
            Value::Object(Rc::new(ObjectData::new(vec![
                (
                    "value".to_string(),
                    Value::String("AggregateError".to_string()),
                ),
                ("writable".to_string(), Value::Boolean(true)),
                ("enumerable".to_string(), Value::Boolean(false)),
                ("configurable".to_string(), Value::Boolean(true)),
            ]))),
        ),
        (
            "\0prototype".to_string(),
            Value::Builtin(Builtin::ErrorPrototype),
        ),
        ("message".to_string(), Value::String(String::new())),
        (
            descriptor_key("message"),
            Value::Object(Rc::new(ObjectData::new(vec![
                ("value".to_string(), Value::String(String::new())),
                ("writable".to_string(), Value::Boolean(true)),
                ("enumerable".to_string(), Value::Boolean(false)),
                ("configurable".to_string(), Value::Boolean(true)),
            ]))),
        ),
    ])));
    let descriptor = Value::Object(Rc::new(ObjectData::new(vec![
        ("value".to_string(), value.clone()),
        ("writable".to_string(), Value::Boolean(false)),
        ("enumerable".to_string(), Value::Boolean(false)),
        ("configurable".to_string(), Value::Boolean(false)),
    ])));
    write_intrinsic_override(Builtin::AggregateError, "prototype", descriptor);
    value
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
include!("builtins_tail.rs");

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
        for (index, value) in values.iter().enumerate() {
            crate::functions::execute_target(
                callback,
                &Value::Undefined,
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
    for (index, value) in values.iter().enumerate() {
        let args = [value.clone(), Value::Number(index as f64), Value::Undefined];
        let result = crate::functions::execute_target(callback, &Value::Undefined, &args)?;
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
        (
            descriptor_key("_value"),
            Value::Object(Rc::new(ObjectData::new(vec![
                ("value".to_string(), value.clone()),
                ("writable".to_string(), Value::Boolean(true)),
                ("enumerable".to_string(), Value::Boolean(false)),
                ("configurable".to_string(), Value::Boolean(true)),
            ]))),
        ),
    ];
    if let Some(prototype) = crate::builtin_meta::instance_prototype(constructor) {
        properties.push(("\0prototype".to_string(), Value::Builtin(prototype)));
    }
    Value::Object(Rc::new(ObjectData::new(properties)))
}

pub(crate) fn error(builtin: Builtin, arguments: &[Value]) -> Value {
    let (name, constructor, _prototype) = match builtin {
        Builtin::RangeError => (
            "RangeError",
            Builtin::RangeError,
            Builtin::RangeErrorPrototype,
        ),
        Builtin::ReferenceError => (
            "ReferenceError",
            Builtin::ReferenceError,
            Builtin::ReferenceErrorPrototype,
        ),
        Builtin::SyntaxError => (
            "SyntaxError",
            Builtin::SyntaxError,
            Builtin::SyntaxErrorPrototype,
        ),
        Builtin::EvalError => ("EvalError", Builtin::EvalError, Builtin::EvalErrorPrototype),
        Builtin::URIError => ("URIError", Builtin::URIError, Builtin::URIErrorPrototype),
        Builtin::AggregateError => (
            "AggregateError",
            Builtin::AggregateError,
            Builtin::AggregateErrorPrototype,
        ),
        Builtin::TypeError => ("TypeError", Builtin::TypeError, Builtin::TypeErrorPrototype),
        Builtin::SuppressedError => (
            "SuppressedError",
            Builtin::SuppressedError,
            Builtin::ErrorPrototype,
        ),
        Builtin::Error => ("Error", Builtin::Error, Builtin::ErrorPrototype),
        _ => ("Error", Builtin::Error, Builtin::ErrorPrototype),
    };
    let message = arguments.first().map_or_else(String::new, value_to_string);
    let intrinsic = crate::vm::intrinsic_for_realm(
        crate::vm::current_context_or_default().realm(),
        constructor,
    );
    let prototype = crate::execute::get_property(&intrinsic, "prototype");
    let mut properties = vec![
        ("name".to_string(), Value::String(name.to_string())),
        ("message".to_string(), Value::String(message)),
        ("constructor".to_string(), intrinsic),
        (ERROR_SLOT.to_string(), Value::Boolean(true)),
        ("\0prototype".to_string(), prototype),
    ];
    if let Some(message) = arguments
        .first()
        .filter(|value| !matches!(value, Value::Undefined))
        .map(value_to_string)
    {
        properties.push(("message".to_string(), Value::String(message)));
    }
    if let Some(Value::Object(existing)) = arguments.first() {
        properties.extend(existing.properties.clone());
    }
    Value::Object(Rc::new(ObjectData::new(properties)))
}

pub(crate) fn type_error_in_realm(realm: crate::ops::RealmId, message: &str) -> Value {
    let arguments = [Value::String(message.to_string())];
    crate::vm::with_error_realm(realm, || {
        crate::vm::with_realm(realm, || error(Builtin::TypeError, &arguments))
    })
    .unwrap_or_else(|| error(Builtin::TypeError, &arguments))
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
        (Value::Builtin(left), Value::String(right))
        | (Value::String(right), Value::Builtin(left)) => {
            crate::intl::tolocale::symbol::name(*left)
                .is_some_and(|name| right == name || right == &format!("{name}\0"))
        }
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
