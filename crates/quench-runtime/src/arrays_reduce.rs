pub(crate) fn reduce_values(
    receiver: Option<&Value>,
    arguments: &[Value],
    reverse: bool,
) -> Result<Value, crate::execute::VmError> {
    let Some(receiver) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "Array.prototype.reduce called on null or undefined",
        ));
    };
    if matches!(receiver, Value::Null | Value::Undefined) {
        return Err(crate::value::error::throw_type_error(
            "Array.prototype.reduce called on null or undefined",
        ));
    }
    let values = reduce_input_values(receiver);
    reduce_accumulate(receiver, arguments, reverse, values)
}

fn reduce_input_values(receiver: &Value) -> Vec<Value> {
    if let Value::Array(values) = receiver {
        return values.iter().cloned().collect();
    }
    let length = crate::execute::get_property_result(receiver, "length")
        .ok()
        .and_then(|v| match v {
            Value::Number(n) => {
                let clamped = if n.is_nan() || n < 0.0 {
                    0.0
                } else if n > 1_048_576.0 {
                    1_048_576.0
                } else {
                    n
                };
                Some(clamped.trunc() as usize)
            }
            _ => None,
        })
        .unwrap_or(0);
    (0..length)
        .map(|i| {
            crate::execute::get_property_result(receiver, &i.to_string())
                .unwrap_or(Value::Undefined)
        })
        .collect()
}

fn reduce_accumulate(
    receiver: &Value,
    arguments: &[Value],
    reverse: bool,
    values: Vec<Value>,
) -> Result<Value, crate::execute::VmError> {
    let Some(callback) = arguments.first() else {
        return Ok(Value::Undefined);
    };
    let indices: Vec<usize> = if reverse {
        (0..values.len()).rev().collect()
    } else {
        (0..values.len()).collect()
    };
    if indices.is_empty() && arguments.get(1).is_none() {
        return Ok(Value::Undefined);
    }
    let (mut accumulator, start) = match arguments.get(1) {
        Some(value) => (value.clone(), 0),
        None => (values[indices.first().copied().unwrap_or(0)].clone(), 1),
    };
    for index in indices.into_iter().skip(start) {
        let args = [
            accumulator,
            values[index].clone(),
            Value::Number(index as f64),
            receiver.clone(),
        ];
        accumulator = crate::functions::execute_target(callback, receiver, &args)?;
    }
    Ok(accumulator)
}
