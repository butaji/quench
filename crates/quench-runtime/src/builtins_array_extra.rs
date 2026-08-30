pub(crate) fn array_with(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let receiver = receiver.filter(|value| !matches!(value, Value::Null | Value::Undefined)).ok_or_else(|| {
        crate::value::error::throw_type_error("Array.prototype.with called on null or undefined")
    })?;
    let length = crate::builtins::map_length(receiver)?;
    if length > u32::MAX as usize {
        return Err(crate::value::error::throw_range_error("Invalid array length"));
    }
    let number = arguments.first().map(crate::conversion::to_number).transpose()?.unwrap_or(0.0);
    let integer = if number.is_nan() { 0.0 } else { number.trunc() };
    let index = if integer < 0.0 { length as f64 + integer } else { integer };
    if index < 0.0 || index as usize >= length {
        return Err(crate::value::error::throw_range_error("Invalid index"));
    }
    let mut result = Vec::with_capacity(length);
    for current in 0..length {
        result.push(if current == index as usize {
            arguments.get(1).cloned().unwrap_or(Value::Undefined)
        } else {
            crate::execute::get_property_result(receiver, &current.to_string())?
        });
    }
    Ok(Value::array(result))
}

pub(crate) fn array_to_spliced(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let receiver = receiver.filter(|value| !matches!(value, Value::Null | Value::Undefined)).ok_or_else(|| {
        crate::value::error::throw_type_error("Array.prototype.toSpliced called on null or undefined")
    })?;
    let length = crate::builtins::map_length(receiver)?;
    let number = arguments.first().map(crate::conversion::to_number).transpose()?.unwrap_or(0.0);
    let number = if number.is_nan() { 0.0 } else { number.trunc() };
    let start = if number < 0.0 { (length as f64 + number).max(0.0) as usize } else { (number as usize).min(length) };
    let delete_count = if arguments.is_empty() { 0 } else if arguments.len() == 1 { length - start } else {
        crate::conversion::to_number(&arguments[1])?.max(0.0).trunc() as usize
    }.min(length - start);
    let new_length = length.saturating_sub(delete_count).saturating_add(arguments.len().saturating_sub(2));
    if (new_length as u64) > 9_007_199_254_740_991u64 {
        return Err(crate::value::error::throw_type_error("Array length exceeds maximum safe integer"));
    }
    if new_length > u32::MAX as usize {
        return Err(crate::value::error::throw_range_error("Invalid array length"));
    }
    let mut result = Vec::with_capacity(new_length);
    for index in 0..start { result.push(crate::execute::get_property_result(receiver, &index.to_string())?); }
    result.extend(arguments.iter().skip(2).cloned());
    for index in start + delete_count..length { result.push(crate::execute::get_property_result(receiver, &index.to_string())?); }
    Ok(Value::array(result))
}
