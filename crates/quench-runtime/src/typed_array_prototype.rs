use crate::value::Value;

pub(crate) fn set(target: &Value, key: &str, value: Value) -> Option<Value> {
    if key != "\0prototype" {
        if key.parse::<usize>().is_err() {
            return set_named(target, key, value);
        }
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

fn set_named(target: &Value, key: &str, value: Value) -> Option<Value> {
    macro_rules! store {
        ($($variant:ident),+) => {
            match target {
                $(Value::$variant(data) => data.meta.set_property(key, value),)+
                _ => return None,
            }
        };
    }
    store!(
        Float64Array,
        Float32Array,
        Int8Array,
        Int16Array,
        Uint16Array,
        Int32Array,
        Uint32Array,
        BigInt64Array,
        BigUint64Array,
        Uint8Array,
        Uint8ClampedArray
    );
    Some(target.clone())
}

/// Whether a typed-array index is in bounds.
pub(crate) fn index_exists(value: &Value, index: usize) -> bool {
    macro_rules! check {
        ($($variant:ident),+) => {
            match value {
                $(Value::$variant(data) => data.get(index).is_some(),)+
                _ => return false,
            }
        };
    }
    check!(
        Float64Array,
        Float32Array,
        Int8Array,
        Int16Array,
        Uint16Array,
        Int32Array,
        Uint32Array,
        BigInt64Array,
        BigUint64Array,
        Uint8Array,
        Uint8ClampedArray
    )
}

/// An own named property stored on a typed-array instance, if present.
pub(crate) fn own_property(value: &Value, key: &str) -> Option<Value> {
    macro_rules! lookup {
        ($($variant:ident),+) => {
            match value {
                $(Value::$variant(data) => data.meta.property(key),)+
                _ => return None,
            }
        };
    }
    lookup!(
        Float64Array,
        Float32Array,
        Int8Array,
        Int16Array,
        Uint16Array,
        Int32Array,
        Uint32Array,
        BigInt64Array,
        BigUint64Array,
        Uint8Array,
        Uint8ClampedArray
    )
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
