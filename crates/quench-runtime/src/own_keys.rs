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
        Value::ObjectAlias(alias) => {
            return alias
                .0
                .borrow()
                .upgrade()
                .map_or_else(Vec::new, |properties| enumerable(&properties));
        }
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
        Value::Array(values) => return array_keys(values, symbols),
        Value::Builtin(builtin) => {
            return crate::builtins::own_property_names(*builtin)
                .iter()
                .map(|key| (*key).to_string())
                .filter(|key| key.contains('\0') == symbols)
                .collect();
        }
        _ => return Vec::new(),
    };
    ordered(properties, symbols)
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
