use crate::{execute::VmError, value::Value};

pub(crate) fn names(target: Option<&Value>) -> Result<Value, VmError> {
    let keys = keys(require_object(target)?, false);
    Ok(Value::array(keys.into_iter().map(Value::String).collect()))
}

pub(crate) fn symbols(target: Option<&Value>) -> Result<Value, VmError> {
    let keys = keys(require_object(target)?, true);
    Ok(Value::array(keys.into_iter().map(Value::String).collect()))
}

pub(crate) fn all(target: &Value) -> Result<Value, VmError> {
    if !crate::value::is_object(target) {
        return Err(crate::value::error::throw_type_error(
            "Reflect.ownKeys target must be an object",
        ));
    }
    let mut values = keys(target, false);
    values.extend(keys(target, true));
    Ok(Value::array(
        values.into_iter().map(Value::String).collect(),
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
    for key in own_enumerable_string_keys(target) {
        let value = crate::execute::get_property_result(target, &key)?;
        result.push(if entries {
            Value::array(vec![Value::String(key), value])
        } else {
            value
        });
    }
    Ok(Value::array(result))
}

fn own_enumerable_string_keys(target: &Value) -> Vec<String> {
    match target {
        Value::Object(properties) => object_enumerable_keys(properties),
        Value::ObjectAlias(alias) => alias
            .0
            .borrow()
            .upgrade()
            .map_or_else(Vec::new, |properties| enumerable_ordered(&properties)),
        Value::Function(function) => {
            let properties = function.properties.borrow();
            enumerable_ordered(&properties)
                .into_iter()
                .filter(|key| key != "prototype")
                .collect()
        }
        Value::Array(values) => array_enumerable_keys(values),
        Value::String(value) if !crate::conversion::is_symbol_string(value) => {
            string_indices(value)
        }
        _ => Vec::new(),
    }
}

fn object_enumerable_keys(properties: &[(String, Value)]) -> Vec<String> {
    let Some((_, Value::String(value))) = properties.iter().find(|(key, _)| key == "_value") else {
        return enumerable_ordered(properties);
    };
    if crate::conversion::is_symbol_string(value) {
        return enumerable_ordered(properties);
    }
    let mut keys = string_indices(value);
    keys.extend(
        enumerable_ordered(properties)
            .into_iter()
            .filter(|key| !matches!(key.as_str(), "_value" | "constructor")),
    );
    keys
}

fn enumerable_ordered(properties: &[(String, Value)]) -> Vec<String> {
    ordered(properties, false)
        .into_iter()
        .filter(|key| descriptor_enumerable(properties, key))
        .collect()
}

fn string_indices(value: &str) -> Vec<String> {
    (0..crate::strings::utf16_len(value))
        .map(|index| index.to_string())
        .collect()
}

fn array_enumerable_keys(values: &crate::value::ArrayData) -> Vec<String> {
    let mut keys = indexed_array_keys(values, false);
    for key in values.property_keys() {
        if key == "length" || array_index(&key).is_some() || keys.contains(&key) {
            continue;
        }
        if descriptor_enumerable_value(values.descriptor(&key).as_ref()) {
            keys.push(key);
        }
    }
    keys
}

pub(crate) fn enumerable_key_strings(target: Option<&Value>) -> Vec<String> {
    match target {
        Some(target) => own_enumerable_string_keys(target),
        None => Vec::new(),
    }
}

fn object_keys(properties: &[(String, Value)], symbols: bool) -> Vec<String> {
    let Some((_, Value::String(value))) = properties.iter().find(|(key, _)| key == "_value") else {
        return ordered(properties, symbols);
    };
    if crate::conversion::is_symbol_string(value) {
        return ordered(properties, symbols);
    }
    boxed_string_keys(properties, value, symbols)
}

fn boxed_string_keys(properties: &[(String, Value)], value: &str, symbols: bool) -> Vec<String> {
    if symbols {
        return ordered(properties, true);
    }
    let mut keys = value
        .chars()
        .enumerate()
        .map(|(index, _)| index.to_string())
        .collect::<Vec<_>>();
    keys.push("length".to_string());
    keys.extend(
        ordered(properties, false)
            .into_iter()
            .filter(|key| !matches!(key.as_str(), "_value" | "constructor")),
    );
    keys
}

fn descriptor_enumerable(properties: &[(String, Value)], key: &str) -> bool {
    let metadata = crate::builtins::descriptor_key(key);
    let descriptor = properties
        .iter()
        .rev()
        .find_map(|(name, value)| (name == &metadata).then_some(value));
    descriptor_enumerable_value(descriptor)
}

fn descriptor_enumerable_value(descriptor: Option<&Value>) -> bool {
    let Some(Value::Object(descriptor)) = descriptor else {
        return true;
    };
    descriptor
        .iter()
        .rev()
        .any(|(name, value)| name == "enumerable" && matches!(value, Value::Boolean(true)))
}

fn keys(target: &Value, symbols: bool) -> Vec<String> {
    match target {
        Value::Object(properties) => object_keys(properties, symbols),
        Value::ObjectAlias(alias) => alias
            .0
            .borrow()
            .upgrade()
            .map_or_else(Vec::new, |properties| ordered(&properties, symbols)),
        Value::Function(function) => function_keys(function, symbols),
        Value::Array(values) => array_keys(values, symbols),
        Value::String(value) if !crate::conversion::is_symbol_string(value) => {
            string_keys(value, symbols)
        }
        Value::Builtin(builtin) => crate::builtins::own_property_names(*builtin)
            .iter()
            .map(|key| (*key).to_string())
            .filter(|key| key.contains('\0') == symbols)
            .collect(),
        _ => Vec::new(),
    }
}

fn string_keys(value: &str, symbols: bool) -> Vec<String> {
    if symbols {
        return Vec::new();
    }
    let mut keys = (0..crate::strings::utf16_len(value))
        .map(|index| index.to_string())
        .collect::<Vec<_>>();
    keys.push("length".to_string());
    keys
}

fn array_keys(values: &crate::value::ArrayData, symbols: bool) -> Vec<String> {
    let mut keys = indexed_array_keys(values, symbols);
    if !symbols {
        keys.push("length".to_string());
    }
    append_unique(
        &mut keys,
        ordered_properties(&values.property_keys(), symbols),
    );
    keys
}

fn indexed_array_keys(values: &crate::value::ArrayData, symbols: bool) -> Vec<String> {
    if symbols {
        return Vec::new();
    }
    (0..values.logical_len())
        .filter(|index| values.has_index(*index))
        .map(|index| index.to_string())
        .collect()
}

fn ordered_properties(keys: &[String], symbols: bool) -> Vec<String> {
    let properties = keys
        .iter()
        .map(|key| (key.clone(), Value::Undefined))
        .collect::<Vec<_>>();
    ordered(&properties, symbols)
}

fn append_unique(keys: &mut Vec<String>, additions: Vec<String>) {
    for key in additions {
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
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
    crate::arrays::array_index(key)
}

fn require_object(target: Option<&Value>) -> Result<&Value, VmError> {
    match target {
        None | Some(Value::Null | Value::Undefined) => Err(crate::value::error::throw_type_error(
            "Cannot convert undefined or null to object",
        )),
        Some(target) => Ok(target),
    }
}

#[cfg(test)]
mod tests {
    use super::names;
    use crate::value::Value;

    #[test]
    fn array_names_include_indices_and_length() {
        let array = Value::array(vec![Value::Boolean(true), Value::Null]);
        let Value::Array(names) = names(Some(&array)).expect("array names") else {
            panic!("own names result is not an array");
        };
        assert_eq!(
            &names[..],
            &[
                Value::String("0".into()),
                Value::String("1".into()),
                Value::String("length".into())
            ]
        );
    }
}
