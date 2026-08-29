use std::rc::Rc;

fn array_iterator(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    if let Some(Value::Array(data)) = receiver {
        return Ok(crate::collections::iterator::make_array(Rc::clone(data)));
    }
    if let Some(value) = receiver.filter(|value| value.is_typed_array()) {
        return Ok(crate::collections::iterator::make_typed(value.clone()));
    }
    Ok(crate::collections::iterator::make(array_iterator_values(
        receiver,
    )?))
}
fn typed_array_iterator(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    let value = receiver.map(unwrap_binding_cells);
    let Some(value) = value.as_ref().filter(|value| is_typed_array(value)) else {
        return Err(crate::value::error::throw_type_error(
            "TypedArray.prototype.values called on incompatible receiver",
        ));
    };
    if typed_array_is_detached(value) {
        return Err(crate::value::error::throw_type_error(
            "TypedArray iterator called on detached TypedArray",
        ));
    }
    if crate::typed_array::prototype::is_out_of_bounds(value) {
        return Err(crate::value::error::throw_type_error(
            "TypedArray iterator called on out-of-bounds TypedArray",
        ));
    }
    Ok(crate::collections::iterator::make_typed(value.clone()))
}

fn unwrap_binding_cells(value: &Value) -> Value {
    match value {
        Value::BindingCell(cell) => unwrap_binding_cells(&cell.borrow()),
        value => value.clone(),
    }
}

pub(crate) fn typed_array_is_detached(value: &Value) -> bool {
    let buffer = match value {
        Value::Float64Array(data) => &data.buffer,
        Value::Float32Array(data) => &data.buffer,
        Value::Int8Array(data) => &data.buffer,
        Value::Int16Array(data) => &data.buffer,
        Value::Int32Array(data) => &data.buffer,
        Value::Uint8Array(data) => &data.buffer,
        Value::Uint8ClampedArray(data) => &data.buffer,
        Value::Uint16Array(data) => &data.buffer,
        Value::Uint32Array(data) => &data.buffer,
        Value::BigInt64Array(data) => &data.buffer,
        Value::BigUint64Array(data) => &data.buffer,
        _ => return false,
    };
    *buffer.detached.borrow()
}

fn array_keys(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    if let Some(Value::Array(data)) = receiver {
        return Ok(crate::collections::iterator::make_array_keys(Rc::clone(data)));
    }
    if let Some(value) = receiver.filter(|value| is_typed_array(value)) {
        return Ok(crate::collections::iterator::make_typed_keys(value.clone()));
    }
    let values = array_iterator_values(receiver)?;
    Ok(crate::collections::iterator::make(
        (0..values.len())
            .map(|index| Value::Number(index as f64))
            .collect(),
    ))
}

fn array_entries(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    if let Some(Value::Array(data)) = receiver {
        return Ok(crate::collections::iterator::make_array_entries(Rc::clone(data)));
    }
    if let Some(value) = receiver.filter(|value| value.is_typed_array()) {
        return Ok(crate::collections::iterator::make_typed_entries(value.clone()));
    }
    let values = array_iterator_values(receiver)?;
    Ok(crate::collections::iterator::make(
        values
            .into_iter()
            .enumerate()
            .map(|(index, value)| Value::array(vec![Value::Number(index as f64), value]))
            .collect(),
    ))
}

fn typed_array_entries(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    let value = receiver.map(unwrap_binding_cells);
    let Some(value) = value.as_ref().filter(|value| is_typed_array(value)) else {
        return Err(crate::value::error::throw_type_error(
            "TypedArray.prototype.entries called on incompatible receiver",
        ));
    };
    if typed_array_is_detached(value) {
        return Err(crate::value::error::throw_type_error(
            "TypedArray iterator called on detached TypedArray",
        ));
    }
    if crate::typed_array::prototype::is_out_of_bounds(value) {
        return Err(crate::value::error::throw_type_error(
            "TypedArray iterator called on out-of-bounds TypedArray",
        ));
    }
    Ok(crate::collections::iterator::make_typed_entries(value.clone()))
}

fn typed_array_keys(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    let value = receiver.map(unwrap_binding_cells);
    let Some(value) = value.as_ref().filter(|value| is_typed_array(value)) else {
        return Err(crate::value::error::throw_type_error(
            "TypedArray.prototype.keys called on incompatible receiver",
        ));
    };
    if typed_array_is_detached(value) {
        return Err(crate::value::error::throw_type_error(
            "TypedArray iterator called on detached TypedArray",
        ));
    }
    if crate::typed_array::prototype::is_out_of_bounds(value) {
        return Err(crate::value::error::throw_type_error(
            "TypedArray iterator called on out-of-bounds TypedArray",
        ));
    }
    Ok(crate::collections::iterator::make_typed_keys(value.clone()))
}

fn array_iterator_values(receiver: Option<&Value>) -> Result<Vec<Value>, crate::execute::VmError> {
    if let Some(Value::Array(values)) = receiver {
        return Ok(values.snapshot());
    }
    let Some(value) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "Array iterator called on incompatible receiver",
        ));
    };
    array_like_values(value)
}

fn is_typed_array(value: &Value) -> bool {
    matches!(
        value,
        Value::Float64Array(_)
            | Value::Float32Array(_)
            | Value::Int8Array(_)
            | Value::Int16Array(_)
            | Value::Int32Array(_)
            | Value::Uint8Array(_)
            | Value::Uint8ClampedArray(_)
            | Value::Uint16Array(_)
            | Value::Uint32Array(_)
            | Value::BigInt64Array(_)
            | Value::BigUint64Array(_)
    )
}

fn array_like_values(value: &Value) -> Result<Vec<Value>, crate::execute::VmError> {
    if matches!(value, Value::Null | Value::Undefined) {
        return Err(crate::value::error::throw_type_error(
            "Array iterator called on incompatible receiver",
        ));
    }
    let length =
        crate::conversion::to_number(&crate::execute::get_property_result(value, "length")?)?;
    let length = if length.is_finite() && length > 0.0 {
        length.floor().min(usize::MAX as f64) as usize
    } else {
        0
    };
    let mut values = Vec::with_capacity(length.min(1024));
    for index in 0..length {
        values.push(crate::execute::get_property_result(
            value,
            &index.to_string(),
        )?);
    }
    Ok(values)
}
