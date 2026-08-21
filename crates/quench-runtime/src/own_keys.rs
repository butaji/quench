use std::collections::HashSet;

use crate::{execute::VmError, value::Value};

pub(crate) fn names(target: Option<&Value>) -> Result<Value, VmError> {
    pack_keys(listed(require_object(target)?, false)?)
}

pub(crate) fn symbols(target: Option<&Value>) -> Result<Value, VmError> {
    pack_keys(listed(require_object(target)?, true)?)
}

pub(crate) fn all(target: &Value) -> Result<Value, VmError> {
    if !crate::value::is_object(target) {
        return Err(crate::value::error::throw_type_error(
            "Reflect.ownKeys target must be an object",
        ));
    }
    crate::module_bindings::exports(target, "")?;
    let mut values = keys(target, false);
    values.extend(keys(target, true));
    pack_keys(values)
}

fn listed(target: &Value, symbols: bool) -> Result<Vec<String>, VmError> {
    crate::module_bindings::exports(target, "")?;
    Ok(keys(target, symbols))
}

fn pack_keys(keys: Vec<String>) -> Result<Value, VmError> {
    Ok(Value::array(
        keys.into_iter()
            .map(|key| crate::conversion::own_key_value(&key))
            .collect(),
    ))
}

pub(crate) fn keys_result(target: Option<&Value>) -> Result<Value, VmError> {
    let target = require_object(target)?;
    Ok(Value::array(
        own_enumerable_string_keys(target)
            .into_iter()
            .map(Value::String)
            .collect(),
    ))
}

pub(crate) fn values(target: Option<&Value>, entries: bool) -> Result<Value, VmError> {
    let target = require_object(target)?;
    let mut result = Vec::new();
    for key in keys(target, false) {
        let current = crate::locals::resolved_replacement(target.clone());
        let descriptor = crate::proxy::proxy_get_own_property_descriptor(&current, &key)?;
        if !descriptor_enumerable_value_opt(&descriptor) {
            continue;
        }
        let value = crate::execute::get_property_result(&current, &key)?;
        result.push(if entries {
            Value::array(vec![Value::String(key), value])
        } else {
            value
        });
    }
    Ok(Value::array(result))
}

fn descriptor_enumerable_value_opt(descriptor: &Value) -> bool {
    match descriptor {
        Value::Object(_) => descriptor_enumerable_value(Some(descriptor)),
        _ => false,
    }
}

fn typed_array_enumerable_keys(value: &Value) -> Option<Vec<String>> {
    let length = typed_array_length(value)?;
    Some((0..length).map(|index| index.to_string()).collect())
}

fn typed_array_length(value: &Value) -> Option<usize> {
    Some(match value {
        Value::Float64Array(view) => view.logical_len(),
        Value::Float32Array(view) => view.logical_len(),
        Value::Int8Array(view) => view.logical_len(),
        Value::Int16Array(view) => view.logical_len(),
        Value::Uint8Array(view) => view.logical_len(),
        Value::Uint8ClampedArray(view) => view.logical_len(),
        Value::Uint16Array(view) => view.logical_len(),
        Value::Int32Array(view) => view.logical_len(),
        Value::Uint32Array(view) => view.logical_len(),
        Value::BigInt64Array(view) => view.logical_len(),
        Value::BigUint64Array(view) => view.logical_len(),
        _ => return None,
    })
}

fn own_enumerable_string_keys(target: &Value) -> Vec<String> {
    if let Some(keys) = typed_array_enumerable_keys(target) {
        return keys;
    }
    match target {
        Value::Object(properties) => object_enumerable_keys(properties),
        Value::ObjectAlias(alias) => alias
            .0
            .borrow()
            .upgrade()
            .map_or_else(Vec::new, |properties| object_enumerable_keys(&properties)),
        Value::Function(function) => {
            let properties = function.properties.borrow();
            enumerable_ordered(&properties)
                .into_iter()
                .filter(|key| key != "prototype")
                .collect()
        }
        Value::BoundFunction(bound) => enumerable_ordered(&bound.properties.borrow())
            .into_iter()
            .filter(|key| key != "prototype")
            .collect(),
        Value::Array(values) => array_enumerable_keys(values),
        Value::Builtin(builtin) => builtin_enumerable_keys(*builtin),
        Value::String(value) if !crate::conversion::is_symbol_string(value) => {
            string_indices(value)
        }
        _ => Vec::new(),
    }
}

