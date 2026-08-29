fn array_iteration_is_intrinsic() -> bool {
    let result = !crate::builtins::builtin_prototype_property_is_removed(
        crate::ops::Builtin::ArrayPrototype,
        "Symbol.iterator",
    ) && crate::builtins::read_intrinsic_override(
        crate::ops::Builtin::ArrayPrototype,
        "Symbol.iterator",
    )
    .is_none()
        && crate::builtins::read_intrinsic_override(
            crate::ops::Builtin::ArrayIteratorPrototype,
            "next",
        )
        .is_none();
    result
}

fn construct_float64_array(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    match arguments.first() {
        None | Some(Value::Undefined) => empty_float64_array(),
        Some(Value::ArrayBuffer(buffer)) => view_float64_array(buffer, arguments),
        Some(Value::Float64Array(view)) => copy_float64_array(view),
        Some(Value::Array(values)) if array_iteration_is_intrinsic() => {
            values.dense_numeric_snapshot().map_or_else(
                || values_float64_array(&values.snapshot()),
                |numbers| dense_float64_array(&numbers),
            )
        }
        Some(Value::Object(properties)) => {
            let object = Value::Object(properties.clone());
            let values = object_array_like(properties)?
                .or_else(|| crate::collections::iterator::collect_iterable(object).ok());
            match values {
                Some(values) => values_float64_array(&values),
                None => Err(type_error(
                    "Float64Array source must be iterable or a buffer",
                )),
            }
        }
        Some(value) if crate::value::is_object(value) => {
            let values = crate::collections::iterator::collect_iterable(value.clone())?;
            values_float64_array(&values)
        }
        Some(value) => length_float64_array(crate::conversion::to_number(value)?),
    }
}

fn construct_float32_array(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    match arguments.first() {
        None | Some(Value::Undefined) => empty_float32_array(),
        Some(Value::ArrayBuffer(buffer)) => view_float32_array(buffer, arguments),
        Some(Value::Float32Array(view)) => copy_float32_array(view),
        Some(Value::Array(values)) if array_iteration_is_intrinsic() => {
            values.dense_numeric_snapshot().map_or_else(
                || values_float32_array(&values.snapshot()),
                |numbers| dense_float32_array(&numbers),
            )
        }
        Some(Value::Object(properties)) => {
            let object = Value::Object(properties.clone());
            let values = object_array_like(properties)?
                .or_else(|| crate::collections::iterator::collect_iterable(object).ok());
            match values {
                Some(values) => values_float32_array(&values),
                None => Err(type_error(
                    "Float32Array source must be iterable or a buffer",
                )),
            }
        }
        Some(value) if crate::value::is_object(value) => {
            let values = crate::collections::iterator::collect_iterable(value.clone())?;
            values_float32_array(&values)
        }
        Some(value) => length_float32_array(crate::conversion::to_number(value)?),
    }
}

fn construct_int8_array(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    match arguments.first() {
        None | Some(Value::Undefined) => empty_int8_array(),
        Some(Value::ArrayBuffer(buffer)) => view_int8_array(buffer, arguments),
        Some(Value::Int8Array(view)) => copy_int8_array(view),
        Some(Value::Array(values)) if array_iteration_is_intrinsic() => {
            values.dense_numeric_snapshot().map_or_else(
                || values_int8_array(&values.snapshot()),
                |numbers| dense_int8_array(&numbers),
            )
        }
        Some(Value::Object(properties)) => {
            let object = Value::Object(properties.clone());
            let values = object_array_like(properties)?
                .or_else(|| crate::collections::iterator::collect_iterable(object).ok());
            match values {
                Some(values) => values_int8_array(&values),
                None => Err(type_error("Int8Array source must be iterable or a buffer")),
            }
        }
        Some(value) if crate::value::is_object(value) => {
            let values = crate::collections::iterator::collect_iterable(value.clone())?;
            values_int8_array(&values)
        }
        Some(value) => length_int8_array(crate::conversion::to_number(value)?),
    }
}

