fn object_keys(target: Option<&Value>) -> Result<Value, VmError> {
    let Some(target @ Value::Proxy(_)) = target else {
        return Ok(crate::own_keys::enumerable_names(target));
    };
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
        if key.contains('\0') {
            continue;
        }
        let descriptor = crate::proxy::proxy_get_own_property_descriptor(target, &key)?;
        let enumerable = crate::execute::get_property_result(&descriptor, "enumerable")?;
        if crate::execute::is_truthy(&enumerable) {
            result.push(Value::String(key));
        }
    }
    Ok(Value::array(result))
}
