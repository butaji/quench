use std::rc::Rc;

use crate::{ops::Builtin, value::Value};

pub(crate) fn property(builtin: Builtin, key: &str) -> Value {
    if builtin == Builtin::Array && key == "prototype" {
        return Value::Builtin(Builtin::ArrayPrototype);
    }
    if builtin == Builtin::ArrayPrototype && key == "map" {
        return Value::Builtin(Builtin::ArrayMap);
    }
    if builtin == Builtin::ArrayMap && key == "call" {
        return Value::Builtin(Builtin::FunctionCall);
    }
    if matches!(builtin, Builtin::Array) && key == "isArray" {
        return Value::Builtin(Builtin::ArrayIsArray);
    }
    if matches!(builtin, Builtin::Object) && key == "is" {
        return Value::Builtin(Builtin::ObjectIs);
    }
    if matches!(builtin, Builtin::Object) && key == "keys" {
        return Value::Builtin(Builtin::ObjectKeys);
    }
    if matches!(builtin, Builtin::Object) && key == "getOwnPropertyDescriptor" {
        return Value::Builtin(Builtin::ObjectGetOwnPropertyDescriptor);
    }
    if builtin == Builtin::Math && key == "pow" {
        return Value::Builtin(Builtin::MathPow);
    }
    Value::Undefined
}

pub(crate) fn object_method(value: &Value, key: &str) -> Value {
    if matches!(value, Value::Object(_) | Value::Array(_)) && key == "hasOwnProperty" {
        return Value::Builtin(Builtin::ObjectHasOwnProperty);
    }
    Value::Undefined
}

pub(crate) fn has_own_property(receiver: Option<&Value>, key: Option<&Value>) -> Value {
    let Some(key) = key.map(value_to_string) else {
        return Value::Boolean(false);
    };
    let present = match receiver {
        Some(Value::Object(properties)) => properties.iter().any(|(name, _)| name == &key),
        Some(Value::Array(values)) => {
            key == "length" || key.parse::<usize>().is_ok_and(|i| i < values.len())
        }
        Some(Value::String(value)) => {
            key == "length"
                || key
                    .parse::<usize>()
                    .is_ok_and(|i| i < value.chars().count())
        }
        _ => false,
    };
    Value::Boolean(present)
}

pub(crate) fn object_special(
    builtin: Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Value {
    match builtin {
        Builtin::ObjectHasOwnProperty => has_own_property(receiver, arguments.first()),
        Builtin::ObjectGetOwnPropertyDescriptor => descriptor(arguments.first(), arguments.get(1)),
        _ => Value::Undefined,
    }
}

pub(crate) fn descriptor(value: Option<&Value>, key: Option<&Value>) -> Value {
    let Some(value) = value else {
        return Value::Undefined;
    };
    let Some(key) = key.map(value_to_string) else {
        return Value::Undefined;
    };
    let property = match value {
        Value::Object(properties) => properties
            .iter()
            .rev()
            .find(|(name, _)| name == &key)
            .map(|(_, value)| value.clone()),
        Value::Array(values) => array_descriptor(values, &key),
        Value::String(value) => string_descriptor(value, &key),
        _ => None,
    };
    property.map_or(Value::Undefined, |property| descriptor_object(&property))
}

fn array_descriptor(values: &[Value], key: &str) -> Option<Value> {
    if key == "length" {
        return None;
    }
    key.parse::<usize>()
        .ok()
        .and_then(|index| values.get(index).cloned())
}

fn string_descriptor(value: &str, key: &str) -> Option<Value> {
    let index = key.parse::<usize>().ok()?;
    let character = value.chars().nth(index)?;
    Some(Value::String(character.to_string()))
}

fn descriptor_object(value: &Value) -> Value {
    Value::Object(Rc::new(vec![
        ("value".to_string(), value.clone()),
        ("writable".to_string(), Value::Boolean(true)),
        ("enumerable".to_string(), Value::Boolean(true)),
        ("configurable".to_string(), Value::Boolean(true)),
    ]))
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        Value::Boolean(value) => value.to_string(),
        Value::Null => "null".to_string(),
        Value::Undefined => "undefined".to_string(),
        _ => "[object Object]".to_string(),
    }
}

pub(crate) fn escape(value: Option<&Value>) -> Value {
    let mut result = String::new();
    for character in value
        .map_or_else(|| value_to_string(&Value::Undefined), value_to_string)
        .chars()
    {
        if character.is_ascii_alphanumeric() || "@*_+-./".contains(character) {
            result.push(character);
        } else if (character as u32) <= 0xFF {
            result.push_str(&format!("%{:02X}", character as u32));
        } else {
            result.push_str(&format!("%u{:04X}", character as u32));
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
        let parsed = u8::from_str_radix(&digits, 16).ok().map(char::from);
        if let Some(parsed) = parsed {
            result.push(parsed);
        } else {
            result.push('%');
            result.push_str(&digits);
        }
    }
    Value::String(result)
}

pub(crate) fn array(arguments: &[Value]) -> Value {
    if arguments.len() == 1 {
        if let Value::Number(length) = arguments[0] {
            if length >= 0.0 && length.fract() == 0.0 {
                return Value::Array(Rc::new(vec![Value::Undefined; length as usize]));
            }
        }
    }
    Value::Array(Rc::new(arguments.to_vec()))
}

pub(crate) fn array_map(arguments: &[Value]) -> Value {
    function_call(arguments)
}

pub(crate) fn function_call(arguments: &[Value]) -> Value {
    let Some(Value::Array(values)) = arguments.first() else {
        return Value::Array(Rc::new(Vec::new()));
    };
    let mapped = match arguments.get(1) {
        Some(Value::Builtin(Builtin::String)) => values
            .iter()
            .map(|value| Value::String(value_to_string(value)))
            .collect(),
        _ => values.iter().cloned().collect(),
    };
    Value::Array(Rc::new(mapped))
}

pub(crate) fn object(arguments: &[Value]) -> Value {
    match arguments.first() {
        Some(Value::Array(_))
        | Some(Value::Object(_))
        | Some(Value::Function(_))
        | Some(Value::Builtin(_)) => arguments[0].clone(),
        _ => Value::Object(Rc::new(Vec::new())),
    }
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
        Value::Object(properties) => {
            let mut properties = (*properties).clone();
            if let Some((_, current)) = properties.iter_mut().rev().find(|(name, _)| name == key) {
                *current = value;
            } else {
                properties.push((key.to_string(), value));
            }
            Value::Object(Rc::new(properties))
        }
        Value::Array(values) => set_array_property(values, key, value),
        Value::Function(function) => set_function_property(function, key, value),
        other => other,
    }
}

fn set_array_property(values: Rc<Vec<Value>>, key: &str, value: Value) -> Value {
    let Ok(index) = key.parse::<usize>() else {
        return Value::Array(values);
    };
    let mut values = (*values).clone();
    values.resize(index.saturating_add(1), Value::Undefined);
    values[index] = value;
    Value::Array(Rc::new(values))
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
