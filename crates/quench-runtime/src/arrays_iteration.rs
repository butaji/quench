pub(crate) fn some(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let receiver = expect_array_like(receiver, "Array.prototype.some")?;
    let callback = arguments.first().ok_or_else(crate::vm::not_callable)?;
    if !crate::conversion::is_callable(callback) {
        return Err(crate::vm::not_callable());
    }
    let length = crate::builtins::map_length(&receiver)?;
    let this_arg = arguments.get(1).map_or(&Value::Undefined, |value| value);
    for index in 0..length {
        let Some(value) = crate::builtins::map_value(&receiver, index)? else {
            continue;
        };
        let args = [
            value,
            Value::Number(index as f64),
            receiver.clone(),
        ];
        let result = crate::functions::execute_target(callback, this_arg, &args)?;
        if crate::execute::is_truthy(&result) {
            return Ok(Value::Boolean(true));
        }
    }
    Ok(Value::Boolean(false))
}

pub(crate) fn every(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let receiver = expect_array_like(receiver, "Array.prototype.every")?;
    let callback = arguments.first().ok_or_else(crate::vm::not_callable)?;
    if !crate::conversion::is_callable(callback) {
        return Err(crate::vm::not_callable());
    }
    let length = crate::builtins::map_length(&receiver)?;
    let this_arg = arguments.get(1).map_or(&Value::Undefined, |value| value);
    for index in 0..length {
        let Some(value) = crate::builtins::map_value(&receiver, index)? else {
            continue;
        };
        let args = [
            value,
            Value::Number(index as f64),
            receiver.clone(),
        ];
        let result = crate::functions::execute_target(callback, this_arg, &args)?;
        if !crate::execute::is_truthy(&result) {
            return Ok(Value::Boolean(false));
        }
    }
    Ok(Value::Boolean(true))
}

pub(crate) fn find(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let receiver = expect_array_like(receiver, "Array.prototype.find")?;
    let callback = arguments.first().ok_or_else(crate::vm::not_callable)?;
    if !crate::conversion::is_callable(callback) {
        return Err(crate::vm::not_callable());
    }
    let length = crate::builtins::map_length(&receiver)?;
    let this_arg = arguments.get(1).map_or(&Value::Undefined, |value| value);
    for index in 0..length {
        let Some(value) = crate::builtins::map_value(&receiver, index)? else {
            continue;
        };
        let args = [
            value.clone(),
            Value::Number(index as f64),
            receiver.clone(),
        ];
        let result = crate::functions::execute_target(callback, this_arg, &args)?;
        if crate::execute::is_truthy(&result) {
            return Ok(value);
        }
    }
    Ok(Value::Undefined)
}

pub(crate) fn find_index(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let receiver = expect_array_like(receiver, "Array.prototype.findIndex")?;
    let callback = arguments.first().ok_or_else(crate::vm::not_callable)?;
    if !crate::conversion::is_callable(callback) {
        return Err(crate::vm::not_callable());
    }
    let length = crate::builtins::map_length(&receiver)?;
    let this_arg = arguments.get(1).map_or(&Value::Undefined, |value| value);
    for index in 0..length {
        let Some(value) = crate::builtins::map_value(&receiver, index)? else {
            continue;
        };
        let args = [value, Value::Number(index as f64), receiver.clone()];
        let result = crate::functions::execute_target(callback, this_arg, &args)?;
        if crate::execute::is_truthy(&result) {
            return Ok(Value::Number(index as f64));
        }
    }
    Ok(Value::Number(-1.0))
}

fn expect_array_like(
    receiver: Option<&Value>,
    method: &str,
) -> Result<Value, crate::execute::VmError> {
    let Some(receiver) = receiver else {
        return Err(crate::value::error::throw_type_error(
            format!("{method} called on null or undefined").as_str(),
        ));
    };
    if matches!(receiver, Value::Null | Value::Undefined) {
        return Err(crate::value::error::throw_type_error(
            format!("{method} called on null or undefined").as_str(),
        ));
    }
    Ok(receiver.clone())
}
