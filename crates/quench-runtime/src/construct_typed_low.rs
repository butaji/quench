fn construct_float64_array(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    match arguments.first() {
        None | Some(Value::Undefined) => empty_float64_array(),
        Some(Value::ArrayBuffer(buffer)) => view_float64_array(buffer, arguments),
        Some(Value::Float64Array(view)) => copy_float64_array(view),
        Some(Value::Array(values)) => values_float64_array(values),
        Some(_) => Err(type_error(
            "Float64Array source must be iterable or a buffer",
        )),
    }
}

fn construct_float32_array(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    match arguments.first() {
        None | Some(Value::Undefined) => empty_float32_array(),
        Some(Value::ArrayBuffer(buffer)) => view_float32_array(buffer, arguments),
        Some(Value::Float32Array(view)) => copy_float32_array(view),
        Some(Value::Array(values)) => values_float32_array(values),
        Some(_) => Err(type_error(
            "Float32Array source must be iterable or a buffer",
        )),
    }
}

fn construct_int8_array(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    match arguments.first() {
        None | Some(Value::Undefined) => empty_int8_array(),
        Some(Value::ArrayBuffer(buffer)) => view_int8_array(buffer, arguments),
        Some(Value::Int8Array(view)) => copy_int8_array(view),
        Some(Value::Array(values)) => values_int8_array(values),
        Some(_) => Err(type_error("Int8Array source must be iterable or a buffer")),
    }
}

fn construct_int16_array(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    match arguments.first() {
        None | Some(Value::Undefined) => empty_int16_array(),
        Some(Value::ArrayBuffer(buffer)) => view_int16_array(buffer, arguments),
        Some(Value::Int16Array(view)) => copy_int16_array(view),
        Some(Value::Array(values)) => values_int16_array(values),
        Some(_) => Err(type_error("Int16Array source must be iterable or a buffer")),
    }
}

fn construct_int32_array(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    match arguments.first() {
        None | Some(Value::Undefined) => empty_int32_array(),
        Some(Value::ArrayBuffer(buffer)) => view_int32_array(buffer, arguments),
        Some(Value::Int32Array(view)) => copy_int32_array(view),
        Some(Value::Array(values)) => values_int32_array(values),
        Some(_) => Err(type_error("Int32Array source must be iterable or a buffer")),
    }
}

fn construct_uint8_array(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    match arguments.first() {
        None | Some(Value::Undefined) => empty_uint8_array(),
        Some(Value::ArrayBuffer(buffer)) => view_uint8_array(buffer, arguments),
        Some(Value::Uint8Array(view)) => copy_uint8_array(view),
        Some(Value::Array(values)) => values_uint8_array(values),
        Some(_) => Err(type_error("Uint8Array source must be iterable or a buffer")),
    }
}

fn construct_uint32_array(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    match arguments.first() {
        None | Some(Value::Undefined) => empty_uint32_array(),
        Some(Value::ArrayBuffer(buffer)) => view_uint32_array(buffer, arguments),
        Some(Value::Uint32Array(view)) => copy_uint32_array(view),
        Some(Value::Array(values)) => values_uint32_array(values),
        Some(_) => Err(type_error(
            "Uint32Array source must be iterable or a buffer",
        )),
    }
}

fn construct_uint16_array(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    match arguments.first() {
        None | Some(Value::Undefined) => empty_uint16_array(),
        Some(Value::ArrayBuffer(buffer)) => view_uint16_array(buffer, arguments),
        Some(Value::Uint16Array(view)) => copy_uint16_array(view),
        Some(Value::Array(values)) => values_uint16_array(values),
        Some(_) => Err(type_error(
            "Uint16Array source must be iterable or a buffer",
        )),
    }
}

fn empty_uint16_array() -> Result<Value, crate::execute::VmError> {
    let buffer = Rc::new(crate::value::ArrayBufferData::new(0));
    Ok(Value::Uint16Array(Rc::new(
        crate::value::Uint16ArrayData::new(buffer, 0, 0),
    )))
}

fn values_uint16_array(values: &[Value]) -> Result<Value, crate::execute::VmError> {
    let element_size = crate::value::Uint16ArrayData::BYTES_PER_ELEMENT;
    let buffer = Rc::new(crate::value::ArrayBufferData::new(
        values.len() * element_size,
    ));
    let view = crate::value::Uint16ArrayData::new(buffer, 0, values.len());
    for (index, value) in values.iter().enumerate() {
        view.set(
            index,
            to_uint16(crate::intl::tolocale::value::to_number(Some(value))),
        );
    }
    Ok(Value::Uint16Array(Rc::new(view)))
}

