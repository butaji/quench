fn object_array_like(properties: &crate::value::ObjectData) -> Option<Vec<Value>> {
    let (_, length) = properties.iter().rev().find(|(name, _)| name == "length")?;
    let Value::Number(length) = length else {
        return None;
    };
    let length = (*length).max(0.0) as usize;
    let mut values = Vec::with_capacity(length);
    for index in 0..length {
        let key = index.to_string();
        let value = properties
            .iter()
            .rev()
            .find(|(name, _)| name == &key)
            .map(|(_, value)| value.clone())
            .unwrap_or(Value::Undefined);
        values.push(value);
    }
    Some(values)
}
fn length_uint8_array(length: f64) -> Result<Value, crate::execute::VmError> {
    let length = to_index(length)?;
    let buffer = Rc::new(crate::value::ArrayBufferData::new(length));
    Ok(Value::Uint8Array(Rc::new(
        crate::value::Uint8ArrayData::new(buffer, 0, length),
    )))
}
fn length_float64_array(length: f64) -> Result<Value, crate::execute::VmError> {
    let length = to_index(length)?;
    let buffer = Rc::new(crate::value::ArrayBufferData::new(length * 8));
    Ok(Value::Float64Array(Rc::new(
        crate::value::Float64ArrayData::new(buffer, 0, length),
    )))
}
fn length_float32_array(length: f64) -> Result<Value, crate::execute::VmError> {
    let length = to_index(length)?;
    let buffer = Rc::new(crate::value::ArrayBufferData::new(length * 4));
    Ok(Value::Float32Array(Rc::new(
        crate::value::Float32ArrayData::new(buffer, 0, length),
    )))
}
fn length_int8_array(length: f64) -> Result<Value, crate::execute::VmError> {
    let length = to_index(length)?;
    let buffer = Rc::new(crate::value::ArrayBufferData::new(length));
    Ok(Value::Int8Array(Rc::new(
        crate::value::Int8ArrayData::new(buffer, 0, length),
    )))
}
fn length_int16_array(length: f64) -> Result<Value, crate::execute::VmError> {
    let length = to_index(length)?;
    let buffer = Rc::new(crate::value::ArrayBufferData::new(length * 2));
    Ok(Value::Int16Array(Rc::new(
        crate::value::Int16ArrayData::new(buffer, 0, length),
    )))
}
fn length_int32_array(length: f64) -> Result<Value, crate::execute::VmError> {
    let length = to_index(length)?;
    let buffer = Rc::new(crate::value::ArrayBufferData::new(length * 4));
    Ok(Value::Int32Array(Rc::new(
        crate::value::Int32ArrayData::new(buffer, 0, length),
    )))
}
fn length_uint32_array(length: f64) -> Result<Value, crate::execute::VmError> {
    let length = to_index(length)?;
    let buffer = Rc::new(crate::value::ArrayBufferData::new(length * 4));
    Ok(Value::Uint32Array(Rc::new(
        crate::value::Uint32ArrayData::new(buffer, 0, length),
    )))
}
fn length_uint16_array(length: f64) -> Result<Value, crate::execute::VmError> {
    let length = to_index(length)?;
    let buffer = Rc::new(crate::value::ArrayBufferData::new(length * 2));
    Ok(Value::Uint16Array(Rc::new(
        crate::value::Uint16ArrayData::new(buffer, 0, length),
    )))
}
fn length_uint8_clamped_array(length: f64) -> Result<Value, crate::execute::VmError> {
    let length = to_index(length)?;
    let buffer = Rc::new(crate::value::ArrayBufferData::new(length));
    Ok(Value::Uint8ClampedArray(Rc::new(
        crate::value::Uint8ClampedArrayData::new(buffer, 0, length),
    )))
}
