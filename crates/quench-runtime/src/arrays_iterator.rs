fn array_iterator(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    let Some(Value::Array(values)) = receiver else {
        let Some(value) = receiver else {
            return Err(crate::value::error::throw_type_error(
                "Array iterator called on incompatible receiver",
            ));
        };
        return array_like_iterator(value);
    };
    Ok(crate::collections::iterator::make(values.snapshot()))
}

fn array_like_iterator(value: &Value) -> Result<Value, crate::execute::VmError> {
    if matches!(value, Value::Null | Value::Undefined) {
        return Err(crate::value::error::throw_type_error(
            "Array iterator called on incompatible receiver",
        ));
    }
    let length =
        crate::conversion::to_number(&crate::execute::get_property_result(value, "length")?)?;
    let length = if length.is_finite() && length > 0.0 {
        length.floor().min(usize::MAX as f64) as usize
    } else {
        0
    };
    let mut values = Vec::with_capacity(length.min(1024));
    for index in 0..length {
        values.push(crate::execute::get_property_result(
            value,
            &index.to_string(),
        )?);
    }
    Ok(crate::collections::iterator::make(values))
}
