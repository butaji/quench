use std::rc::Rc;

use crate::{execute::VmError, ops::Builtin, value::Value};

pub(crate) fn object_method(value: &Value, key: &str) -> Value {
    if matches!(value, Value::Object(_) | Value::Array(_)) {
        return match key {
            "hasOwnProperty" => Value::Builtin(Builtin::ObjectHasOwnProperty),
            "propertyIsEnumerable" => Value::Builtin(Builtin::ObjectPropertyIsEnumerable),
            _ => Value::Undefined,
        };
    }
    Value::Undefined
}

pub(crate) fn has_own_property(receiver: Option<&Value>, key: Option<&Value>) -> Value {
    has_own_property_result(receiver, key).unwrap_or(Value::Boolean(false))
}

pub(crate) fn execute_special(
    builtin: Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let (target, key) = target_and_key(receiver, arguments);
    match builtin {
        Builtin::ObjectHasOwnProperty => has_own_property_result(target, key),
        Builtin::ObjectGetOwnPropertyDescriptor => {
            require_object_coercible(target)?;
            Ok(descriptor(target, key))
        }
        _ => Ok(Value::Undefined),
    }
}

fn target_and_key<'a>(
    receiver: Option<&'a Value>,
    arguments: &'a [Value],
) -> (Option<&'a Value>, Option<&'a Value>) {
    receiver.map_or_else(
        || (arguments.first(), arguments.get(1)),
        |value| (Some(value), arguments.first()),
    )
}

fn has_own_property_result(
    receiver: Option<&Value>,
    key: Option<&Value>,
) -> Result<Value, VmError> {
    let Some(key) = key.map(value_to_string) else {
        return Ok(Value::Boolean(false));
    };
    let receiver = require_object_coercible(receiver)?;
    Ok(Value::Boolean(owns_property(receiver, &key)?))
}

fn require_object_coercible(receiver: Option<&Value>) -> Result<&Value, VmError> {
    match receiver {
        Some(Value::Null) | Some(Value::Undefined) | None => {
            Err(VmError::Thrown(crate::builtins::error(
                Builtin::TypeError,
                &[Value::String(
                    "Cannot convert undefined or null to object".to_string(),
                )],
            )))
        }
        Some(value) => Ok(value),
    }
}

fn owns_property(receiver: &Value, key: &str) -> Result<bool, VmError> {
    Ok(match receiver {
        Value::Object(properties) => properties.iter().any(|(name, _)| name == key),
        Value::Array(values) => key == "length" || valid_index(key, values.len()),
        Value::String(value) => key == "length" || valid_index(key, value.chars().count()),
        Value::Builtin(builtin) => builtin_owns_property(*builtin, key),
        Value::Function(function) => {
            matches!(key, "length" | "prototype")
                || function
                    .properties
                    .borrow()
                    .iter()
                    .rev()
                    .any(|(name, _)| name == key)
        }
        Value::Proxy(_) => {
            crate::proxy::proxy_get_own_property_descriptor(receiver, key)? != Value::Undefined
        }
        _ => false,
    })
}

fn builtin_owns_property(builtin: Builtin, key: &str) -> bool {
    (builtin == Builtin::Object && key == "hasOwn")
        || super::callable_property(builtin, key).is_some()
        || super::special_property(builtin, key).is_some()
}

fn valid_index(key: &str, len: usize) -> bool {
    key.parse::<usize>().is_ok_and(|index| index < len)
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
    execute_special(builtin, receiver, arguments).unwrap_or(Value::Undefined)
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
        Value::Builtin(builtin) => builtin_descriptor(*builtin, &key),
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
        Value::Function(function) => function
            .properties
            .borrow()
            .iter()
            .rev()
            .find(|(name, _)| name == &key)
            .map(|(_, value)| value.clone()),
        _ => None,
    };
    property.map_or(Value::Undefined, |property| descriptor_object(&property))
}

fn builtin_descriptor(builtin: Builtin, key: &str) -> Option<Value> {
    super::callable_property(builtin, key).or_else(|| super::special_property(builtin, key))
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

#[cfg(test)]
mod tests {
    use super::execute_special;
    use crate::{
        execute::{execute_builtin_with_receiver, VmError},
        ops::Builtin,
        value::{FunctionValue, Value},
    };
    use std::{cell::RefCell, rc::Rc};

    #[test]
    fn static_has_own_uses_first_argument_as_target() {
        let result = execute_special(
            Builtin::ObjectHasOwnProperty,
            None,
            &[
                Value::Builtin(Builtin::Object),
                Value::String("hasOwn".to_string()),
            ],
        )
        .unwrap();
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn has_own_throws_on_nullish_target() {
        let error = execute_builtin_with_receiver(
            Builtin::ObjectHasOwnProperty,
            &[Value::Null, Value::String("x".to_string())],
            None,
        )
        .unwrap_err();
        assert!(matches!(error, VmError::Thrown(_)));
    }

    #[test]
    fn get_own_property_descriptor_throws_on_nullish_target() {
        let error = execute_builtin_with_receiver(
            Builtin::ObjectGetOwnPropertyDescriptor,
            &[Value::Null, Value::String("x".to_string())],
            None,
        )
        .unwrap_err();
        assert!(matches!(error, VmError::Thrown(_)));
    }

    #[test]
    fn has_own_observes_function_own_properties() {
        let function = Value::Function(Rc::new(FunctionValue {
            body: Vec::new(),
            params: 2,
            captures: Rc::new(RefCell::new(Vec::new())),
            properties: Rc::new(RefCell::new(vec![(
                "custom".to_string(),
                Value::Boolean(true),
            )])),
        }));
        assert_eq!(
            execute_special(
                Builtin::ObjectHasOwnProperty,
                None,
                &[function.clone(), Value::String("length".to_string())],
            )
            .unwrap(),
            Value::Boolean(true)
        );
        assert_eq!(
            execute_special(
                Builtin::ObjectHasOwnProperty,
                None,
                &[function, Value::String("custom".to_string())],
            )
            .unwrap(),
            Value::Boolean(true)
        );
    }
}
