pub(crate) fn to_uint8(value: f64) -> u8 {
    if !value.is_finite() || value == 0.0 {
        return 0;
    }
    value.trunc().rem_euclid(256.0) as u8
}

fn empty_int8_array() -> Result<Value, crate::execute::VmError> {
    let buffer = Rc::new(crate::value::ArrayBufferData::new(0));
    Ok(Value::Int8Array(Rc::new(crate::value::Int8ArrayData::new(
        buffer, 0, 0,
    ))))
}

fn values_int8_array(values: &[Value]) -> Result<Value, crate::execute::VmError> {
    let buffer = Rc::new(crate::value::ArrayBufferData::new(values.len()));
    let view = crate::value::Int8ArrayData::new(buffer, 0, values.len());
    for (index, value) in values.iter().enumerate() {
        view.set(index, to_int8(number_for_typed_array(value)?));
    }
    Ok(Value::Int8Array(Rc::new(view)))
}

fn copy_int8_array(source: &crate::value::Int8ArrayData) -> Result<Value, crate::execute::VmError> {
    let length = source.logical_len();
    let buffer = Rc::new(crate::value::ArrayBufferData::new(source.byte_length()));
    let view = crate::value::Int8ArrayData::new(buffer, 0, length);
    for index in 0..length {
        view.set(index, source.get(index).unwrap_or(0));
    }
    Ok(Value::Int8Array(Rc::new(view)))
}

fn view_int8_array(
    buffer: &Rc<crate::value::ArrayBufferData>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let (offset, length) = typed_view_bounds(
        buffer,
        arguments,
        crate::value::Int8ArrayData::BYTES_PER_ELEMENT,
        "Int8Array",
    )?;
    Ok(Value::Int8Array(Rc::new(crate::value::Int8ArrayData::new(
        buffer.clone(),
        offset,
        length,
    ))))
}

fn empty_int32_array() -> Result<Value, crate::execute::VmError> {
    let buffer = Rc::new(crate::value::ArrayBufferData::new(0));
    Ok(Value::Int32Array(Rc::new(
        crate::value::Int32ArrayData::new(buffer, 0, 0),
    )))
}

fn empty_int16_array() -> Result<Value, crate::execute::VmError> {
    let buffer = Rc::new(crate::value::ArrayBufferData::new(0));
    Ok(Value::Int16Array(Rc::new(
        crate::value::Int16ArrayData::new(buffer, 0, 0),
    )))
}

fn values_int16_array(values: &[Value]) -> Result<Value, crate::execute::VmError> {
    let element_size = crate::value::Int16ArrayData::BYTES_PER_ELEMENT;
    let buffer = Rc::new(crate::value::ArrayBufferData::new(
        values.len() * element_size,
    ));
    let view = crate::value::Int16ArrayData::new(buffer, 0, values.len());
    for (index, value) in values.iter().enumerate() {
        view.set(index, to_int16(number_for_typed_array(value)?));
    }
    Ok(Value::Int16Array(Rc::new(view)))
}

fn copy_int16_array(
    source: &crate::value::Int16ArrayData,
) -> Result<Value, crate::execute::VmError> {
    let length = source.logical_len();
    let buffer = Rc::new(crate::value::ArrayBufferData::new(source.byte_length()));
    let view = crate::value::Int16ArrayData::new(buffer, 0, length);
    for index in 0..length {
        view.set(index, source.get(index).unwrap_or(0));
    }
    Ok(Value::Int16Array(Rc::new(view)))
}

fn view_int16_array(
    buffer: &Rc<crate::value::ArrayBufferData>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let element_size = crate::value::Int16ArrayData::BYTES_PER_ELEMENT;
    let (offset, length) = typed_view_bounds(buffer, arguments, element_size, "Int16Array")?;
    Ok(Value::Int16Array(Rc::new(
        crate::value::Int16ArrayData::new(buffer.clone(), offset, length),
    )))
}

fn values_int32_array(values: &[Value]) -> Result<Value, crate::execute::VmError> {
    let element_size = crate::value::Int32ArrayData::BYTES_PER_ELEMENT;
    let buffer = Rc::new(crate::value::ArrayBufferData::new(
        values.len() * element_size,
    ));
    let view = crate::value::Int32ArrayData::new(buffer, 0, values.len());
    for (index, value) in values.iter().enumerate() {
        view.set(index, to_int32(number_for_typed_array(value)?));
    }
    Ok(Value::Int32Array(Rc::new(view)))
}

fn copy_int32_array(
    source: &crate::value::Int32ArrayData,
) -> Result<Value, crate::execute::VmError> {
    let length = source.logical_len();
    let buffer = Rc::new(crate::value::ArrayBufferData::new(source.byte_length()));
    let view = crate::value::Int32ArrayData::new(buffer, 0, length);
    for index in 0..length {
        view.set(index, source.get(index).unwrap_or(0));
    }
    Ok(Value::Int32Array(Rc::new(view)))
}

fn view_int32_array(
    buffer: &Rc<crate::value::ArrayBufferData>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let element_size = crate::value::Int32ArrayData::BYTES_PER_ELEMENT;
    let (offset, length) = typed_view_bounds(buffer, arguments, element_size, "Int32Array")?;
    Ok(Value::Int32Array(Rc::new(
        crate::value::Int32ArrayData::new(buffer.clone(), offset, length),
    )))
}