fn construct_int16_array(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    match arguments.first() {
        None | Some(Value::Undefined) => empty_int16_array(),
        Some(Value::ArrayBuffer(buffer)) => view_int16_array(buffer, arguments),
        Some(Value::Int16Array(view)) => copy_int16_array(view),
        Some(Value::Array(values)) if array_iteration_is_intrinsic() => {
            values.dense_numeric_snapshot().map_or_else(
                || values_int16_array(&values.snapshot()),
                |numbers| dense_int16_array(&numbers),
            )
        }
        Some(Value::Object(properties)) => {
            let object = Value::Object(properties.clone());
            let values = object_array_like(properties)?
                .or_else(|| crate::collections::iterator::collect_iterable(object).ok());
            match values {
                Some(values) => values_int16_array(&values),
                None => Err(type_error("Int16Array source must be iterable or a buffer")),
            }
        }
        Some(value) if crate::value::is_object(value) => {
            let values = crate::collections::iterator::collect_iterable(value.clone())?;
            values_int16_array(&values)
        }
        Some(value) => length_int16_array(crate::conversion::to_number(value)?),
    }
}

fn construct_int32_array(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    match arguments.first() {
        None | Some(Value::Undefined) => empty_int32_array(),
        Some(Value::ArrayBuffer(buffer)) => view_int32_array(buffer, arguments),
        Some(Value::Int32Array(view)) => copy_int32_array(view),
        Some(Value::Array(values)) if array_iteration_is_intrinsic() => {
            values.dense_numeric_snapshot().map_or_else(
                || values_int32_array(&values.snapshot()),
                |numbers| dense_int32_array(&numbers),
            )
        }
        Some(Value::Object(properties)) => {
            let object = Value::Object(properties.clone());
            let values = object_array_like(properties)?
                .or_else(|| crate::collections::iterator::collect_iterable(object).ok());
            match values {
                Some(values) => values_int32_array(&values),
                None => Err(type_error("Int32Array source must be iterable or a buffer")),
            }
        }
        Some(value) if crate::value::is_object(value) => {
            let values = crate::collections::iterator::collect_iterable(value.clone())?;
            values_int32_array(&values)
        }
        Some(value) => length_int32_array(crate::conversion::to_number(value)?),
    }
}

fn construct_uint8_array(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    match arguments.first() {
        None | Some(Value::Undefined) => empty_uint8_array(),
        Some(Value::ArrayBuffer(buffer)) => view_uint8_array(buffer, arguments),
        Some(Value::Uint8Array(view)) => copy_uint8_array(view),
        Some(Value::Array(values)) if array_iteration_is_intrinsic() => {
            values.dense_numeric_snapshot().map_or_else(
                || values_uint8_array(&values.snapshot()),
                |numbers| dense_uint8_array(&numbers),
            )
        }
        Some(Value::Object(properties)) => {
            let object = Value::Object(properties.clone());
            let values = object_array_like(properties)?
                .or_else(|| crate::collections::iterator::collect_iterable(object).ok());
            match values {
                Some(values) => values_uint8_array(&values),
                None => Err(type_error("Uint8Array source must be iterable or a buffer")),
            }
        }
        Some(value) if !crate::value::is_object(value) => {
            length_uint8_array(crate::conversion::to_number(value)?)
        }
        // DataView is an array-buffer view but not an iterable source;
        // TypedArray construction observes its absent `length` as empty.
        Some(Value::DataView(_)) => empty_uint8_array(),
        // Every numeric typed array is iterable and is copied by element,
        // regardless of its source element width. Keep this conversion in
        // the constructor path so all callers share the same semantics.
        Some(value) => match crate::collections::iterator::collect_iterable(value.clone()) {
            Ok(values) => values_uint8_array(&values),
            Err(error) => Err(error),
        },
    }
}

