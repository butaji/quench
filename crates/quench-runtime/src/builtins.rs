pub mod object;
pub mod props;

use std::rc::Rc;

use crate::{ops::Builtin, value::Value};

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

pub(crate) fn escape(value: Option<&Value>) -> Value {
    let source = value.map_or_else(|| value_to_string(&Value::Undefined), value_to_string);
    let mut result = String::new();
    for character in source.chars() {
        if character.is_ascii_alphanumeric() || "@*_+-./".contains(character) {
            result.push(character);
        } else {
            let code = character as u32;
            let escaped = if code <= 0xFF {
                format!("%{code:02X}")
            } else {
                format!("%u{code:04X}")
            };
            result.push_str(&escaped);
        }
    }
    Value::String(result)
}

pub(crate) fn unescape(value: Option<&Value>) -> Value {
    let text = value.map_or_else(|| value_to_string(&Value::Undefined), value_to_string);
    let mut result = String::new();
    let mut chars = text.chars().peekable();
    while let Some(character) = chars.next() {
        if character != '%' {
            result.push(character);
            continue;
        }
        let digits: String = chars.by_ref().take(2).collect();
        if let Some(parsed) = u8::from_str_radix(&digits, 16).ok().map(char::from) {
            result.push(parsed);
        } else {
            result.push('%');
            result.push_str(&digits);
        }
    }
    Value::String(result)
}

pub(crate) fn array(arguments: &[Value]) -> Value {
    if let [Value::Number(length)] = arguments {
        if *length >= 0.0 && length.fract() == 0.0 {
            return Value::Array(Rc::new(vec![Value::Undefined; *length as usize]));
        }
    }
    Value::Array(Rc::new(arguments.to_vec()))
}

pub(crate) fn array_map(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let Some(receiver) = receiver else {
        return Ok(Value::Array(Rc::new(Vec::new())));
    };
    let Some(callback) = arguments.first() else {
        return Ok(Value::Array(Rc::new(Vec::new())));
    };
    let length = array_like_length(receiver);
    let mut mapped = Vec::with_capacity(length);
    for index in 0..length {
        let value = array_like_value(receiver, index);
        let args = [value, Value::Number(index as f64), receiver.clone()];
        let this_arg = arguments.get(1).map_or(&Value::Undefined, |value| value);
        mapped.push(crate::functions::execute_target(callback, this_arg, &args)?);
    }
    Ok(Value::Array(Rc::new(mapped)))
}

fn array_like_length(value: &Value) -> usize {
    let length = match value {
        Value::Array(values) => values.len() as f64,
        Value::Object(properties) => properties
            .iter()
            .rev()
            .find(|(key, _)| key == "length")
            .map_or(0.0, |(_, value)| value_to_number(value)),
        _ => 0.0,
    };
    length.max(0.0).min(usize::MAX as f64) as usize
}

fn array_like_value(value: &Value, index: usize) -> Value {
    match value {
        Value::Array(values) => values.get(index).cloned().unwrap_or(Value::Undefined),
        Value::Object(properties) => properties
            .iter()
            .rev()
            .find(|(key, _)| key == &index.to_string())
            .map_or(Value::Undefined, |(_, value)| value.clone()),
        _ => Value::Undefined,
    }
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
        return Ok(Value::Array(Rc::new(Vec::new())));
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
    Ok(Value::Array(Rc::new(filtered)))
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
    let Some(Value::Array(values)) = receiver else {
        return Value::Number(f64::NAN);
    };
    let mut result = values.as_ref().clone();
    result.extend_from_slice(arguments);
    Value::Number(result.len() as f64)
}

pub(crate) fn math_pow(arguments: &[Value]) -> Value {
    let base = arguments.first().map_or(f64::NAN, value_to_number);
    let exponent = arguments.get(1).map_or(f64::NAN, value_to_number);
    Value::Number(base.powf(exponent))
}

pub(crate) fn is_array(value: Option<&Value>) -> Value {
    Value::Boolean(matches!(value, Some(Value::Array(_))))
}

pub(crate) fn delete_property(target: Value, key: &str) -> Value {
    match target {
        Value::Object(properties) => Value::Object(Rc::new(
            properties
                .iter()
                .filter(|(name, _)| name != key)
                .cloned()
                .collect(),
        )),
        Value::Array(values) if key != "length" => Value::Array(values),
        value => value,
    }
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
        _ => Value::Object(Rc::new(Vec::new())),
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
        properties.extend((**existing).clone());
    }
    Value::Object(Rc::new(properties))
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
        _ => left == right,
    }
}

pub(crate) fn keys(value: Option<&Value>) -> Value {
    let keys = match value {
        Some(Value::Object(properties)) => properties
            .iter()
            .map(|(key, _)| Value::String(key.clone()))
            .collect(),
        Some(Value::Array(values)) => (0..values.len())
            .map(|index| Value::String(index.to_string()))
            .collect(),
        Some(Value::String(value)) => (0..value.chars().count())
            .map(|index| Value::String(index.to_string()))
            .collect(),
        _ => Vec::new(),
    };
    Value::Array(Rc::new(keys))
}

pub(crate) fn set_property(target: Value, key: &str, value: Value) -> Value {
    match target {
        Value::Object(mut properties) => {
            {
                let properties = Rc::make_mut(&mut properties);
                if let Some((_, current)) =
                    properties.iter_mut().rev().find(|(name, _)| name == key)
                {
                    *current = value;
                } else {
                    properties.push((key.to_string(), value));
                }
            }
            Value::Object(properties)
        }
        Value::Array(values) => set_array_property(values, key, value),
        Value::Function(function) => set_function_property(function, key, value),
        other => other,
    }
}

pub(crate) fn define_property(arguments: &[Value]) -> Value {
    let Some(target) = arguments.first() else {
        return Value::Undefined;
    };
    let key = arguments.get(1).map_or_else(String::new, value_to_string);
    let value = match arguments.get(2) {
        Some(Value::Object(properties)) => properties
            .iter()
            .rev()
            .find(|(name, _)| name == "value")
            .map_or(Value::Undefined, |(_, value)| value.clone()),
        _ => Value::Undefined,
    };
    set_property(target.clone(), &key, value)
}

fn set_array_property(values: Rc<Vec<Value>>, key: &str, value: Value) -> Value {
    let Ok(index) = key.parse::<usize>() else {
        return Value::Array(values);
    };
    let mut values = values;
    {
        let values = Rc::make_mut(&mut values);
        values.resize(index.saturating_add(1), Value::Undefined);
        values[index] = value;
    }
    Value::Array(values)
}

fn set_function_property(
    function: Rc<crate::value::FunctionValue>,
    key: &str,
    value: Value,
) -> Value {
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
        Some(Value::Function(_)) => "Function",
        Some(Value::BoundFunction(_)) => "Function",
        Some(Value::Builtin(_)) => "Function",
        Some(Value::Proxy(_)) => "Object",
        Some(Value::Promise(_)) => "Promise",
        Some(Value::Map(_)) => "Map",
        Some(Value::Set(_)) => "Set",
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

/// Implement `Function.prototype.toString`.
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

/// Implement `Function.prototype.valueOf`.
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
