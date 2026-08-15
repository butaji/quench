use crate::value::Value;

macro_rules! number_values {
    ($data:expr) => {
        collect_typed(
            $data.logical_len(),
            $data.buffer.byte_length() < $data.byte_offset,
            |index| $data.get(index).map(|value| Value::Number(value.into())),
        )
    };
}
macro_rules! bigint_values {
    ($data:expr) => {
        collect_typed(
            $data.logical_len(),
            $data.buffer.byte_length() < $data.byte_offset,
            |index| {
                $data
                    .get(index)
                    .map(|value| Value::BigInt(value.to_string()))
            },
        )
    };
}
pub(crate) fn typed_values(value: Value) -> Result<Vec<Value>, crate::execute::VmError> {
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
fn collect_typed(
    length: usize,
    out_of_bounds: bool,
    mut get: impl FnMut(usize) -> Option<Value>,
) -> Result<Vec<Value>, crate::execute::VmError> {
    if out_of_bounds {
        return Err(crate::collections::iterator::not_iterable());
    }
    (0..length)
        .map(|index| get(index).ok_or_else(crate::collections::iterator::not_iterable))
        .collect()
}
