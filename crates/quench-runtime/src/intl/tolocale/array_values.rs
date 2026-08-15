use crate::value::Value;

pub(super) fn typed_values(value: &Value) -> Option<Vec<Value>> {
    macro_rules! values {
        ($data:expr) => {
            (0..$data.logical_len())
                .filter_map(|index| $data.get(index).map(|value| Value::Number(value as f64)))
                .collect()
        };
    }
    match value {
        Value::Float64Array(data) => Some(values!(data)),
        Value::Float32Array(data) => Some(values!(data)),
        Value::Int8Array(data) => Some(values!(data)),
        Value::Int16Array(data) => Some(values!(data)),
        Value::Int32Array(data) => Some(values!(data)),
        Value::Uint8Array(data) => Some(values!(data)),
        Value::Uint8ClampedArray(data) => Some(values!(data)),
        Value::Uint16Array(data) => Some(values!(data)),
        Value::Uint32Array(data) => Some(values!(data)),
        _ => None,
    }
}
