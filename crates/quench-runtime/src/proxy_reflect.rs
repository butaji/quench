fn reflect_define_property(arguments: &[Value]) -> Result<Value, VmError> {
    let target = reflect_target(arguments)?;
    let prop = reflect_property(arguments)?;
    let descriptor = arguments.get(2).cloned().unwrap_or(Value::Undefined);
    proxy_define_property(target, &prop, &descriptor)?;
    Ok(Value::Boolean(true))
}

fn reflect_own_keys(arguments: &[Value]) -> Result<Value, VmError> {
    proxy_own_keys(arguments.first().ok_or(VmError::NotCallable)?)
}

fn extract_array_arg(arguments: &[Value], index: usize) -> Result<Vec<Value>, VmError> {
    let value = arguments.get(index).ok_or_else(|| {
        crate::value::error::throw_type_error("Reflect arguments list must be an object")
    })?;
    if !crate::value::is_object(value) {
        return Err(crate::value::error::throw_type_error(
            "Reflect arguments list must be an object",
        ));
    }
    crate::vm::create_list_from_array_like(Some(value))
}

fn reflect_apply(arguments: &[Value]) -> Result<Value, VmError> {
    let target = arguments.first().ok_or(VmError::NotCallable)?;
    let this_arg = arguments.get(1).cloned().unwrap_or(Value::Undefined);
    proxy_apply(target, &this_arg, &extract_array_arg(arguments, 2)?)
}

fn reflect_construct(arguments: &[Value]) -> Result<Value, VmError> {
    let target = arguments.first().ok_or(VmError::NotCallable)?;
    proxy_construct(target, &extract_array_arg(arguments, 1)?, arguments.get(2))
}
