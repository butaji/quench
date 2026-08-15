pub(crate) fn integrity_level(
    target: Option<&crate::value::Value>,
    frozen: bool,
) -> Result<crate::value::Value, crate::execute::VmError> {
    let Some(target) = target else {
        return Err(crate::execute::VmError::NotCallable);
    };
    if !crate::value::is_object(target) {
        return Ok(crate::value::Value::Boolean(true));
    }
    if matches!(target, crate::value::Value::Proxy(_)) {
        return crate::proxy::proxy_is_extensible(target);
    }
    if object_is_extensible(target) {
        return Ok(crate::value::Value::Boolean(false));
    }
    Ok(crate::value::Value::Boolean(integrity_props(
        target, frozen,
    )))
}

fn integrity_props(target: &crate::value::Value, frozen: bool) -> bool {
    let Ok(crate::value::Value::Array(keys)) = crate::own_keys::names(Some(target)) else {
        return false;
    };
    keys.snapshot().iter().all(|key| {
        let crate::value::Value::String(key) = key else {
            return true;
        };
        if crate::builtins::descriptor_flag(target, key, "configurable").unwrap_or(true) {
            return false;
        }
        !frozen || crate::builtins::descriptor_flag(target, key, "writable") != Some(true)
    })
}

pub(crate) fn integrity_apply(
    target: Option<&crate::value::Value>,
    frozen: bool,
) -> Result<crate::value::Value, crate::execute::VmError> {
    let Some(target) = target else {
        return Err(crate::value::error::throw_type_error("Object expected"));
    };
    if matches!(target, crate::value::Value::Proxy(_)) {
        return crate::proxy::proxy_prevent_extensions(target);
    }
    match target {
        crate::value::Value::Object(properties) => {
            let mut sealed = integrity_properties(properties, frozen);
            push_non_extensible(&mut sealed);
            Ok(replace_object(target, sealed))
        }
        crate::value::Value::Array(values) => Ok(integrity_array(values, target, frozen)),
        _ => Ok(target.clone()),
    }
}

fn integrity_properties(
    properties: &crate::value::ObjectData,
    frozen: bool,
) -> crate::value::ObjectData {
    let mut sealed = properties.properties.clone();
    let keys: Vec<String> = sealed
        .iter()
        .map(|(name, _)| name.clone())
        .filter(|name| !name.starts_with('\0') && !crate::builtins::is_descriptor_key(name))
        .collect();
    for key in keys.iter() {
        let metadata_key = crate::builtins::descriptor_key(key);
        let metadata = integrity_descriptor(&sealed, &metadata_key, frozen);
        sealed.retain(|(name, _)| name != &metadata_key);
        sealed.push((metadata_key, metadata));
    }
    crate::value::ObjectData::with_private_slots(
        sealed,
        std::rc::Rc::clone(&properties.private_slots),
    )
}

fn integrity_descriptor(
    properties: &crate::value::ObjectProperties,
    metadata_key: &str,
    frozen: bool,
) -> crate::value::Value {
    let existing = properties
        .iter()
        .rev()
        .find_map(|(name, value)| (name == metadata_key).then_some(value.clone()));
    let flag = |field: &str| descriptor_flag_field(existing.as_ref(), field).unwrap_or(true);
    let accessor = existing.as_ref().is_some_and(is_accessor_descriptor);
    let mut fields = Vec::new();
    if accessor {
        for field in ["get", "set"] {
            if let Some(value) = descriptor_field_value(existing.as_ref(), field) {
                fields.push((field.to_string(), value));
            }
        }
    } else {
        if let Some(value) = descriptor_field_value(existing.as_ref(), "value")
            .or_else(|| property_value(properties, metadata_key))
        {
            fields.push(("value".to_string(), value));
        }
        fields.push((
            "writable".to_string(),
            crate::value::Value::Boolean(!frozen && flag("writable")),
        ));
    }
    fields.push((
        "enumerable".to_string(),
        crate::value::Value::Boolean(flag("enumerable")),
    ));
    fields.push((
        "configurable".to_string(),
        crate::value::Value::Boolean(false),
    ));
    descriptor_object(fields)
}

