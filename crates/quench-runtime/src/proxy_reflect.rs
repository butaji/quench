fn reflect_define_property(arguments: &[Value]) -> Result<Value, VmError> {
    let target = arguments.first().ok_or(VmError::NotCallable)?;
    let prop = arguments.get(1).map(value_to_string).unwrap_or_default();
    let descriptor = arguments.get(2).cloned().unwrap_or(Value::Undefined);
    proxy_define_property(target, &prop, &descriptor)?;
    Ok(Value::Boolean(true))
}

fn reflect_own_keys(arguments: &[Value]) -> Result<Value, VmError> {
    proxy_own_keys(arguments.first().ok_or(VmError::NotCallable)?)
}

fn extract_array_arg(arguments: &[Value], index: usize) -> Vec<Value> {
    arguments.get(index).and_then(|value| match value {
        Value::Array(array) => Some(array.to_vec()),
        _ => None,
    }).unwrap_or_default()
}

fn reflect_apply(arguments: &[Value]) -> Result<Value, VmError> {
    let target = arguments.first().ok_or(VmError::NotCallable)?;
    let this_arg = arguments.get(1).cloned().unwrap_or(Value::Undefined);
    proxy_apply(target, &this_arg, &extract_array_arg(arguments, 2))
}

fn reflect_construct(arguments: &[Value]) -> Result<Value, VmError> {
    let target = arguments.first().ok_or(VmError::NotCallable)?;
    proxy_construct(target, &extract_array_arg(arguments, 1), arguments.get(2))
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(), Value::Number(value) => value.to_string(),
        Value::Boolean(value) => value.to_string(), Value::Null => "null".to_string(),
        Value::Undefined => "undefined".to_string(), _ => "[object Object]".to_string(),
    }
}
