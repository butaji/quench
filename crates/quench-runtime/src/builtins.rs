mod builtins_cells;
pub mod object;
pub mod object_alias;
pub mod props;

mod array_like;
mod intrinsic_overrides;
use array_like::{array_like_length, array_like_value};
use intrinsic_overrides as overrides;

use std::rc::Rc;

use crate::{
    ops::Builtin,
    value::{ObjectData, Value},
};

const DESCRIPTOR_PREFIX: &str = "\0quench:descriptor:\0";

pub(crate) fn descriptor_key(key: &str) -> String {
    format!("{DESCRIPTOR_PREFIX}{key}")
}

pub(crate) fn is_descriptor_key(key: &str) -> bool {
    key.starts_with(DESCRIPTOR_PREFIX)
}

pub(crate) fn read_intrinsic_override(builtin: Builtin, key: &str) -> Option<Value> {
    overrides::read(builtin, key)
}

pub(crate) fn write_intrinsic_override(builtin: Builtin, key: &str, descriptor: Value) {
    overrides::write(builtin, key, descriptor)
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
    props::lookup(builtin, key)
}

pub(crate) fn special_property(builtin: Builtin, key: &str) -> Option<Value> {
    props::special_property(builtin, key)
}

pub(crate) fn callable_property(builtin: Builtin, key: &str) -> Option<Value> {
    props::callable(builtin, key)
}

pub(crate) fn builtin_name(builtin: Builtin) -> &'static str {
    props::builtin_name(builtin)
}

include!("builtins_escape.rs");

pub(crate) fn array(arguments: &[Value]) -> Value {
    if let [Value::Number(length)] = arguments {
        if *length >= 0.0 && length.fract() == 0.0 {
            return Value::array(vec![Value::Undefined; *length as usize]);
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
    let length = array_like_length(receiver);
    let mut mapped = Vec::with_capacity(length);
    for index in 0..length {
        let value = array_like_value(receiver, index);
        let args = [value, Value::Number(index as f64), receiver.clone()];
        let this_arg = arguments.get(1).map_or(&Value::Undefined, |value| value);
        mapped.push(crate::functions::execute_target(callback, this_arg, &args)?);
    }
    Ok(Value::array(mapped))
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

pub(crate) fn math_pow(arguments: &[Value]) -> Value {
    let base = arguments.first().map_or(f64::NAN, value_to_number);
    let exponent = arguments.get(1).map_or(f64::NAN, value_to_number);
    Value::Number(base.powf(exponent))
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
        ) => Value::Object(Rc::new(ObjectData::new(vec![
            ("_value".to_string(), value.clone()),
            (
                "constructor".to_string(),
                Value::Builtin(object::boxed_constructor(value)),
            ),
        ]))),
        _ => Value::Object(Rc::new(ObjectData::new(vec![(
            "constructor".to_string(),
            Value::Builtin(Builtin::Object),
        )]))),
    }
}

/// Construct an error object for the given error constructor.
///
/// The resulting object carries `name`, `message`, and `constructor` so that
/// `err.constructor` and `err.constructor.name` behave like the specification.
pub(crate) fn error(builtin: Builtin, arguments: &[Value]) -> Value {
    let (name, constructor) = match builtin {
        Builtin::RangeError => ("RangeError", Builtin::RangeError),
        Builtin::ReferenceError => ("ReferenceError", Builtin::ReferenceError),
        Builtin::SyntaxError => ("SyntaxError", Builtin::SyntaxError),
        Builtin::EvalError => ("EvalError", Builtin::EvalError),
        Builtin::URIError => ("URIError", Builtin::URIError),
        Builtin::AggregateError => ("AggregateError", Builtin::AggregateError),
        Builtin::TypeError => ("TypeError", Builtin::TypeError),
        _ => ("Error", Builtin::Error),
    };
    let message = arguments.first().map_or_else(
        || Value::String(String::new()),
        |value| Value::String(value_to_string(value)),
    );
    let mut properties = vec![
        ("name".to_string(), Value::String(name.to_string())),
        ("message".to_string(), message),
        ("constructor".to_string(), Value::Builtin(constructor)),
    ];
    if let Some(Value::Object(existing)) = arguments.first() {
        properties.extend(existing.properties.clone());
    }
    Value::Object(Rc::new(ObjectData::new(properties)))
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
        _ => left == right,
    }
}

pub(crate) fn same_value_zero(left: &Value, right: &Value) -> bool {
    if let (Value::Number(left), Value::Number(right)) = (left, right) {
        return left.is_nan() && right.is_nan() || left == right;
    }
    same_value(Some(left), Some(right))
}

pub(crate) fn keys(value: Option<&Value>) -> Value {
    crate::own_keys::enumerable_names(value)
}

