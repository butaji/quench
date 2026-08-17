pub(crate) fn some(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let receiver = expect_array_like(receiver, "Array.prototype.some")?;
    let values = array_indexed_values(&receiver);
    let Some(callback) = arguments.first() else {
        return Ok(Value::Boolean(false));
    };
    let this_arg = arguments.get(1).map_or(&Value::Undefined, |value| value);
    for (index, value) in values.iter().enumerate() {
        let args = [
            value.clone(),
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
    let values = array_indexed_values(&receiver);
    let Some(callback) = arguments.first() else {
        return Ok(Value::Boolean(true));
    };
    let this_arg = arguments.get(1).map_or(&Value::Undefined, |value| value);
    for (index, value) in values.iter().enumerate() {
        let args = [
            value.clone(),
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
    let values = array_indexed_values(&receiver);
    let Some(callback) = arguments.first() else {
        return Ok(Value::Undefined);
    };
    let this_arg = arguments.get(1).map_or(&Value::Undefined, |value| value);
    for (index, value) in values.iter().enumerate() {
        let args = [
            value.clone(),
            Value::Number(index as f64),
            receiver.clone(),
        ];
        let result = crate::functions::execute_target(callback, this_arg, &args)?;
        if crate::execute::is_truthy(&result) {
            return Ok(value.clone());
        }
    }
    Ok(Value::Undefined)
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

fn array_indexed_values(receiver: &Value) -> Vec<Value> {
    if let Value::Array(values) = receiver {
        return values.iter().cloned().collect();
    }
    let length = crate::execute::get_property_result(receiver, "length")
        .ok()
        .and_then(|v| {
            if let Value::Number(n) = v {
                // Cap materialization to a sane upper bound to avoid
                // OOM/hang on pathological `length` values like 2^32+1.
                let clamped = if n.is_nan() || n < 0.0 {
                    0.0
                } else if n > 1_048_576.0 {
                    1_048_576.0
                } else {
                    n
                };
                Some(clamped.trunc() as usize)
            } else {
                None
            }
        })
        .unwrap_or(0);
    let mut out = Vec::with_capacity(length);
    for index in 0..length {
        let value = crate::execute::get_property_result(receiver, &index.to_string())
            .unwrap_or(Value::Undefined);
        out.push(value);
    }
    out
}
