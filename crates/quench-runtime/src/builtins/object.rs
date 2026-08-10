use std::rc::Rc;

use crate::{execute::VmError, ops::Builtin, value::Value};

pub(crate) fn boxed_constructor(value: &Value) -> Builtin {
    match value {
        Value::String(value) if value.contains('\0') => Builtin::Symbol,
        Value::String(_) => Builtin::String,
        Value::Number(_) => Builtin::Number,
        Value::Boolean(_) => Builtin::Boolean,
        Value::BigInt(_) => Builtin::BigInt,
        _ => Builtin::Object,
    }
}

pub(crate) fn has_own_property(receiver: Option<&Value>, key: Option<&Value>) -> Value {
    has_own_property_result(receiver, key).unwrap_or(Value::Boolean(false))
}

pub(crate) fn get_prototype_of(value: Option<&Value>) -> Result<Value, VmError> {
    let value = require_object_coercible(value)?;
    Ok(match value {
        Value::Builtin(builtin) if is_typed_array_constructor(*builtin) => {
            Value::Builtin(Builtin::TypedArray)
        }
        Value::Builtin(_) | Value::Function(_) | Value::BoundFunction(_) => {
            Value::Builtin(Builtin::FunctionPrototype)
        }
        Value::Array(_) => Value::Builtin(Builtin::ArrayPrototype),
        Value::Object(_) => Value::Builtin(Builtin::ObjectPrototype),
        _ => Value::Null,
    })
}

fn is_typed_array_constructor(builtin: Builtin) -> bool {
    matches!(
        builtin,
        Builtin::Float64Array
            | Builtin::Float32Array
            | Builtin::Int8Array
            | Builtin::Int16Array
            | Builtin::Int32Array
            | Builtin::Uint8Array
            | Builtin::Uint16Array
            | Builtin::Uint32Array
            | Builtin::Uint8ClampedArray
    )
}

pub(crate) fn execute_special(
    builtin: Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    match builtin {
        Builtin::ObjectHasOwnProperty => {
            let (target, key) = has_own_target(receiver, arguments);
            has_own_property_result(target, key)
        }
        Builtin::ObjectPropertyIsEnumerable => {
            Ok(object_property_is_enumerable(receiver, arguments))
        }
        Builtin::ObjectGetOwnPropertyDescriptor => {
            let (target, key) = static_target(arguments);
            require_object_coercible(target)?;
            Ok(descriptor(target, key))
        }
        _ => Ok(Value::Undefined),
    }
}

fn has_own_target<'a>(
    receiver: Option<&'a Value>,
    arguments: &'a [Value],
) -> (Option<&'a Value>, Option<&'a Value>) {
    if receiver.is_none() || matches!(receiver, Some(Value::Builtin(Builtin::Object))) {
        return static_target(arguments);
    }
    (receiver, arguments.first())
}

fn static_target(arguments: &[Value]) -> (Option<&Value>, Option<&Value>) {
    (arguments.first(), arguments.get(1))
}

fn has_own_property_result(
    receiver: Option<&Value>,
    key: Option<&Value>,
) -> Result<Value, VmError> {
    let Some(key) = key else {
        return Ok(Value::Boolean(false));
    };
    let key = crate::properties::dynamic_property_key(key)?;
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
        Value::Object(properties) => properties
            .iter()
            .any(|(name, _)| name == key && !super::is_descriptor_key(name)),
        Value::Array(values) => {
            (!values.is_arguments() && key == "length")
                || valid_index(key, values.len())
                || values.property(key).is_some()
                || (values.is_strict_arguments() && key == "callee")
        }
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
    let (Some(receiver), Some(key)) = (receiver, arguments.first()) else {
        return Value::Boolean(false);
    };
    let Ok(key) = crate::properties::dynamic_property_key(key) else {
        return Value::Boolean(false);
    };
    let owned = owns_property(receiver, &key).unwrap_or(false);
    let enumerable = crate::builtins::descriptor_flag(receiver, &key, "enumerable").unwrap_or(true);
    Value::Boolean(owned && enumerable)
}

