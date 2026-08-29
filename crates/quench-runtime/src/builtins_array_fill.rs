pub(crate) fn array_fill(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let receiver = receiver.unwrap_or(&Value::Undefined);
    let mut object = crate::construct::to_object(receiver)?;
    let length = to_length(&crate::execute::get_property_result(&object, "length")?)?;
    let start = fill_index(arguments.get(1), length, 0)?;
    let end = fill_index(
        arguments
            .get(2)
            .filter(|value| !matches!(value, Value::Undefined)),
        length,
        length,
    )?;
    let value = arguments.first().cloned().unwrap_or(Value::Undefined);
    if let (Value::Array(data), Value::Number(number)) = (&object, &value) {
        if data.can_fast_fill() {
            let original = std::rc::Rc::clone(data);
            let mut updated = data.as_ref().clone();
            updated.fill_numeric_constant_range(start, end, *number);
            let result = Value::Array(std::rc::Rc::new(updated));
            crate::locals::replace_value(&Value::Array(original), &result);
            return Ok(result);
        }
    }
    for index in start..end {
        let updated = crate::properties::assign_set_property(
            &object,
            &index.to_string(),
            value.clone(),
        )?;
        crate::locals::replace_value(&object, &updated);
        object = updated;
    }
    Ok(object)
}

fn to_length(value: &Value) -> Result<usize, crate::execute::VmError> {
    let number = crate::conversion::to_number(value)?;
    if number.is_nan() || number <= 0.0 {
        return Ok(0);
    }
    const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
    Ok(number.floor().min(MAX_SAFE_INTEGER) as usize)
}

fn fill_index(
    value: Option<&Value>,
    length: usize,
    default: usize,
) -> Result<usize, crate::execute::VmError> {
    let Some(value) = value else {
        return Ok(default);
    };
    let number = crate::conversion::to_number(value)?;
    if number.is_nan() || number == 0.0 || number == f64::NEG_INFINITY {
        return Ok(0);
    }
    if number == f64::INFINITY {
        return Ok(length);
    }
    if number.is_sign_negative() {
        return Ok(length.saturating_sub(number.abs().trunc() as usize));
    }
    Ok((number.trunc() as usize).min(length))
}
