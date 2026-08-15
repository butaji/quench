const BIGINT_ELEMENT_SIZE: usize = crate::value::BigInt64ArrayData::BYTES_PER_ELEMENT;

pub(crate) fn construct_bigint64_array(
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    match arguments.first() {
        None | Some(Value::Undefined) => bigint64_with_length(0),
        Some(Value::ArrayBuffer(buffer)) => view_bigint64_array(buffer, arguments),
        Some(Value::BigInt64Array(view)) => copy_bigint64_array(view),
        Some(Value::Array(values)) => values_bigint64_array(values),
        Some(Value::Number(length)) => bigint64_with_length(to_index(*length)?),
        Some(Value::Object(properties)) => excessive_object_length(properties),
        Some(_) => Err(type_error("BigInt64Array source must contain BigInts")),
    }
}

pub(crate) fn construct_biguint64_array(
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    match arguments.first() {
        None | Some(Value::Undefined) => biguint64_with_length(0),
        Some(Value::ArrayBuffer(buffer)) => view_biguint64_array(buffer, arguments),
        Some(Value::BigUint64Array(view)) => copy_biguint64_array(view),
        Some(Value::Array(values)) => values_biguint64_array(values),
        Some(Value::Number(length)) => biguint64_with_length(to_index(*length)?),
        Some(Value::Object(properties)) => excessive_object_length(properties),
        Some(_) => Err(type_error("BigUint64Array source must contain BigInts")),
    }
}

fn bigint64_with_length(length: usize) -> Result<Value, crate::execute::VmError> {
    Ok(Value::BigInt64Array(new_bigint64_data(length)?))
}

fn biguint64_with_length(length: usize) -> Result<Value, crate::execute::VmError> {
    Ok(Value::BigUint64Array(new_biguint64_data(length)?))
}

fn new_bigint64_data(
    length: usize,
) -> Result<Rc<crate::value::BigInt64ArrayData>, crate::execute::VmError> {
    let bytes = typed_byte_length(length)?;
    let buffer = Rc::new(
        crate::value::ArrayBufferData::try_new(bytes)
            .ok_or_else(|| range_error("Typed-array length is too large"))?,
    );
    Ok(Rc::new(crate::value::BigInt64ArrayData::new(
        buffer, 0, length,
    )))
}

fn new_biguint64_data(
    length: usize,
) -> Result<Rc<crate::value::BigUint64ArrayData>, crate::execute::VmError> {
    let bytes = typed_byte_length(length)?;
    let buffer = Rc::new(
        crate::value::ArrayBufferData::try_new(bytes)
            .ok_or_else(|| range_error("Typed-array length is too large"))?,
    );
    Ok(Rc::new(crate::value::BigUint64ArrayData::new(
        buffer, 0, length,
    )))
}

fn excessive_object_length(
    properties: &Rc<crate::value::ObjectData>,
) -> Result<Value, crate::execute::VmError> {
    let length = crate::execute::get_property_result(
        &Value::Object(properties.clone()),
        "length",
    )?;
    let length = crate::conversion::to_number(&length)?;
    if length >= 9_007_199_254_740_992.0 {
        return Err(range_error("Typed-array length is too large"));
    }
    Err(type_error("BigInt typed-array source must contain BigInts"))
}

fn values_bigint64_array(values: &[Value]) -> Result<Value, crate::execute::VmError> {
    let view = new_bigint64_data(values.len())?;
    for (index, value) in values.iter().enumerate() {
        view.set(index, bigint_bits(value)? as i64);
    }
    Ok(Value::BigInt64Array(view))
}

fn values_biguint64_array(values: &[Value]) -> Result<Value, crate::execute::VmError> {
    let view = new_biguint64_data(values.len())?;
    for (index, value) in values.iter().enumerate() {
        view.set(index, bigint_bits(value)?);
    }
    Ok(Value::BigUint64Array(view))
}

fn copy_bigint64_array(
    source: &crate::value::BigInt64ArrayData,
) -> Result<Value, crate::execute::VmError> {
    let view = new_bigint64_data(source.length)?;
    for index in 0..source.length {
        view.set(index, source.get(index).unwrap_or(0));
    }
    Ok(Value::BigInt64Array(view))
}

fn copy_biguint64_array(
    source: &crate::value::BigUint64ArrayData,
) -> Result<Value, crate::execute::VmError> {
    let view = new_biguint64_data(source.length)?;
    for index in 0..source.length {
        view.set(index, source.get(index).unwrap_or(0));
    }
    Ok(Value::BigUint64Array(view))
}

fn view_bigint64_array(
    buffer: &Rc<crate::value::ArrayBufferData>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let (offset, length) = bigint_view_bounds(buffer, arguments, "BigInt64Array")?;
    Ok(Value::BigInt64Array(Rc::new(
        crate::value::BigInt64ArrayData::new(buffer.clone(), offset, length),
    )))
}

fn view_biguint64_array(
    buffer: &Rc<crate::value::ArrayBufferData>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let (offset, length) = bigint_view_bounds(buffer, arguments, "BigUint64Array")?;
    Ok(Value::BigUint64Array(Rc::new(
        crate::value::BigUint64ArrayData::new(buffer.clone(), offset, length),
    )))
}

fn bigint_view_bounds(
    buffer: &crate::value::ArrayBufferData,
    arguments: &[Value],
    name: &str,
) -> Result<(usize, usize), crate::execute::VmError> {
    if *buffer.detached.borrow() {
        return Err(type_error("Cannot use a detached ArrayBuffer"));
    }
    let offset = match arguments.get(1) {
        Some(value) => crate::conversion::to_number(value)?,
        None => 0.0,
    };
    let offset = to_index(offset)?;
    let available = buffer
        .byte_length()
        .checked_sub(offset)
        .ok_or_else(|| range_error(&format!("Invalid {name} byte offset")))?;
    if offset % BIGINT_ELEMENT_SIZE != 0
        || arguments.get(2).is_none() && available % BIGINT_ELEMENT_SIZE != 0
    {
        return Err(range_error(&format!("Invalid {name} byte offset")));
    }
    let length = match arguments.get(2) {
        Some(value) => to_index(crate::conversion::to_number(value)?)?,
        None => view_length(buffer, available / BIGINT_ELEMENT_SIZE),
    };
    if arguments.get(2).is_some() && typed_byte_length(length)? > available {
        return Err(range_error(&format!("Invalid {name} length")));
    }
    Ok((offset, length))
}

fn typed_byte_length(length: usize) -> Result<usize, crate::execute::VmError> {
    length
        .checked_mul(BIGINT_ELEMENT_SIZE)
        .ok_or_else(|| range_error("Typed-array length is too large"))
}

pub(crate) fn bigint_bits(value: &Value) -> Result<u64, crate::execute::VmError> {
    let raw = match value {
        Value::BigInt(raw) | Value::String(raw) => raw,
        _ => {
            return Err(type_error("Cannot convert a non-BigInt value to BigInt"));
        }
    };
    let integer = raw
        .parse::<num_bigint::BigInt>()
        .map_err(|_| type_error("Invalid BigInt value"))?;
    let fill = if integer.sign() == num_bigint::Sign::Minus {
        u8::MAX
    } else {
        0
    };
    let source = integer.to_signed_bytes_le();
    let mut bytes = [fill; BIGINT_ELEMENT_SIZE];
    let count = source.len().min(bytes.len());
    bytes[..count].copy_from_slice(&source[..count]);
    Ok(u64::from_le_bytes(bytes))
}