pub(crate) fn set_property(target: Value, key: &str, value: Value) -> Value {
    if let Some(result) = crate::typed_array_ops::set_property(&target, key, &value) {
        return result.unwrap_or(target);
    }
    match target {
        Value::Object(properties)
            if descriptor_flag_in(&properties, key, "writable") == Some(false) =>
        {
            Value::Object(properties)
        }
        Value::Object(properties) => builtins_cells::set_object_property(properties, key, value),
        Value::Array(values) if array_descriptor_flag(&values, key, "writable") == Some(false) => {
            Value::Array(values)
        }
        Value::Array(values) => set_array_property(values, key, value),
        Value::Function(function) => set_function_property(function, key, value),
        other => other,
    }
}

pub(crate) fn define_property(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let Some(target) = arguments.first() else {
        return Ok(Value::Undefined);
    };
    let key = crate::conversion::to_property_key(arguments.get(1).unwrap_or(&Value::Undefined))?;
    let Some(Value::Object(descriptor)) = arguments.get(2) else {
        return Ok(target.clone());
    };
    define_own_property(target, &key, descriptor)
}

pub(crate) fn define_own_property(
    target: &Value,
    key: &str,
    descriptor: &[(String, Value)],
) -> Result<Value, crate::execute::VmError> {
    let key_value = Value::String(key.to_string());
    let current = crate::builtins::object::descriptor(Some(target), Some(&key_value))?;
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
        Value::Builtin(builtin) => write_intrinsic_override(*builtin, key, metadata),
        _ => {}
    }
}

fn define_accessor_placeholder(target: Value, key: &str) -> Value {
    if matches!(
        target,
        Value::Object(_) | Value::Function(_) | Value::Builtin(_)
    ) {
        return set_property(target, key, Value::Undefined);
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

/// Implement `Object.prototype.toString` using the receiver's [[Class]].
pub(crate) fn prototype_to_string(receiver: Option<&Value>) -> Value {
    let tag = match receiver {
        None | Some(Value::Undefined) => "Undefined",
        Some(Value::Null) => "Null",
        Some(Value::Boolean(_)) => "Boolean",
        Some(Value::Number(_)) => "Number",
        Some(Value::String(s)) if s.starts_with("Symbol(") => "Symbol",
        Some(Value::String(_)) => "String",
        Some(Value::BigInt(_)) => "BigInt",
        Some(Value::Array(_)) => "Array",
        Some(Value::Object(_)) => "Object",
        Some(Value::ArrayBuffer(_)) => "ArrayBuffer",
        Some(Value::DataView(_)) => "DataView",
        Some(Value::Float32Array(_)) => "Float32Array",
        Some(Value::Float64Array(_)) => "Float64Array",
        Some(Value::Int16Array(_)) => "Int16Array",
        Some(Value::Int8Array(_)) => "Int8Array",
        Some(Value::Int32Array(_)) => "Int32Array",
        Some(Value::Uint16Array(_)) => "Uint16Array",
        Some(Value::Uint8Array(_)) => "Uint8Array",
        Some(Value::Uint8ClampedArray(_)) => "Uint8ClampedArray",
        Some(Value::Uint32Array(_)) => "Uint32Array",
        Some(Value::BigInt64Array(_)) => "BigInt64Array",
        Some(Value::BigUint64Array(_)) => "BigUint64Array",
        Some(Value::Function(_)) => "Function",
        Some(Value::BoundFunction(_)) => "Function",
        Some(Value::Builtin(_)) => "Function",
        Some(Value::Proxy(_)) => "Object",
        Some(Value::Promise(_)) => "Promise",
        Some(Value::Map(_)) => "Map",
        Some(Value::Set(_)) => "Set",
        Some(Value::Generator(_)) => "Generator",
        Some(Value::BindingCell(cell)) => return prototype_to_string(Some(&cell.borrow())),
        Some(Value::HostCapability(_) | Value::Iterator(_) | Value::ObjectAlias(_)) => "Object",
    };
    Value::String(format!("[object {tag}]"))
}

/// Implement `Object.prototype.valueOf`.
pub(crate) fn prototype_value_of(receiver: Option<&Value>) -> Value {
    match receiver {
        None | Some(Value::Undefined) | Some(Value::Null) => Value::Null,
        Some(v) => v.clone(),
    }
}

pub(crate) fn function_prototype_to_string(receiver: Option<&Value>) -> Value {
    match receiver {
        Some(Value::Builtin(b)) => Value::String(format!(
            "function {}() {{ [native code] }}",
            builtin_name(*b)
        )),
        Some(Value::Function(_)) | Some(Value::BoundFunction(_)) => {
            Value::String("function () {{ [native code] }}".to_string())
        }
        _ => Value::String("".to_string()),
    }
}

pub(crate) fn function_prototype_value_of(receiver: Option<&Value>) -> Value {
    prototype_value_of(receiver)
}
fn value_to_string(value: &Value) -> String {
    use Value::*;
    match value {
        String(value) => value.clone(),
        Number(value) => value.to_string(),
        Boolean(value) => value.to_string(),
        Null => "null".to_string(),
        Undefined => "undefined".to_string(),
        _ => "[object Object]".to_string(),
    }
}
