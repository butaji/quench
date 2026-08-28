fn object_array_like(
    properties: &crate::value::ObjectData,
) -> Result<Option<Vec<Value>>, crate::execute::VmError> {
    let object = Value::Object(Rc::new(properties.clone()));
    if let Ok(values) = crate::collections::iterator::collect_iterable(object.clone()) {
        return Ok(Some(values));
    }
    // A plain object with only data properties can be projected in one pass.
    // This keeps large array-like constructor inputs linear while leaving
    // accessors, prototypes, and iterators on the observable property path.
    let plain = properties.original_prototype().is_none_or(|prototype| {
        matches!(
            prototype,
            crate::value::Value::Builtin(crate::ops::Builtin::ObjectPrototype)
        )
    })
        && !properties.iter().any(|(name, _)| {
            name.starts_with('\0') || name == "Symbol.iterator"
    });
    if plain {
        let length = properties.value_for_key("length");
        let Some(length) = length else {
            return Ok(Some(Vec::new()));
        };
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
        let mut values = vec![Value::Undefined; length];
        for (name, value) in properties.iter() {
            if let Ok(index) = name.parse::<usize>() {
                if index < length {
                    values[index] = value.clone();
                }
            }
        }
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

#[inline]
fn number_for_typed_array(value: &crate::value::Value) -> Result<f64, crate::execute::VmError> {
    match value {
        crate::value::Value::Number(number) => Ok(*number),
        _ => crate::conversion::to_number(value),
    }
}

fn identity_number(number: f64) -> f64 {
    number
}

macro_rules! dense_numeric_constructor {
    ($name:ident, $variant:ident, $view:ident, $convert:expr) => {
        fn $name(numbers: &[f64]) -> Result<crate::value::Value, crate::execute::VmError> {
            let buffer = std::rc::Rc::new(crate::value::ArrayBufferData::new(
                numbers.len() * crate::value::$view::BYTES_PER_ELEMENT,
            ));
            let view = crate::value::$view::new(buffer, 0, numbers.len());
            for (index, number) in numbers.iter().copied().enumerate() {
                view.set(index, ($convert)(number));
            }
            Ok(crate::value::Value::$variant(std::rc::Rc::new(view)))
        }
    };
}

dense_numeric_constructor!(dense_float64_array, Float64Array, Float64ArrayData, identity_number);
dense_numeric_constructor!(dense_float32_array, Float32Array, Float32ArrayData, |number| number as f32);
dense_numeric_constructor!(dense_int8_array, Int8Array, Int8ArrayData, to_int8);
dense_numeric_constructor!(dense_int16_array, Int16Array, Int16ArrayData, to_int16);
dense_numeric_constructor!(dense_int32_array, Int32Array, Int32ArrayData, to_int32);
dense_numeric_constructor!(dense_uint8_array, Uint8Array, Uint8ArrayData, to_uint8);
dense_numeric_constructor!(
    dense_uint8_clamped_array,
    Uint8ClampedArray,
    Uint8ClampedArrayData,
    identity_number
);
dense_numeric_constructor!(dense_uint16_array, Uint16Array, Uint16ArrayData, to_uint16);
dense_numeric_constructor!(dense_uint32_array, Uint32Array, Uint32ArrayData, to_uint32);

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
            if available % element_size != 0 && buffer.max_byte_length.is_none() {
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