fn object_enumerable_keys(data: &crate::value::ObjectData) -> Vec<String> {
    let properties: &[(String, Value)] = data;
    if is_boxed_primitive(properties) {
        return enumerable_ordered(properties)
            .into_iter()
            .filter(|key| key != "_value" && key != "constructor")
            .collect();
    }
    let Some((_, value)) = properties.iter().find(|(key, _)| key == "_value") else {
        return enumerable_created(data)
            .into_iter()
            .filter(|key| key != "timeValue")
            .collect();
    };
    if matches!(value, Value::String(value) if crate::conversion::is_symbol_string(value)) {
        return enumerable_ordered(properties)
            .into_iter()
            .filter(|key| key != "_value" && key != "constructor")
            .collect();
    }
    let mut keys = match value {
        Value::String(value) => string_indices(value),
        _ => Vec::new(),
    };
    keys.extend(
        enumerable_ordered(properties)
            .into_iter()
            .filter(|key| !matches!(key.as_str(), "_value" | "constructor")),
    );
    keys
}

fn is_boxed_primitive(properties: &[(String, Value)]) -> bool {
    let has_value = properties.iter().any(|(key, _)| key == "_value");
    let has_constructor = properties.iter().any(|(key, _)| key == "constructor");
    has_value && has_constructor
}

fn enumerable_created(data: &crate::value::ObjectData) -> Vec<String> {
    let properties: Vec<(String, Value)> = data
        .created
        .iter()
        .filter(|key| !key.starts_with('\0'))
        .filter(|key| descriptor_enumerable(data, key))
        .map(|key| (key.clone(), Value::Undefined))
        .collect();
    ordered(&properties, false)
}

fn enumerable_ordered(properties: &[(String, Value)]) -> Vec<String> {
    ordered(properties, false)
        .into_iter()
        .filter(|key| !key.starts_with('\0'))
        .filter(|key| descriptor_enumerable(properties, key))
        .collect()
}

fn string_indices(value: &str) -> Vec<String> {
    (0..crate::strings::utf16_len(value))
        .map(|index| index.to_string())
        .collect()
}

fn array_enumerable_keys(values: &crate::value::ArrayData) -> Vec<String> {
    let mut indices = dense_enumerable_indices(values);
    let mut named = Vec::new();
    for key in array_extra_keys(values) {
        if !array_key_enumerable(values, &key) {
            continue;
        }
        match array_index(&key) {
            Some(index) => indices.push((index, key)),
            None => named.push(key),
        }
    }
    indices.sort_by_key(|(index, _)| *index);
    let mut keys: Vec<String> = indices.into_iter().map(|(_, key)| key).collect();
    keys.extend(named);
    keys.dedup();
    keys
}

fn dense_enumerable_indices(values: &crate::value::ArrayData) -> Vec<(u32, String)> {
    (0..values.logical_len().min(values.len()))
        .filter(|index| values.has_index(*index))
        .map(|index| index as u32)
        .filter(|index| array_key_enumerable(values, &index.to_string()))
        .map(|index| (index, index.to_string()))
        .collect()
}

fn array_extra_keys(values: &crate::value::ArrayData) -> Vec<String> {
    let mut extra = values.property_keys();
    extra.extend(values.descriptor_keys());
    extra.retain(|key| key != "length" && !key.contains('\0'));
    extra.dedup();
    extra
}

include!("own_keys_tail.rs");
