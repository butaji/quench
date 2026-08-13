// `InternalizeJSONProperty`: reviver walk for `JSON.parse`.

pub(crate) fn internalize(parsed: Parsed, reviver: &Value) -> Result<Value, VmError> {
    let mut properties = vec![(String::new(), parsed.value)];
    if let Some(source) = parsed.source {
        let original = properties[0].1.clone();
        let pair = Value::array(vec![original, Value::String(source)]);
        properties.push((source_key(""), pair));
    }
    let holder = Value::Object(Rc::new(crate::value::ObjectData::new(properties)));
    walk(holder, "", reviver)
}

fn walk(holder: Value, key: &str, reviver: &Value) -> Result<Value, VmError> {
    let value = crate::execute::get_property_result(&holder, key)?;
    let value = crate::locals::resolved_replacement(resolve_alias(value));
    let value = revive_children(&value, reviver)?;
    let context = context_object(&holder, key, &value);
    crate::functions::execute_target(
        reviver,
        &holder,
        &[Value::String(key.to_string()), value, context],
    )
}

fn revive_children(value: &Value, reviver: &Value) -> Result<Value, VmError> {
    match value {
        Value::Array(_) | Value::Object(_) => revive_container(value, reviver),
        Value::Proxy(proxy) => {
            if crate::proxy::is_revoked(proxy) {
                return Err(revoked_error());
            }
            revive_container(value, reviver)
        }
        _ => Ok(value.clone()),
    }
}

fn revive_container(value: &Value, reviver: &Value) -> Result<Value, VmError> {
    if is_json_array(value)? {
        return revive_array_elements(value, reviver);
    }
    revive_object_properties(value, reviver)
}

fn revive_array_elements(value: &Value, reviver: &Value) -> Result<Value, VmError> {
    let length = crate::execute::get_property_result(value, "length")?;
    let length = to_length(&length)?;
    let mut current = value.clone();
    for index in 0..length {
        current = crate::locals::resolved_replacement(current);
        let key = index.to_string();
        let element = walk(current.clone(), &key, reviver)?;
        current = apply(current, &key, element)?;
    }
    Ok(current)
}

fn revive_object_properties(value: &Value, reviver: &Value) -> Result<Value, VmError> {
    let keys = enumerable_keys(value)?;
    let mut current = value.clone();
    for key in keys {
        current = crate::locals::resolved_replacement(current);
        let element = walk(current.clone(), &key, reviver)?;
        current = apply(current, &key, element)?;
    }
    Ok(current)
}

fn apply(holder: Value, key: &str, element: Value) -> Result<Value, VmError> {
    let holder = crate::locals::resolved_replacement(holder);
    let element = crate::locals::resolved_replacement(resolve_alias(element));
    let updated = if matches!(element, Value::Undefined) {
        delete_json_property(holder.clone(), key)?
    } else {
        create_json_property(holder.clone(), key, element)?
    };
    if matches!(holder, Value::Object(_) | Value::Array(_)) {
        crate::locals::replace_value(&holder, &updated);
    }
    Ok(updated)
}

fn delete_json_property(holder: Value, key: &str) -> Result<Value, VmError> {
    if matches!(holder, Value::Proxy(_)) {
        crate::proxy::proxy_delete(&holder, key)?;
        return Ok(holder);
    }
    Ok(crate::builtins::delete_property(holder, key).0)
}

fn create_json_property(holder: Value, key: &str, element: Value) -> Result<Value, VmError> {
    if matches!(holder, Value::Proxy(_)) {
        crate::proxy::proxy_define_property(&holder, key, &data_descriptor(element))?;
        return Ok(holder);
    }
    if crate::builtins::descriptor_flag(&holder, key, "configurable") == Some(false) {
        return Ok(holder);
    }
    Ok(crate::builtins::set_property(holder, key, element))
}

fn data_descriptor(value: Value) -> Value {
    Value::Object(Rc::new(crate::value::ObjectData::new(vec![
        ("value".to_string(), value),
        ("writable".to_string(), Value::Boolean(true)),
        ("enumerable".to_string(), Value::Boolean(true)),
        ("configurable".to_string(), Value::Boolean(true)),
    ])))
}

fn context_object(holder: &Value, key: &str, value: &Value) -> Value {
    let source = primitive_source(holder, key, value);
    let properties = source
        .map(|source| vec![("source".to_string(), Value::String(source))])
        .unwrap_or_default();
    Value::Object(Rc::new(crate::value::ObjectData::new(properties)))
}

fn primitive_source(holder: &Value, key: &str, value: &Value) -> Option<String> {
    if crate::value::is_object(value) {
        return None;
    }
    let holder = crate::locals::resolved_replacement(holder.clone());
    let stored = match &holder {
        Value::Object(properties) => properties
            .iter()
            .rev()
            .find(|(name, _)| name == &source_key(key))?
            .1
            .clone(),
        Value::Array(values) => values.property(&source_key(key))?,
        _ => return None,
    };
    let Value::Array(pair) = stored else {
        return None;
    };
    if pair.len() != 2 || !crate::builtins::same_value_zero(value, &pair[0]) {
        return None;
    }
    match &pair[1] {
        Value::String(source) => Some(source.clone()),
        _ => None,
    }
}