fn copy_uint16_array(
    source: &crate::value::Uint16ArrayData,
) -> Result<Value, crate::execute::VmError> {
    let buffer = Rc::new(crate::value::ArrayBufferData::new(source.byte_length()));
    let view = crate::value::Uint16ArrayData::new(buffer, 0, source.length);
    for index in 0..source.length {
        view.set(index, source.get(index).unwrap_or(0));
    }
    Ok(Value::Uint16Array(Rc::new(view)))
}

fn view_uint16_array(
    buffer: &Rc<crate::value::ArrayBufferData>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let offset = arguments.get(1).map_or(0.0, |value| {
        crate::intl::tolocale::value::to_number(Some(value))
    });
    let offset = to_index(offset)?;
    let element_size = crate::value::Uint16ArrayData::BYTES_PER_ELEMENT;
    if offset % element_size != 0 || offset > buffer.byte_length() {
        return Err(range_error("Invalid Uint16Array byte offset"));
    }
    let available = buffer.byte_length() - offset;
    let length = match arguments.get(2) {
        Some(value) => to_index(crate::intl::tolocale::value::to_number(Some(value)))?,
        None => available / element_size,
    };
    if length > available / element_size {
        return Err(range_error("Invalid Uint16Array length"));
    }
    Ok(Value::Uint16Array(Rc::new(
        crate::value::Uint16ArrayData::new(buffer.clone(), offset, length),
    )))
}

fn empty_uint32_array() -> Result<Value, crate::execute::VmError> {
    let buffer = Rc::new(crate::value::ArrayBufferData::new(0));
    Ok(Value::Uint32Array(Rc::new(
        crate::value::Uint32ArrayData::new(buffer, 0, 0),
    )))
}

fn values_uint32_array(values: &[Value]) -> Result<Value, crate::execute::VmError> {
    let element_size = crate::value::Uint32ArrayData::BYTES_PER_ELEMENT;
    let buffer = Rc::new(crate::value::ArrayBufferData::new(
        values.len() * element_size,
    ));
    let view = crate::value::Uint32ArrayData::new(buffer, 0, values.len());
    for (index, value) in values.iter().enumerate() {
        view.set(
            index,
            to_uint32(crate::intl::tolocale::value::to_number(Some(value))),
        );
    }
    Ok(Value::Uint32Array(Rc::new(view)))
}

fn copy_uint32_array(
    source: &crate::value::Uint32ArrayData,
) -> Result<Value, crate::execute::VmError> {
    let buffer = Rc::new(crate::value::ArrayBufferData::new(source.byte_length()));
    let view = crate::value::Uint32ArrayData::new(buffer, 0, source.length);
    for index in 0..source.length {
        view.set(index, source.get(index).unwrap_or(0));
    }
    Ok(Value::Uint32Array(Rc::new(view)))
}

fn view_uint32_array(
    buffer: &Rc<crate::value::ArrayBufferData>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let offset = arguments.get(1).map_or(0.0, |value| {
        crate::intl::tolocale::value::to_number(Some(value))
    });
    let offset = to_index(offset)?;
    let element_size = crate::value::Uint32ArrayData::BYTES_PER_ELEMENT;
    if offset % element_size != 0 || offset > buffer.byte_length() {
        return Err(range_error("Invalid Uint32Array byte offset"));
    }
    let available = buffer.byte_length() - offset;
    let length = match arguments.get(2) {
        Some(value) => to_index(crate::intl::tolocale::value::to_number(Some(value)))?,
        None => available / element_size,
    };
    if length > available / element_size {
        return Err(range_error("Invalid Uint32Array length"));
    }
    Ok(Value::Uint32Array(Rc::new(
        crate::value::Uint32ArrayData::new(buffer.clone(), offset, length),
    )))
}

fn construct_uint8_clamped_array(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    match arguments.first() {
        None | Some(Value::Undefined) => empty_uint8_clamped_array(),
        Some(Value::ArrayBuffer(buffer)) => view_uint8_clamped_array(buffer, arguments),
        Some(Value::Uint8ClampedArray(view)) => copy_uint8_clamped_array(view),
        Some(Value::Array(values)) => values_uint8_clamped_array(values),
        Some(_) => Err(type_error(
            "Uint8ClampedArray source must be iterable or a buffer",
        )),
    }
}

