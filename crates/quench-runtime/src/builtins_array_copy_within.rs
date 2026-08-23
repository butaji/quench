pub(crate) fn array_copy_within(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let Some(receiver @ Value::Array(values)) = receiver else {
        return Ok(Value::Undefined);
    };
    let mut updated = values.to_vec();
    let length = updated.len();
    let target = copy_index(arguments.first(), length)?;
    let start = copy_index(arguments.get(1), length)?;
    let end = match arguments.get(2) {
        None | Some(Value::Undefined) => length,
        Some(value) => copy_index(Some(value), length)?,
    };
    let count = end.saturating_sub(start).min(length.saturating_sub(target));
    let source = updated[start..start + count].to_vec();
    updated[target..target + count].clone_from_slice(&source);
    let result = Value::array(updated);
    crate::locals::replace_value(receiver, &result);
    Ok(result)
}

fn copy_index(value: Option<&Value>, length: usize) -> Result<usize, crate::execute::VmError> {
    let Some(value) = value else {
        return Ok(0);
    };
    let number = crate::conversion::to_number(value)?;
    if number.is_nan() {
        return Ok(0);
    }
    if number.is_sign_negative() {
        return Ok(length.saturating_sub(number.abs().floor() as usize));
    }
    Ok((number.floor() as usize).min(length))
}
