pub(crate) fn array(arguments: &[Value]) -> Value {
    if let [Value::Number(length)] = arguments {
        if *length >= 0.0 && length.fract() == 0.0 && *length <= u32::MAX as f64 {
            let mut values = Value::array(Vec::new());
            if let Value::Array(values) = &mut values {
                Rc::make_mut(values).set_length(*length as usize);
            }
            return values;
        }
    }
    Value::array(arguments.to_vec())
}

pub(crate) fn array_map(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let Some(receiver) = receiver else {
        return Ok(Value::array(Vec::new()));
    };
    let Some(callback) = arguments.first() else {
        return Ok(Value::array(Vec::new()));
    };
    let length = map_length(receiver)?;
    if length > u32::MAX as usize {
        return Err(crate::value::error::throw_range_error(
            "Invalid array length",
        ));
    }
    let mut mapped = Value::array(Vec::new());
    if let Value::Array(values) = &mut mapped {
        Rc::make_mut(values).set_length(length);
    }
    for index in 0..length {
        let Some(value) = map_value(receiver, index)? else {
            continue;
        };
        let args = [value, Value::Number(index as f64), receiver.clone()];
        let this_arg = arguments.get(1).map_or(&Value::Undefined, |value| value);
        let result = crate::functions::execute_target(callback, this_arg, &args)?;
        if let Value::Array(values) = &mut mapped {
            Rc::make_mut(values).set_index(index, result);
        }
    }
    Ok(mapped)
}
fn map_length(receiver: &Value) -> Result<usize, crate::execute::VmError> {
    if let Value::Array(values) = receiver {
        return Ok(values.logical_len());
    }
    let length = crate::execute::get_property_result(receiver, "length")?;
    let number = crate::conversion::to_number(&length)?;
    if number.is_nan() || number <= 0.0 {
        return Ok(0);
    }
    Ok(number.floor().min(9_007_199_254_740_991.0) as usize)
}
fn map_value(receiver: &Value, index: usize) -> Result<Option<Value>, crate::execute::VmError> {
    if let Value::Array(values) = receiver {
        return Ok(values
            .has_index(index)
            .then(|| values.get_index(index))
            .flatten());
    }
    let key = index.to_string();
    if !crate::with_scope::has_property(receiver, &key)? {
        return Ok(None);
    }
    crate::execute::get_property_result(receiver, &key).map(Some)
}
pub(crate) fn array_for_each(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let Some(Value::Array(values)) = receiver else {
        return Ok(Value::Undefined);
    };
    if let Some(callback) = arguments.first() {
        let this_arg = arguments.get(1).map_or(&Value::Undefined, |value| value);
        for (index, value) in values.iter().enumerate() {
            crate::functions::execute_target(
                callback,
                this_arg,
                &[value.clone(), Value::Number(index as f64), Value::Undefined],
            )?;
        }
    }
    Ok(Value::Undefined)
}
pub(crate) fn array_filter(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let Some(Value::Array(values)) = receiver else {
        return Ok(Value::array(Vec::new()));
    };
    let Some(callback) = arguments.first() else {
        return Ok(Value::Array(values.clone()));
    };
    let mut filtered = Vec::new();
    let this_arg = arguments.get(1).map_or(&Value::Undefined, |value| value);
    for (index, value) in values.iter().enumerate() {
        let args = [value.clone(), Value::Number(index as f64), Value::Undefined];
        let result = crate::functions::execute_target(callback, this_arg, &args)?;
        if crate::execute::is_truthy(&result) {
            filtered.push(value.clone());
        }
    }
    Ok(Value::array(filtered))
}

pub(crate) fn array_join(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Some(Value::Array(values)) = receiver else {
        return Value::String(String::new());
    };
    let separator = arguments
        .first()
        .map_or_else(|| ",".to_string(), value_to_string);
    Value::String(
        values
            .iter()
            .map(value_to_string)
            .collect::<Vec<_>>()
            .join(&separator),
    )
}

pub(crate) fn array_push(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Some(receiver @ Value::Array(values)) = receiver else {
        return Value::Number(f64::NAN);
    };
    let mut result = values.to_vec();
    result.extend_from_slice(arguments);
    let length = result.len();
    crate::locals::replace_value(receiver, &Value::array(result));
    Value::Number(length as f64)
}