fn empty_uint8_array() -> Result<Value, crate::execute::VmError> {
    let buffer = Rc::new(crate::value::ArrayBufferData::new(0));
    Ok(Value::Uint8Array(Rc::new(
        crate::value::Uint8ArrayData::new(buffer, 0, 0),
    )))
}

fn values_uint8_array(values: &[Value]) -> Result<Value, crate::execute::VmError> {
    let buffer = Rc::new(crate::value::ArrayBufferData::new(values.len()));
    let view = crate::value::Uint8ArrayData::new(buffer, 0, values.len());
    for (index, value) in values.iter().enumerate() {
        view.set(
            index,
            to_uint8(crate::intl::tolocale::value::to_number(Some(value))),
        );
    }
    Ok(Value::Uint8Array(Rc::new(view)))
}

fn empty_uint8_clamped_array() -> Result<Value, crate::execute::VmError> {
    let buffer = Rc::new(crate::value::ArrayBufferData::new(0));
    Ok(Value::Uint8ClampedArray(Rc::new(
        crate::value::Uint8ClampedArrayData::new(buffer, 0, 0),
    )))
}

fn values_uint8_clamped_array(values: &[Value]) -> Result<Value, crate::execute::VmError> {
    let buffer = Rc::new(crate::value::ArrayBufferData::new(values.len()));
    let view = crate::value::Uint8ClampedArrayData::new(buffer, 0, values.len());
    for (index, value) in values.iter().enumerate() {
        view.set(index, crate::intl::tolocale::value::to_number(Some(value)));
    }
    Ok(Value::Uint8ClampedArray(Rc::new(view)))
}

fn copy_uint8_clamped_array(
    source: &crate::value::Uint8ClampedArrayData,
) -> Result<Value, crate::execute::VmError> {
    let buffer = Rc::new(crate::value::ArrayBufferData::new(source.byte_length()));
    let view = crate::value::Uint8ClampedArrayData::new(buffer, 0, source.length);
    for index in 0..source.length {
        view.set(index, f64::from(source.get(index).unwrap_or(0)));
    }
    Ok(Value::Uint8ClampedArray(Rc::new(view)))
}

fn view_uint8_clamped_array(
    buffer: &Rc<crate::value::ArrayBufferData>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let offset = arguments.get(1).map_or(0.0, |value| {
        crate::intl::tolocale::value::to_number(Some(value))
    });
    let offset = to_index(offset)?;
    if offset > buffer.byte_length() {
        return Err(range_error("Invalid Uint8ClampedArray byte offset"));
    }
    let available = buffer.byte_length() - offset;
    let length = match arguments.get(2) {
        Some(value) => to_index(crate::intl::tolocale::value::to_number(Some(value)))?,
        None => available,
    };
    if length > available {
        return Err(range_error("Invalid Uint8ClampedArray length"));
    }
    Ok(Value::Uint8ClampedArray(Rc::new(
        crate::value::Uint8ClampedArrayData::new(buffer.clone(), offset, length),
    )))
}

fn copy_uint8_array(
    source: &crate::value::Uint8ArrayData,
) -> Result<Value, crate::execute::VmError> {
    let buffer = Rc::new(crate::value::ArrayBufferData::new(source.byte_length()));
    let view = crate::value::Uint8ArrayData::new(buffer, 0, source.length);
    for index in 0..source.length {
        view.set(index, source.get(index).unwrap_or(0));
    }
    Ok(Value::Uint8Array(Rc::new(view)))
}

fn view_uint8_array(
    buffer: &Rc<crate::value::ArrayBufferData>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let offset = arguments.get(1).map_or(0.0, |value| {
        crate::intl::tolocale::value::to_number(Some(value))
    });
    let offset = to_index(offset)?;
    if offset > buffer.byte_length() {
        return Err(range_error("Invalid Uint8Array byte offset"));
    }
    let available = buffer.byte_length() - offset;
    let length = match arguments.get(2) {
        Some(value) => to_index(crate::intl::tolocale::value::to_number(Some(value)))?,
        None => available,
    };
    if length > available {
        return Err(range_error("Invalid Uint8Array length"));
    }
    Ok(Value::Uint8Array(Rc::new(
        crate::value::Uint8ArrayData::new(buffer.clone(), offset, length),
    )))
}
