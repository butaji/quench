/// `ToObject(this)`: nullish receivers of the search methods throw.
fn search_receiver(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    let this = receiver.cloned().unwrap_or(Value::Undefined);
    if matches!(this, Value::Null | Value::Undefined) {
        return Err(crate::value::error::throw_type_error(
            "Array search method called on null or undefined",
        ));
    }
    Ok(this)
}

/// `ToIntegerOrInfinity` of an optional fromIndex, clamped for indexOf.
fn search_from_index(
    value: Option<&Value>,
    length: isize,
) -> Result<isize, crate::execute::VmError> {
    let Some(value) = value else {
        return Ok(0);
    };
    let number = crate::conversion::to_number(value)?;
    if number.is_nan() {
        return Ok(0);
    }
    if number.is_infinite() {
        return Ok(if number < 0.0 { 0 } else { length });
    }
    let integer = number.trunc() as isize;
    Ok(if integer < 0 {
        (length + integer).max(0)
    } else {
        integer
    })
}

pub(crate) fn includes(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let this = search_receiver(receiver)?;
    let search = arguments.first().unwrap_or(&Value::Undefined);
    let length = slice_length(&this)?;
    let this = crate::locals::resolved_replacement(this);
    if length == 0 {
        return Ok(Value::Boolean(false));
    }
    let start = search_from_index(arguments.get(1), length)?;
    for index in start..length.min(i32::MAX as isize) {
        // A getter may have replaced the receiver (copy-on-write) mid-search.
        let current = crate::locals::resolved_replacement(this.clone());
        let element = crate::execute::get_property_result(&current, &index.to_string())?;
        if same_value_zero(&element, search) {
            return Ok(Value::Boolean(true));
        }
    }
    Ok(Value::Boolean(false))
}

pub(crate) fn index_of(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let this = search_receiver(receiver)?;
    let length = slice_length(&this)?;
    if length == 0 {
        return Ok(Value::Number(-1.0));
    }
    // The length getter may have replaced the receiver (copy-on-write).
    let this = crate::locals::resolved_replacement(this);
    let search = arguments.first().unwrap_or(&Value::Undefined);
    let start = search_from_index(arguments.get(1), length)?;
    for index in start..length.min(i32::MAX as isize) {
        let key = index.to_string();
        // A getter may have replaced the receiver (copy-on-write) mid-search.
        let current = crate::locals::resolved_replacement(this.clone());
        if crate::with_scope::has_property(&current, &key)?
            && strict_equal(&crate::execute::get_property_result(&current, &key)?, search)
        {
            return Ok(Value::Number(index as f64));
        }
    }
    Ok(Value::Number(-1.0))
}

pub(crate) fn last_index_of(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let this = search_receiver(receiver)?;
    let length = slice_length(&this)?;
    if length == 0 {
        return Ok(Value::Number(-1.0));
    }
    // The length getter may have replaced the receiver (copy-on-write).
    let this = crate::locals::resolved_replacement(this);
    let search = arguments.first().unwrap_or(&Value::Undefined);
    let start = last_search_from_index(arguments.get(1), length)?;
    for index in (0..=start).rev() {
        let key = index.to_string();
        // A getter may have replaced the receiver (copy-on-write) mid-search.
        let current = crate::locals::resolved_replacement(this.clone());
        if crate::with_scope::has_property(&current, &key)?
            && strict_equal(&crate::execute::get_property_result(&current, &key)?, search)
        {
            return Ok(Value::Number(index as f64));
        }
    }
    Ok(Value::Number(-1.0))
}

/// `ToIntegerOrInfinity` of an optional fromIndex, clamped for lastIndexOf.
fn last_search_from_index(
    value: Option<&Value>,
    length: isize,
) -> Result<isize, crate::execute::VmError> {
    let Some(value) = value else {
        return Ok(length - 1);
    };
    let number = crate::conversion::to_number(value)?;
    if number.is_nan() {
        return Ok(0);
    }
    if number.is_infinite() {
        return Ok(if number < 0.0 { -1 } else { length - 1 });
    }
    let integer = number.trunc() as isize;
    Ok(if integer < 0 {
        length + integer
    } else {
        integer.min(length - 1)
    })
}
