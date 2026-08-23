pub(crate) fn execute_copy_data_properties(
    registers: &mut Vec<crate::value::Value>,
    op: &Op,
) -> Result<(), crate::execute::VmError> {
    let Op::CopyDataProperties {
        target,
        source,
        excluded,
    } = op
    else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    let source = crate::execute::read_register(registers, *source)?;
    if matches!(
        source,
        crate::value::Value::Null | crate::value::Value::Undefined
    ) {
        return Ok(());
    }
    let excluded = excluded_keys(registers, excluded)?;
    let keys = enumerable_own_keys(&source)?;
    for key in keys.into_iter().filter(|key| !excluded.contains(key)) {
        copy_data_property(registers, *target, &source, &key)?;
    }
    Ok(())
}

pub(crate) fn assign_properties(
    target: crate::value::Value,
    sources: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    if matches!(target, crate::value::Value::Null | crate::value::Value::Undefined) {
        return Err(crate::value::error::throw_type_error(
            "Cannot convert undefined or null to object",
        ));
    }
    let mut target = crate::builtins::object(&[target]);
    for source in sources {
        if matches!(source, crate::value::Value::Null | crate::value::Value::Undefined) {
            continue;
        }
        for key in enumerable_own_keys(source)? {
            let value = if matches!(source, crate::value::Value::Proxy(_)) {
                crate::proxy::proxy_get(source, &key, Some(source))?
            } else {
                crate::execute::get_property_result(source, &key)?
            };
            target = assign_property(&target, &key, value)?;
        }
    }
    Ok(target)
}

fn assign_property(
    target: &crate::value::Value,
    key: &str,
    value: crate::value::Value,
) -> Result<crate::value::Value, crate::execute::VmError> {
    let updated = crate::properties::assign_set_property(target, key, value)?;
    crate::locals::replace_value(target, &updated);
    Ok(updated)
}

fn excluded_keys(
    registers: &[crate::value::Value],
    excluded: &[u16],
) -> Result<Vec<String>, crate::execute::VmError> {
    excluded
        .iter()
        .map(|key| dynamic_property_key(&crate::execute::read_register(registers, *key)?))
        .collect()
}

fn enumerable_own_keys(
    source: &crate::value::Value,
) -> Result<Vec<String>, crate::execute::VmError> {
    let keys = own_keys(source)?;
    let mut result = Vec::new();
    for key in keys {
        if own_property_is_enumerable(source, &key)? {
            result.push(key);
        }
    }
    Ok(result)
}

fn own_keys(source: &crate::value::Value) -> Result<Vec<String>, crate::execute::VmError> {
    match source {
        crate::value::Value::Proxy(_) => key_strings(crate::proxy::proxy_own_keys(source)?),
        crate::value::Value::Array(values) => Ok((0..values.logical_len())
            .filter(|index| values.has_index(*index))
            .map(|index| index.to_string())
            .collect()),
        crate::value::Value::String(value) => Ok((0..crate::strings::utf16_len(value))
            .map(|index| index.to_string())
            .collect()),
        _ => {
            let mut names = key_strings(crate::own_keys::names(Some(source))?)?;
            names.extend(key_strings(crate::own_keys::symbols(Some(source))?)?);
            Ok(names)
        }
    }
}

fn key_strings(value: crate::value::Value) -> Result<Vec<String>, crate::execute::VmError> {
    let crate::value::Value::Array(keys) = value else {
        return Ok(Vec::new());
    };
    keys.iter().map(dynamic_property_key).collect()
}

fn own_property_is_enumerable(
    source: &crate::value::Value,
    key: &str,
) -> Result<bool, crate::execute::VmError> {
    let descriptor = crate::proxy::proxy_get_own_property_descriptor(source, key)?;
    let crate::value::Value::Object(fields) = descriptor else {
        return Ok(false);
    };
    Ok(fields.iter().rev().any(|(name, value)| {
        name == "enumerable" && matches!(value, crate::value::Value::Boolean(true))
    }))
}

fn copy_data_property(
    registers: &mut Vec<crate::value::Value>,
    target_register: u16,
    source: &crate::value::Value,
    key: &str,
) -> Result<(), crate::execute::VmError> {
    let value = if matches!(source, crate::value::Value::Proxy(_)) {
        crate::proxy::proxy_get(source, key, Some(source))?
    } else {
        crate::execute::get_property_result(source, key)?
    };
    let target = crate::execute::read_register(registers, target_register)?;
    let descriptor = data_descriptor(value);
    let result = crate::builtins::define_own_property(&target, key, &descriptor)?;
    crate::locals::replace_value(&target, &result);
    crate::execute::write_value(registers, target_register, result);
    Ok(())
}

fn data_descriptor(value: crate::value::Value) -> Vec<(String, crate::value::Value)> {
    use crate::value::Value::Boolean;
    vec![
        ("value".to_string(), value),
        ("writable".to_string(), Boolean(true)),
        ("enumerable".to_string(), Boolean(true)),
        ("configurable".to_string(), Boolean(true)),
    ]
}