fn construct_uint32_array(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    match arguments.first() {
        None | Some(Value::Undefined) => empty_uint32_array(),
        Some(Value::ArrayBuffer(buffer)) => view_uint32_array(buffer, arguments),
        Some(Value::Uint32Array(view)) => copy_uint32_array(view),
        Some(Value::Array(values)) if array_iteration_is_intrinsic() => {
            values.dense_numeric_snapshot().map_or_else(
                || values_uint32_array(&values.snapshot()),
                |numbers| dense_uint32_array(&numbers),
            )
        }
        Some(Value::Object(properties)) => {
            let object = Value::Object(properties.clone());
            let values = object_array_like(properties)?
                .or_else(|| crate::collections::iterator::collect_iterable(object).ok());
            match values {
                Some(values) => values_uint32_array(&values),
                None => Err(type_error(
                    "Uint32Array source must be iterable or a buffer",
                )),
            }
        }
        Some(value) if crate::value::is_object(value) => {
            let values = crate::collections::iterator::collect_iterable(value.clone())?;
            values_uint32_array(&values)
        }
        Some(value) => length_uint32_array(crate::conversion::to_number(value)?),
    }
}

fn construct_uint16_array(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    match arguments.first() {
        None | Some(Value::Undefined) => empty_uint16_array(),
        Some(Value::ArrayBuffer(buffer)) => view_uint16_array(buffer, arguments),
        Some(Value::Uint16Array(view)) => copy_uint16_array(view),
        Some(Value::Array(values)) if array_iteration_is_intrinsic() => {
            values.dense_numeric_snapshot().map_or_else(
                || values_uint16_array(&values.snapshot()),
                |numbers| dense_uint16_array(&numbers),
            )
        }
        Some(Value::Object(properties)) => {
            let object = Value::Object(properties.clone());
            let values = object_array_like(properties)?
                .or_else(|| crate::collections::iterator::collect_iterable(object).ok());
            match values {
                Some(values) => values_uint16_array(&values),
                None => Err(type_error(
                    "Uint16Array source must be iterable or a buffer",
                )),
            }
        }
        Some(value) if crate::value::is_object(value) => {
            let values = crate::collections::iterator::collect_iterable(value.clone())?;
            values_uint16_array(&values)
        }
        Some(value) => length_uint16_array(crate::conversion::to_number(value)?),
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
        view.set(index, to_uint16(number_for_typed_array(value)?));
    }
    Ok(Value::Uint16Array(Rc::new(view)))
}

fn copy_uint16_array(
    source: &crate::value::Uint16ArrayData,
) -> Result<Value, crate::execute::VmError> {
    let length = source.logical_len();
    let buffer = Rc::new(crate::value::ArrayBufferData::new(source.byte_length()));
    let view = crate::value::Uint16ArrayData::new(buffer, 0, length);
    for index in 0..length {
        view.set(index, source.get(index).unwrap_or(0));
    }
    Ok(Value::Uint16Array(Rc::new(view)))
}

fn view_uint16_array(
    buffer: &Rc<crate::value::ArrayBufferData>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let element_size = crate::value::Uint16ArrayData::BYTES_PER_ELEMENT;
    let (offset, length) = typed_view_bounds(buffer, arguments, element_size, "Uint16Array")?;
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
        view.set(index, to_uint32(number_for_typed_array(value)?));
    }
    Ok(Value::Uint32Array(Rc::new(view)))
}

fn copy_uint32_array(
    source: &crate::value::Uint32ArrayData,
) -> Result<Value, crate::execute::VmError> {
    let length = source.logical_len();
    let buffer = Rc::new(crate::value::ArrayBufferData::new(source.byte_length()));
    let view = crate::value::Uint32ArrayData::new(buffer, 0, length);
    for index in 0..length {
        view.set(index, source.get(index).unwrap_or(0));
    }
    Ok(Value::Uint32Array(Rc::new(view)))
}

