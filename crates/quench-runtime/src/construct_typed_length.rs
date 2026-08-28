fn object_array_like(
    properties: &crate::value::ObjectData,
) -> Result<Option<Vec<Value>>, crate::execute::VmError> {
    let object = Value::Object(Rc::new(properties.clone()));
    if let Ok(values) = crate::collections::iterator::collect_iterable(object.clone()) {
        return Ok(Some(values));
    }
    let length = crate::execute::get_property_result(&object, "length")?;
    if matches!(length, Value::Undefined) {
        return Ok(Some(Vec::new()));
    }
    let length = crate::conversion::to_number(&length)?;
    let length = if !length.is_finite() || length <= 0.0 {
        if length.is_infinite() && length.is_sign_positive() {
            usize::MAX
        } else {
            0
        }
    } else {
        length.floor().min(usize::MAX as f64) as usize
    };
    let mut values = Vec::new();
    values
        .try_reserve_exact(length)
        .map_err(|_| range_error("Typed-array length is too large"))?;
    for index in 0..length {
        values.push(crate::execute::get_property_result(&object, &index.to_string())?);
    }
    Ok(Some(values))
}
fn alloc_buffer(
    length: usize,
    element_size: usize,
) -> Result<Rc<crate::value::ArrayBufferData>, crate::execute::VmError> {
    length
        .checked_mul(element_size)
        .and_then(crate::value::ArrayBufferData::try_new)
        .map(Rc::new)
        .ok_or_else(|| range_error(&format!("Invalid typed array length: {length}")))
}

fn typed_view_bounds(
    buffer: &crate::value::ArrayBufferData,
    arguments: &[Value],
    element_size: usize,
    name: &str,
) -> Result<(usize, usize), crate::execute::VmError> {
    if *buffer.detached.borrow() {
        return Err(crate::value::error::throw_type_error(
            "Cannot use a detached ArrayBuffer",
        ));
    }
    let offset = arguments
        .get(1)
        .map(crate::conversion::to_number)
        .transpose()?
        .unwrap_or(0.0);
    let offset = to_index(offset)?;
    if *buffer.detached.borrow() {
        return Err(crate::value::error::throw_type_error(
            "Cannot use a detached ArrayBuffer",
        ));
    }
    let available = buffer
        .byte_length()
        .checked_sub(offset)
        .ok_or_else(|| range_error(&format!("Invalid {name} byte offset")))?;
    if offset % element_size != 0 {
        return Err(range_error(&format!("Invalid {name} byte offset")));
    }
    let length = match arguments.get(2) {
        None | Some(Value::Undefined) => {
            if available % element_size != 0 {
                return Err(range_error(&format!("Invalid {name} byte length")));
            }
            view_length(buffer, available / element_size)
        }
        Some(value) => {
            let number = crate::conversion::to_number(value)?;
            if *buffer.detached.borrow() {
                return Err(crate::value::error::throw_type_error(
                    "Cannot use a detached ArrayBuffer",
                ));
            }
            to_index(number)?
        }
    };
    if arguments
        .get(2)
        .is_some_and(|value| !matches!(value, Value::Undefined))
        && length > available / element_size
    {
        return Err(range_error(&format!("Invalid {name} length")));
    }
    Ok((offset, length))
}
fn length_uint8_array(length: f64) -> Result<Value, crate::execute::VmError> {
    let length = to_index(length)?;
    let buffer = alloc_buffer(length, 1)?;
    Ok(Value::Uint8Array(Rc::new(
        crate::value::Uint8ArrayData::new(buffer, 0, length),
    )))
}
fn length_float64_array(length: f64) -> Result<Value, crate::execute::VmError> {
    let length = to_index(length)?;
    let buffer = alloc_buffer(length, 8)?;
    Ok(Value::Float64Array(Rc::new(
        crate::value::Float64ArrayData::new(buffer, 0, length),
    )))
}
fn length_float32_array(length: f64) -> Result<Value, crate::execute::VmError> {
    let length = to_index(length)?;
    let buffer = alloc_buffer(length, 4)?;
    Ok(Value::Float32Array(Rc::new(
        crate::value::Float32ArrayData::new(buffer, 0, length),
    )))
}
fn length_int8_array(length: f64) -> Result<Value, crate::execute::VmError> {
    let length = to_index(length)?;
    let buffer = alloc_buffer(length, 1)?;
    Ok(Value::Int8Array(Rc::new(crate::value::Int8ArrayData::new(
        buffer, 0, length,
    ))))
}
fn length_int16_array(length: f64) -> Result<Value, crate::execute::VmError> {
    let length = to_index(length)?;
    let buffer = alloc_buffer(length, 2)?;
    Ok(Value::Int16Array(Rc::new(
        crate::value::Int16ArrayData::new(buffer, 0, length),
    )))
}
fn length_int32_array(length: f64) -> Result<Value, crate::execute::VmError> {
    let length = to_index(length)?;
    let buffer = alloc_buffer(length, 4)?;
    Ok(Value::Int32Array(Rc::new(
        crate::value::Int32ArrayData::new(buffer, 0, length),
    )))
}
fn length_uint32_array(length: f64) -> Result<Value, crate::execute::VmError> {
    let length = to_index(length)?;
    let buffer = alloc_buffer(length, 4)?;
    Ok(Value::Uint32Array(Rc::new(
        crate::value::Uint32ArrayData::new(buffer, 0, length),
    )))
}
fn length_uint16_array(length: f64) -> Result<Value, crate::execute::VmError> {
    let length = to_index(length)?;
    let buffer = alloc_buffer(length, 2)?;
    Ok(Value::Uint16Array(Rc::new(
        crate::value::Uint16ArrayData::new(buffer, 0, length),
    )))
}
fn length_uint8_clamped_array(length: f64) -> Result<Value, crate::execute::VmError> {
    let length = to_index(length)?;
    let buffer = alloc_buffer(length, 1)?;
    Ok(Value::Uint8ClampedArray(Rc::new(
        crate::value::Uint8ClampedArrayData::new(buffer, 0, length),
    )))
}
