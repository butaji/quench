use std::rc::Rc;

use crate::{ops::Builtin, value::Value};

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
        Some(Value::Builtin(builtin)) => {
            super::callable_property(*builtin, &key).is_some()
                || super::special_property(*builtin, &key).is_some()
        }
        _ => false,
    };
    Value::Boolean(present)
}

pub(crate) fn object_property_is_enumerable(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Value {
    if matches!(receiver, Some(Value::Builtin(_))) {
        return Value::Boolean(false);
    }
    has_own_property(receiver, arguments.first())
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
    let (Some(value), Some(key)) = (value, key.map(value_to_string)) else {
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
        Value::Builtin(builtin) if matches!(key.as_str(), "length" | "name") => {
            return super::callable_property(*builtin, &key).unwrap_or(Value::Undefined);
        }
        Value::Function(function) if key == "length" => {
            return Value::Object(Rc::new(vec![
                (
                    "value".to_string(),
                    Value::Number(f64::from(function.params)),
                ),
                ("writable".to_string(), Value::Boolean(false)),
                ("enumerable".to_string(), Value::Boolean(false)),
                ("configurable".to_string(), Value::Boolean(true)),
            ]));
        }
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
    value
        .chars()
        .nth(key.parse::<usize>().ok()?)
        .map(|character| Value::String(character.to_string()))
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