pub(crate) fn object_special(
    builtin: Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Value {
    execute_special(builtin, receiver, arguments).unwrap_or(Value::Undefined)
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

pub(crate) fn descriptor(value: Option<&Value>, key: Option<&Value>) -> Value {
    let (Some(value), Some(key)) = (value, key.map(value_to_string)) else {
        return Value::Undefined;
    };
    let descriptor = match value {
        Value::Object(properties) => object_descriptor(properties, &key),
        Value::Array(values) => array_descriptor(values, &key),
        Value::String(value) => string_descriptor(value, &key),
        Value::Builtin(builtin) => builtin_descriptor(*builtin, &key),
        Value::Function(function) if key == "length" => Some(Value::Object(Rc::new(vec![
            (
                "value".to_string(),
                Value::Number(f64::from(function.params)),
            ),
            ("writable".to_string(), Value::Boolean(false)),
            ("enumerable".to_string(), Value::Boolean(false)),
            ("configurable".to_string(), Value::Boolean(true)),
        ]))),
        Value::Function(function) => function
            .properties
            .borrow()
            .iter()
            .rev()
            .find(|(name, _)| name == &key)
            .map(|(_, value)| descriptor_object(value)),
        _ => None,
    };
    descriptor.unwrap_or(Value::Undefined)
}

fn object_descriptor(properties: &[(String, Value)], key: &str) -> Option<Value> {
    if let Some((_, metadata)) = properties
        .iter()
        .rev()
        .find(|(name, _)| name == &super::descriptor_key(key))
    {
        return Some(metadata.clone());
    }
    properties
        .iter()
        .rev()
        .find(|(name, _)| name == key)
        .map(|(_, value)| descriptor_object(value))
}

fn builtin_descriptor(builtin: Builtin, key: &str) -> Option<Value> {
    let property =
        super::callable_property(builtin, key).or_else(|| super::special_property(builtin, key))?;
    let (writable, enumerable) = match key {
        "length" | "name" => (false, false),
        _ => (true, false),
    };
    Some(descriptor_object_with_flags(
        property, writable, enumerable, true,
    ))
}

fn array_descriptor(values: &crate::value::ArrayData, key: &str) -> Option<Value> {
    if let Some(descriptor) = values.descriptor(key) {
        return Some(refresh_array_descriptor(values, key, descriptor));
    }
    if values.is_strict_arguments() && key == "callee" {
        return Some(strict_callee_descriptor());
    }
    if values.is_arguments() && matches!(key, "length" | "callee") {
        return values
            .property(key)
            .map(|value| descriptor_object_with_flags(value, true, false, true));
    }
    if values.is_arguments() && key == "Symbol.iterator" {
        return values
            .property(key)
            .map(|value| descriptor_object_with_flags(value, true, false, true));
    }
    if key == "length" {
        return Some(descriptor_object_with_flags(
            Value::Number(values.logical_len() as f64),
            true,
            false,
            false,
        ));
    }
    key.parse::<usize>()
        .ok()
        .and_then(|index| values.get_index(index))
        .map(|value| descriptor_object(&value))
}

fn refresh_array_descriptor(
    values: &crate::value::ArrayData,
    key: &str,
    mut descriptor: Value,
) -> Value {
    let (Ok(index), Value::Object(properties)) = (key.parse::<usize>(), &mut descriptor) else {
        return descriptor;
    };
    let Some(value) = values.get_index(index) else {
        return descriptor;
    };
    if let Some((_, current)) = Rc::make_mut(properties)
        .iter_mut()
        .find(|(name, _)| name == "value")
    {
        *current = value;
    }
    descriptor
}

fn strict_callee_descriptor() -> Value {
    let thrower = Value::Builtin(Builtin::TypeError);
    Value::Object(Rc::new(vec![
        ("get".to_string(), thrower.clone()),
        ("set".to_string(), thrower),
        ("enumerable".to_string(), Value::Boolean(false)),
        ("configurable".to_string(), Value::Boolean(false)),
    ]))
}

fn string_descriptor(value: &str, key: &str) -> Option<Value> {
    value
        .chars()
        .nth(key.parse::<usize>().ok()?)
        .map(|character| {
            descriptor_object_with_flags(Value::String(character.to_string()), false, true, false)
        })
}

fn descriptor_object(value: &Value) -> Value {
    descriptor_object_with_flags(value.clone(), true, true, true)
}

fn descriptor_object_with_flags(
    value: Value,
    writable: bool,
    enumerable: bool,
    configurable: bool,
) -> Value {
    Value::Object(Rc::new(vec![
        ("value".to_string(), value),
        ("writable".to_string(), Value::Boolean(writable)),
        ("enumerable".to_string(), Value::Boolean(enumerable)),
        ("configurable".to_string(), Value::Boolean(configurable)),
    ]))
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
            kind: crate::ops::FunctionKind::Ordinary,
            strictness: crate::ops::FunctionStrictness::Sloppy,
            is_async: false,
            mapped_arguments: true,
            captures: crate::environment::Environment::new(),
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
