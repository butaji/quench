fn object_keys(target: Option<&Value>) -> Result<Value, VmError> {
    let Some(target @ Value::Proxy(_)) = target else {
        return crate::own_keys::keys_result(target);
    };
    let keys = proxy_enumerable_string_keys(target)?;
    Ok(Value::array(
        keys.into_iter().map(Value::String).collect(),
    ))
}

fn object_values_entries(target: Option<&Value>, entries: bool) -> Result<Value, VmError> {
    let Some(target @ Value::Proxy(_)) = target else {
        return crate::own_keys::values(target, entries);
    };
    let mut result = Vec::new();
    for key in proxy_own_string_keys(target)? {
        let descriptor = crate::proxy::proxy_get_own_property_descriptor(target, &key)?;
        let enumerable = crate::execute::get_property_result(&descriptor, "enumerable")?;
        if !crate::execute::is_truthy(&enumerable) {
            continue;
        }
        let value = crate::proxy::proxy_get(target, &key, None)?;
        result.push(if entries {
            Value::array(vec![Value::String(key), value])
        } else {
            value
        });
    }
    Ok(Value::array(result))
}

fn proxy_enumerable_string_keys(target: &Value) -> Result<Vec<String>, VmError> {
    let mut result = Vec::new();
    for key in proxy_own_string_keys(target)? {
        let descriptor = crate::proxy::proxy_get_own_property_descriptor(target, &key)?;
        let enumerable = crate::execute::get_property_result(&descriptor, "enumerable")?;
        if crate::execute::is_truthy(&enumerable) {
            result.push(key);
        }
    }
    Ok(result)
}

fn proxy_own_string_keys(target: &Value) -> Result<Vec<String>, VmError> {
    let keys = crate::proxy::proxy_own_keys(target)?;
    let Value::Array(keys) = keys else {
        return Err(crate::vm::not_callable());
    };
    let mut result = Vec::new();
    for key in keys.snapshot() {
        let Value::String(key) = key else {
            return Err(crate::value::error::throw_type_error(
                "Proxy ownKeys must return property keys",
            ));
        };
        if !key.contains('\0') {
            result.push(key);
        }
    }
    Ok(result)
}

fn object_proxy_names(target: Option<&Value>, symbols: bool) -> Result<Value, VmError> {
    let Some(Value::Proxy(_)) = target else {
        return if symbols {
            crate::own_keys::symbols(target)
        } else {
            crate::own_keys::names(target)
        };
    };
    let keys = crate::proxy::proxy_own_keys(target.ok_or(crate::vm::not_callable())?)?;
    let Value::Array(keys) = keys else {
        return Err(crate::vm::not_callable());
    };
    let keys = keys
        .snapshot()
        .into_iter()
        .filter(|value| matches!(value, Value::String(key) if key.contains('\0') == symbols))
        .collect();
    Ok(Value::array(keys))
}
