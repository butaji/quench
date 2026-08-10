use crate::{execute::VmError, value::Value};

pub(crate) fn names(target: Option<&Value>) -> Result<Value, VmError> {
    let keys = keys(require_object(target)?, false);
    Ok(Value::array(keys.into_iter().map(Value::String).collect()))
}

pub(crate) fn symbols(target: Option<&Value>) -> Result<Value, VmError> {
    let keys = keys(require_object(target)?, true);
    Ok(Value::array(keys.into_iter().map(Value::String).collect()))
}

pub(crate) fn enumerable_names(target: Option<&Value>) -> Value {
    Value::array(
        enumerable_key_strings(target)
            .into_iter()
            .map(Value::String)
            .collect(),
    )
}

pub(crate) fn enumerable_key_strings(target: Option<&Value>) -> Vec<String> {
    let Some(target) = target else {
        return Vec::new();
    };
    let properties = match target {
        Value::Object(properties) => properties.as_slice(),
        Value::Function(function) => return enumerable_function(function),
        _ => return Vec::new(),
    };
    enumerable(properties)
}

fn enumerable_function(function: &crate::value::FunctionValue) -> Vec<String> {
    let properties = function
        .properties
        .borrow()
        .iter()
        .filter(|(key, _)| key != "prototype")
        .cloned()
        .collect::<Vec<_>>();
    enumerable(&properties)
}

fn enumerable(properties: &[(String, Value)]) -> Vec<String> {
    ordered(properties, false)
        .into_iter()
        .filter(|key| descriptor_enumerable(properties, key))
        .collect()
}

fn descriptor_enumerable(properties: &[(String, Value)], key: &str) -> bool {
    let metadata = crate::builtins::descriptor_key(key);
    let descriptor = properties
        .iter()
        .rev()
        .find_map(|(name, value)| (name == &metadata).then_some(value));
    let Some(Value::Object(descriptor)) = descriptor else {
        return true;
    };
    descriptor
        .iter()
        .rev()
        .any(|(name, value)| name == "enumerable" && matches!(value, Value::Boolean(true)))
}

fn keys(target: &Value, symbols: bool) -> Vec<String> {
    let properties = match target {
        Value::Object(properties) => properties.as_slice(),
        Value::ObjectAlias(alias) => {
            return alias
                .0
                .borrow()
                .upgrade()
                .map_or_else(Vec::new, |properties| ordered(&properties, symbols));
        }
        Value::Function(function) => return function_keys(function, symbols),
        _ => return Vec::new(),
    };
    ordered(properties, symbols)
}

fn function_keys(function: &crate::value::FunctionValue, symbols: bool) -> Vec<String> {
    ordered(&function.properties.borrow(), symbols)
}

fn ordered(properties: &[(String, Value)], symbols: bool) -> Vec<String> {
    let mut indices = Vec::new();
    let mut strings = Vec::new();
    for (key, _) in properties {
        if crate::builtins::is_descriptor_key(key) || key.starts_with('\0') {
            continue;
        }
        if key.contains('\0') != symbols {
            continue;
        }
        match array_index(key) {
            Some(index) if !symbols => indices.push((index, key.clone())),
            _ => strings.push(key.clone()),
        }
    }
    indices.sort_by_key(|(index, _)| *index);
    indices
        .into_iter()
        .map(|(_, key)| key)
        .chain(strings)
        .collect()
}

fn array_index(key: &str) -> Option<u32> {
    let index = key.parse::<u32>().ok()?;
    (index != u32::MAX && index.to_string() == key).then_some(index)
}

fn require_object(target: Option<&Value>) -> Result<&Value, VmError> {
    match target {
        None | Some(Value::Null | Value::Undefined) => Err(crate::value::error::throw_type_error(
            "Cannot convert undefined or null to object",
        )),
        Some(target) => Ok(target),
    }
}