fn view_uint32_array(
    buffer: &Rc<crate::value::ArrayBufferData>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let element_size = crate::value::Uint32ArrayData::BYTES_PER_ELEMENT;
    let (offset, length) = typed_view_bounds(buffer, arguments, element_size, "Uint32Array")?;
    Ok(Value::Uint32Array(Rc::new(
        crate::value::Uint32ArrayData::new(buffer.clone(), offset, length),
    )))
}

fn construct_uint8_clamped_array(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    match arguments.first() {
        None | Some(Value::Undefined) => empty_uint8_clamped_array(),
        Some(Value::ArrayBuffer(buffer)) => view_uint8_clamped_array(buffer, arguments),
        Some(Value::Uint8ClampedArray(view)) => copy_uint8_clamped_array(view),
        Some(Value::Array(values)) if array_iteration_is_intrinsic() => {
            values.dense_numeric_snapshot().map_or_else(
                || values_uint8_clamped_array(&values.snapshot()),
                |numbers| dense_uint8_clamped_array(&numbers),
            )
        }
        Some(Value::Object(properties)) => {
            let object = Value::Object(properties.clone());
            let values = object_array_like(properties)?
                .or_else(|| crate::collections::iterator::collect_iterable(object).ok());
            match values {
                Some(values) => values_uint8_clamped_array(&values),
                None => Err(type_error(
                    "Uint8ClampedArray source must be iterable or a buffer",
                )),
            }
        }
        Some(value) if crate::value::is_object(value) => {
            let values = crate::collections::iterator::collect_iterable(value.clone())?;
            values_uint8_clamped_array(&values)
        }
        Some(value) => length_uint8_clamped_array(crate::conversion::to_number(value)?),
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
        view.set(index, to_uint8(number_for_typed_array(value)?));
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
        view.set(index, number_for_typed_array(value)?);
    }
    Ok(Value::Uint8ClampedArray(Rc::new(view)))
}

fn copy_uint8_clamped_array(
    source: &crate::value::Uint8ClampedArrayData,
) -> Result<Value, crate::execute::VmError> {
    let length = source.logical_len();
    let buffer = Rc::new(crate::value::ArrayBufferData::new(source.byte_length()));
    let view = crate::value::Uint8ClampedArrayData::new(buffer, 0, length);
    for index in 0..length {
        view.set(index, f64::from(source.get(index).unwrap_or(0)));
    }
    Ok(Value::Uint8ClampedArray(Rc::new(view)))
}

fn view_uint8_clamped_array(
    buffer: &Rc<crate::value::ArrayBufferData>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let (offset, length) = typed_view_bounds(
        buffer,
        arguments,
        crate::value::Uint8ClampedArrayData::BYTES_PER_ELEMENT,
        "Uint8ClampedArray",
    )?;
    Ok(Value::Uint8ClampedArray(Rc::new(
        crate::value::Uint8ClampedArrayData::new(buffer.clone(), offset, length),
    )))
}

fn copy_uint8_array(
    source: &crate::value::Uint8ArrayData,
) -> Result<Value, crate::execute::VmError> {
    let length = source.logical_len();
    let buffer = Rc::new(crate::value::ArrayBufferData::new(source.byte_length()));
    let view = crate::value::Uint8ArrayData::new(buffer, 0, length);
    for index in 0..length {
        view.set(index, source.get(index).unwrap_or(0));
    }
    Ok(Value::Uint8Array(Rc::new(view)))
}

fn view_uint8_array(
    buffer: &Rc<crate::value::ArrayBufferData>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let (offset, length) = typed_view_bounds(
        buffer,
        arguments,
        crate::value::Uint8ArrayData::BYTES_PER_ELEMENT,
        "Uint8Array",
    )?;
    Ok(Value::Uint8Array(Rc::new(
        crate::value::Uint8ArrayData::new(buffer.clone(), offset, length),
    )))
}