pub(crate) fn to_int8(value: f64) -> i8 {
    if !value.is_finite() || value == 0.0 {
        return 0;
    }
    let modulo = value.trunc().rem_euclid(256.0);
    (if modulo >= 128.0 {
        modulo - 256.0
    } else {
        modulo
    }) as i8
}

pub(crate) fn to_int16(value: f64) -> i16 {
    if !value.is_finite() || value == 0.0 {
        return 0;
    }
    let modulo = value.trunc().rem_euclid(65_536.0);
    (if modulo >= 32_768.0 {
        modulo - 65_536.0
    } else {
        modulo
    }) as i16
}

pub(crate) fn to_int32(value: f64) -> i32 {
    if !value.is_finite() || value == 0.0 {
        return 0;
    }
    let modulo = value.trunc().rem_euclid(4_294_967_296.0);
    (if modulo >= 2_147_483_648.0 {
        modulo - 4_294_967_296.0
    } else {
        modulo
    }) as i32
}

pub(crate) fn to_uint32(value: f64) -> u32 {
    if !value.is_finite() || value == 0.0 {
        return 0;
    }
    value.trunc().rem_euclid(4_294_967_296.0) as u32
}

pub(crate) fn to_uint16(value: f64) -> u16 {
    if !value.is_finite() || value == 0.0 {
        return 0;
    }
    value.trunc().rem_euclid(65_536.0) as u16
}

fn empty_float32_array() -> Result<Value, crate::execute::VmError> {
    let buffer = Rc::new(crate::value::ArrayBufferData::new(0));
    Ok(Value::Float32Array(Rc::new(
        crate::value::Float32ArrayData::new(buffer, 0, 0),
    )))
}

fn values_float32_array(values: &[Value]) -> Result<Value, crate::execute::VmError> {
    let buffer = Rc::new(crate::value::ArrayBufferData::new(
        values.len() * crate::value::Float32ArrayData::BYTES_PER_ELEMENT,
    ));
    let view = crate::value::Float32ArrayData::new(buffer, 0, values.len());
    for (index, value) in values.iter().enumerate() {
        view.set(index, number_for_typed_array(value)? as f32);
    }
    Ok(Value::Float32Array(Rc::new(view)))
}

fn copy_float32_array(
    source: &crate::value::Float32ArrayData,
) -> Result<Value, crate::execute::VmError> {
    let length = source.logical_len();
    let buffer = Rc::new(crate::value::ArrayBufferData::new(source.byte_length()));
    let view = crate::value::Float32ArrayData::new(buffer, 0, length);
    for index in 0..length {
        view.set(index, source.get(index).unwrap_or(f32::NAN));
    }
    Ok(Value::Float32Array(Rc::new(view)))
}

fn view_float32_array(
    buffer: &Rc<crate::value::ArrayBufferData>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let element_size = crate::value::Float32ArrayData::BYTES_PER_ELEMENT;
    let (offset, length) = typed_view_bounds(buffer, arguments, element_size, "Float32Array")?;
    Ok(Value::Float32Array(Rc::new(
        crate::value::Float32ArrayData::new(buffer.clone(), offset, length),
    )))
}

fn empty_float64_array() -> Result<Value, crate::execute::VmError> {
    let buffer = Rc::new(crate::value::ArrayBufferData::new(0));
    Ok(Value::Float64Array(Rc::new(
        crate::value::Float64ArrayData::new(buffer, 0, 0),
    )))
}

fn values_float64_array(values: &[Value]) -> Result<Value, crate::execute::VmError> {
    let buffer = Rc::new(crate::value::ArrayBufferData::new(
        values.len() * crate::value::Float64ArrayData::BYTES_PER_ELEMENT,
    ));
    let view = crate::value::Float64ArrayData::new(buffer, 0, values.len());
    for (index, value) in values.iter().enumerate() {
        view.set(index, number_for_typed_array(value)?);
    }
    Ok(Value::Float64Array(Rc::new(view)))
}

fn copy_float64_array(
    source: &crate::value::Float64ArrayData,
) -> Result<Value, crate::execute::VmError> {
    let length = source.logical_len();
    let buffer = Rc::new(crate::value::ArrayBufferData::new(source.byte_length()));
    let view = crate::value::Float64ArrayData::new(buffer, 0, length);
    for index in 0..length {
        view.set(index, source.get(index).unwrap_or(f64::NAN));
    }
    Ok(Value::Float64Array(Rc::new(view)))
}

fn view_float64_array(
    buffer: &Rc<crate::value::ArrayBufferData>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let element_size = crate::value::Float64ArrayData::BYTES_PER_ELEMENT;
    let (offset, length) = typed_view_bounds(buffer, arguments, element_size, "Float64Array")?;
    Ok(Value::Float64Array(Rc::new(
        crate::value::Float64ArrayData::new(buffer.clone(), offset, length),
    )))
}

pub(crate) fn to_index(value: f64) -> Result<usize, crate::execute::VmError> {
    if value.is_nan() {
        return Ok(0);
    }
    let truncated = value.trunc();
    if !value.is_finite() || truncated < 0.0 {
        return Err(range_error("Invalid typed-array length"));
    }
    usize::try_from(truncated as u128).map_err(|_| range_error("Typed-array length is too large"))
}

fn type_error(message: &str) -> crate::execute::VmError {
    crate::value::error::throw_type_error(message)
}
