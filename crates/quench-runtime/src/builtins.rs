use std::rc::Rc;

use crate::{ops::Builtin, value::Value};

pub(crate) fn property(builtin: Builtin, key: &str) -> Value {
    if matches!(builtin, Builtin::Array) && key == "isArray" {
        return Value::Builtin(Builtin::ArrayIsArray);
    }
    if matches!(builtin, Builtin::Object) && key == "is" {
        return Value::Builtin(Builtin::ObjectIs);
    }
    if matches!(builtin, Builtin::Object) && key == "keys" {
        return Value::Builtin(Builtin::ObjectKeys);
    }
    Value::Undefined
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