fn property_value(
    properties: &crate::value::ObjectProperties,
    metadata_key: &str,
) -> Option<crate::value::Value> {
    let key = metadata_key.strip_prefix(crate::builtins::descriptor_key("").as_str())?;
    properties
        .iter()
        .rev()
        .find_map(|(name, value)| (name == key).then_some(value.clone()))
}

fn is_accessor_descriptor(value: &crate::value::Value) -> bool {
    matches!(value, crate::value::Value::Object(fields) if fields.iter().any(|(name, _)| matches!(name.as_str(), "get" | "set")))
}

fn descriptor_flag_field(value: Option<&crate::value::Value>, field: &str) -> Option<bool> {
    let crate::value::Value::Object(fields) = value? else {
        return None;
    };
    fields.iter().rev().find_map(|(name, value)| {
        (name == field).then_some(matches!(value, crate::value::Value::Boolean(true)))
    })
}

fn descriptor_field_value(
    value: Option<&crate::value::Value>,
    field: &str,
) -> Option<crate::value::Value> {
    let crate::value::Value::Object(fields) = value? else {
        return None;
    };
    fields
        .iter()
        .rev()
        .find_map(|(name, value)| (name == field).then_some(value.clone()))
}

fn descriptor_object(fields: Vec<(String, crate::value::Value)>) -> crate::value::Value {
    crate::value::Value::Object(std::rc::Rc::new(crate::value::ObjectData::new(fields)))
}

fn push_non_extensible(properties: &mut crate::value::ObjectData) {
    if !properties.iter().any(|(name, _)| name == NON_EXTENSIBLE) {
        properties.push((
            NON_EXTENSIBLE.to_string(),
            crate::value::Value::Boolean(true),
        ));
    }
}

fn replace_object(
    target: &crate::value::Value,
    sealed: crate::value::ObjectData,
) -> crate::value::Value {
    let result = crate::value::Value::Object(std::rc::Rc::new(sealed));
    crate::locals::replace_value(target, &result);
    if crate::vm::is_global_object(target) {
        let mut registers = Vec::new();
        crate::vm::synchronize_global_object(&mut registers, target, &result);
    }
    result
}

fn integrity_array(
    values: &std::rc::Rc<crate::value::ArrayData>,
    target: &crate::value::Value,
    frozen: bool,
) -> crate::value::Value {
    let mut values = std::rc::Rc::clone(values);
    let data = std::rc::Rc::make_mut(&mut values);
    data.set_property(NON_EXTENSIBLE, crate::value::Value::Boolean(true));
    for key in integrity_array_keys(data) {
        let metadata = integrity_array_descriptor(data, &key, frozen);
        data.define_descriptor(&key, metadata);
    }
    let result = crate::value::Value::Array(values);
    crate::locals::replace_value(target, &result);
    result
}

fn integrity_array_keys(data: &crate::value::ArrayData) -> Vec<String> {
    let mut keys: Vec<String> = (0..data.logical_len().min(data.len()))
        .filter(|index| data.has_index(*index))
        .map(|index| index.to_string())
        .collect();
    keys.push("length".to_string());
    for key in data
        .property_keys()
        .into_iter()
        .chain(data.descriptor_keys())
    {
        if !key.contains('\0') && key != "length" && !keys.contains(&key) {
            keys.push(key);
        }
    }
    keys
}

fn integrity_array_descriptor(
    data: &crate::value::ArrayData,
    key: &str,
    frozen: bool,
) -> crate::value::Value {
    let existing = data.descriptor(key);
    let accessor = existing.as_ref().is_some_and(is_accessor_descriptor);
    let enumerable =
        descriptor_flag_field(existing.as_ref(), "enumerable").unwrap_or(true) && key != "length";
    let mut fields = Vec::new();
    if !accessor || key == "length" {
        if key != "length" {
            let value = crate::arrays::array_index(key)
                .and_then(|index| data.get_index(index as usize))
                .or_else(|| data.property(key));
            if let Some(value) = value {
                fields.push(("value".to_string(), value));
            }
        }
        fields.push((
            "writable".to_string(),
            crate::value::Value::Boolean(key != "length" && !frozen),
        ));
    }
    fields.push((
        "enumerable".to_string(),
        crate::value::Value::Boolean(enumerable),
    ));
    fields.push((
        "configurable".to_string(),
        crate::value::Value::Boolean(false),
    ));
    descriptor_object(fields)
}
