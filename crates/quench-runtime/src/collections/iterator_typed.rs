use crate::value::Value;

macro_rules! number_values {
    ($data:expr) => {
        collect_typed(
            checked_length(
                $data.length,
                $data.byte_offset,
                $data.byte_length(),
                $data.logical_len(),
                &$data.buffer,
            )?,
            |index| $data.get(index).map(|value| Value::Number(value.into())),
        )
    };
}
macro_rules! bigint_values {
    ($data:expr) => {
        collect_typed(
            checked_length(
                $data.length,
                $data.byte_offset,
                $data.byte_length(),
                $data.logical_len(),
                &$data.buffer,
            )?,
            |index| {
                $data
                    .get(index)
                    .map(|value| Value::BigInt(value.to_string()))
            },
        )
    };
}
pub(crate) fn typed_values(value: Value) -> Result<Vec<Value>, crate::execute::VmError> {
    if let Value::BindingCell(cell) = value {
        return typed_values(cell.borrow().clone());
    }
    match value {
        Value::Float64Array(data) => number_values!(data),
        Value::Float32Array(data) => number_values!(data),
        Value::Int8Array(data) => number_values!(data),
        Value::Int16Array(data) => number_values!(data),
        Value::Int32Array(data) => number_values!(data),
        Value::Uint8Array(data) => number_values!(data),
        Value::Uint8ClampedArray(data) => number_values!(data),
        Value::Uint16Array(data) => number_values!(data),
        Value::Uint32Array(data) => number_values!(data),
        Value::BigInt64Array(data) => bigint_values!(data),
        Value::BigUint64Array(data) => bigint_values!(data),
        _ => Err(crate::collections::iterator::not_iterable()),
    }
}
fn checked_length(
    length: usize,
    byte_offset: usize,
    byte_length: usize,
    logical_len: usize,
    buffer: &crate::value::ArrayBufferData,
) -> Result<usize, crate::execute::VmError> {
    if length == usize::MAX {
        if *buffer.detached.borrow() {
            return Err(crate::collections::iterator::not_iterable());
        }
        if buffer.byte_length() < byte_offset {
            return Err(crate::collections::iterator::not_iterable());
        }
        return Ok(logical_len);
    }
    let required = byte_offset.saturating_add(byte_length);
    if *buffer.detached.borrow() || buffer.byte_length() < required {
        Err(crate::collections::iterator::not_iterable())
    } else {
        Ok(logical_len)
    }
}
pub(crate) fn typed_length(value: &Value) -> Result<usize, crate::execute::VmError> {
    if let Value::BindingCell(cell) = value {
        return typed_length(&cell.borrow());
    }
    macro_rules! length {
        ($data:expr) => {
            checked_length(
                $data.length,
                $data.byte_offset,
                $data.byte_length(),
                $data.logical_len(),
                &$data.buffer,
            )
        };
    }
    match value {
        Value::Float64Array(data) => length!(data),
        Value::Float32Array(data) => length!(data),
        Value::Int8Array(data) => length!(data),
        Value::Int16Array(data) => length!(data),
        Value::Int32Array(data) => length!(data),
        Value::Uint8Array(data) => length!(data),
        Value::Uint8ClampedArray(data) => length!(data),
        Value::Uint16Array(data) => length!(data),
        Value::Uint32Array(data) => length!(data),
        Value::BigInt64Array(data) => length!(data),
        Value::BigUint64Array(data) => length!(data),
        _ => Err(crate::collections::iterator::not_iterable()),
    }
}
fn collect_typed<T>(
    length: usize,
    mut get: impl FnMut(usize) -> Option<T>,
) -> Result<Vec<T>, crate::execute::VmError> {
    (0..length)
        .map(|index| get(index).ok_or_else(crate::collections::iterator::not_iterable))
        .collect()
}
