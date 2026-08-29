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

pub(crate) fn is_out_of_bounds(value: &Value) -> bool {
    macro_rules! check {
        ($($variant:ident),+) => {
            match value {
                $(Value::$variant(data) => out_of_bounds(
                    data.length,
                    data.byte_offset,
                    data.byte_length(),
                    &data.buffer,
                ),)+
                _ => false,
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

fn out_of_bounds(
    length: usize,
    byte_offset: usize,
    byte_length: usize,
    buffer: &crate::value::ArrayBufferData,
) -> bool {
    let required = if length == usize::MAX {
        byte_offset
    } else {
        byte_offset.saturating_add(byte_length)
    };
    buffer.byte_length() < required
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

pub(crate) fn remove_own_property(value: &Value, key: &str) -> bool {
    macro_rules! remove {
        ($($variant:ident),+) => {
            match value {
                $(Value::$variant(data) => {
                    let existed = data.meta.property(key).is_some() || data.meta.descriptor(key).is_some();
                    data.meta.remove_property(key);
                    existed
                },)+
                _ => return false,
            }
        };
    }
    remove!(
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

pub(crate) fn descriptor(value: &Value, key: &str) -> Option<Value> {
    value.typed_array_meta()?.descriptor(key)
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

/// Resolve a runtime override of the concrete typed-array prototype's
/// `constructor` property while preserving the instance as accessor receiver.
pub(crate) fn constructor_override(value: &Value) -> Option<Value> {
    let prototype = get(value).or_else(|| {
        let builtin = match value {
            Value::Float64Array(_) => crate::ops::Builtin::Float64ArrayPrototype,
            Value::Float32Array(_) => crate::ops::Builtin::Float32ArrayPrototype,
            Value::Int8Array(_) => crate::ops::Builtin::Int8ArrayPrototype,
            Value::Int16Array(_) => crate::ops::Builtin::Int16ArrayPrototype,
            Value::Uint16Array(_) => crate::ops::Builtin::Uint16ArrayPrototype,
            Value::Int32Array(_) => crate::ops::Builtin::Int32ArrayPrototype,
            Value::Uint32Array(_) => crate::ops::Builtin::Uint32ArrayPrototype,
            Value::BigInt64Array(_) => crate::ops::Builtin::BigInt64ArrayPrototype,
            Value::BigUint64Array(_) => crate::ops::Builtin::BigUint64ArrayPrototype,
            Value::Uint8Array(_) => crate::ops::Builtin::Uint8ArrayPrototype,
            Value::Uint8ClampedArray(_) => crate::ops::Builtin::Uint8ClampedArrayPrototype,
            _ => return None,
        };
        Some(crate::vm::realm_intrinsic(builtin))
    })?;
    match prototype {
        Value::Builtin(builtin)
            if crate::builtins::read_intrinsic_override(builtin, "constructor").is_some() =>
        {
            Some(
                crate::vm::intrinsic_override_property(builtin, "constructor", value)
                    .unwrap_or(Value::Undefined),
            )
        }
        Value::BoundFunction(bound) => {
            let descriptor_key = crate::builtins::descriptor_key("constructor");
            let descriptor = bound
                .properties
                .borrow()
                .iter()
                .rev()
                .find_map(|(name, value)| (name == &descriptor_key).then_some(value.clone()))?;
            let Value::Object(fields) = descriptor else {
                return Some(Value::Undefined);
            };
            if let Some(getter) = fields
                .iter()
                .rev()
                .find_map(|(name, value)| (name == "get").then_some(value.clone()))
            {
                return Some(match getter {
                    Value::Undefined => Value::Undefined,
                    getter => crate::functions::execute_target(&getter, value, &[])
                        .ok()
                        .unwrap_or(Value::Undefined),
                });
            }
            let value = fields
                .iter()
                .rev()
                .find_map(|(name, value)| (name == "value").then_some(value.clone()));
            value
        }
        Value::Object(_) | Value::ObjectAlias(_) => {
            crate::execute::get_property_result(&prototype, "constructor").ok()
        }
        _ => None,
    }
}
