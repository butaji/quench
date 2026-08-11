use crate::value::Value;

pub(crate) fn set(target: &Value, key: &str, value: Value) -> Option<Value> {
    if key != "\0prototype" {
        return None;
    }
    match target {
        Value::Float64Array(data) => data.set_prototype(value),
        Value::Float32Array(data) => data.set_prototype(value),
        Value::Int8Array(data) => data.set_prototype(value),
        Value::Int16Array(data) => data.set_prototype(value),
        Value::Uint16Array(data) => data.set_prototype(value),
        Value::Int32Array(data) => data.set_prototype(value),
        Value::Uint32Array(data) => data.set_prototype(value),
        Value::BigInt64Array(data) => data.set_prototype(value),
        Value::BigUint64Array(data) => data.set_prototype(value),
        Value::Uint8Array(data) => data.set_prototype(value),
        Value::Uint8ClampedArray(data) => data.set_prototype(value),
        _ => return None,
    }
    Some(target.clone())
}

pub(crate) fn get(value: &Value) -> Option<Value> {
    match value {
        Value::Float64Array(data) => data.prototype(),
        Value::Float32Array(data) => data.prototype(),
        Value::Int8Array(data) => data.prototype(),
        Value::Int16Array(data) => data.prototype(),
        Value::Uint16Array(data) => data.prototype(),
        Value::Int32Array(data) => data.prototype(),
        Value::Uint32Array(data) => data.prototype(),
        Value::BigInt64Array(data) => data.prototype(),
        Value::BigUint64Array(data) => data.prototype(),
        Value::Uint8Array(data) => data.prototype(),
        Value::Uint8ClampedArray(data) => data.prototype(),
        _ => None,
    }
}
