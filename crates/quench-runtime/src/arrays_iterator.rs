fn array_iterator(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    let Some(value) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "Array iterator called on incompatible receiver",
        ));
    };
    Ok(crate::collections::iterator::make_array_like(value.clone()))
}

fn array_keys(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    let values = array_iterator_values(receiver)?;
    Ok(crate::collections::iterator::make(
        (0..values.len())
            .map(|index| Value::Number(index as f64))
            .collect(),
    ))
}

fn array_entries(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    let values = array_iterator_values(receiver)?;
    Ok(crate::collections::iterator::make(
        values
            .into_iter()
            .enumerate()
            .map(|(index, value)| Value::array(vec![Value::Number(index as f64), value]))
            .collect(),
    ))
}

fn array_iterator_values(receiver: Option<&Value>) -> Result<Vec<Value>, crate::execute::VmError> {
    if let Some(Value::Array(values)) = receiver {
        return Ok(values.snapshot());
    }
    let Some(value) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "Array iterator called on incompatible receiver",
        ));
    };
    if let Value::StringUnits(units) = value {
        return Ok(String::from_utf16_lossy(units)
            .chars()
            .map(|character| Value::String(character.to_string()))
            .collect());
    }
    if let Value::String(string) = value {
        return Ok(string
            .chars()
            .map(|character| Value::String(character.to_string()))
            .collect());
    }
    array_like_values(value)
}

fn array_like_values(value: &Value) -> Result<Vec<Value>, crate::execute::VmError> {
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
    Ok(values)
}
