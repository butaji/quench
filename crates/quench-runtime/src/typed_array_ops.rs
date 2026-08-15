use crate::{execute::VmError, ops::Builtin, value::Value};

macro_rules! fill_view {
    ($view:expr, $value:expr, $convert:expr) => {{
        for index in 0..$view.length {
            $view.set(index, $convert($value));
        }
    }};
}

macro_rules! set_number_view {
    ($target:expr, $key:expr, $value:expr, $variant:ident, $convert:expr) => {
        if let Value::$variant(view) = $target {
            let index = typed_array_index($key)?;
            let number = crate::conversion::to_number($value);
            let number = match number {
                Ok(number) => number,
                Err(error) => return Some(Err(error)),
            };
            view.set(index, $convert(number));
            return Some(Ok($target.clone()));
        }
    };
}

fn typed_array_index(key: &str) -> Option<usize> {
    let index = key.parse::<usize>().ok()?;
    (index.to_string() == key).then_some(index)
}

pub(crate) fn is_view(value: &Value) -> bool {
    matches!(
        value,
        Value::DataView(_)
            | Value::Float64Array(_)
            | Value::Float32Array(_)
            | Value::Int8Array(_)
            | Value::Int16Array(_)
            | Value::Int32Array(_)
            | Value::BigInt64Array(_)
            | Value::BigUint64Array(_)
            | Value::Uint32Array(_)
            | Value::Uint8Array(_)
            | Value::Uint8ClampedArray(_)
            | Value::Uint16Array(_)
    )
}

pub(crate) fn set_property(
    target: &Value,
    key: &str,
    value: &Value,
) -> Option<Result<Value, VmError>> {
    if typed_array_index(key).is_none() {
        if let Some(result) = set_named_property(target, key, value.clone()) {
            return Some(Ok(result));
        }
    }
    if let Some(result) = set_number_property(target, key, value) {
        return Some(result);
    }
    set_bigint_property(target, key, value)
}

fn set_named_property(target: &Value, key: &str, value: Value) -> Option<Value> {
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

fn set_number_property(target: &Value, key: &str, value: &Value) -> Option<Result<Value, VmError>> {
    set_float_property(target, key, value)
        .or_else(|| set_signed_property(target, key, value))
        .or_else(|| set_unsigned_property(target, key, value))
        .or_else(|| set_clamped_property(target, key, value))
}

fn set_float_property(target: &Value, key: &str, value: &Value) -> Option<Result<Value, VmError>> {
    set_number_view!(target, key, value, Float64Array, |value| value);
    set_number_view!(target, key, value, Float32Array, |value| value as f32);
    None
}

fn set_signed_property(target: &Value, key: &str, value: &Value) -> Option<Result<Value, VmError>> {
    set_number_view!(target, key, value, Int8Array, crate::construct::to_int8);
    set_number_view!(target, key, value, Int16Array, crate::construct::to_int16);
    set_number_view!(target, key, value, Int32Array, crate::construct::to_int32);
    None
}

fn set_unsigned_property(
    target: &Value,
    key: &str,
    value: &Value,
) -> Option<Result<Value, VmError>> {
    set_number_view!(target, key, value, Uint8Array, crate::construct::to_uint8);
    set_number_view!(target, key, value, Uint16Array, crate::construct::to_uint16);
    set_number_view!(target, key, value, Uint32Array, crate::construct::to_uint32);
    None
}

fn set_clamped_property(
    target: &Value,
    key: &str,
    value: &Value,
) -> Option<Result<Value, VmError>> {
    set_number_view!(target, key, value, Uint8ClampedArray, |value| value);
    None
}

fn set_bigint_property(target: &Value, key: &str, value: &Value) -> Option<Result<Value, VmError>> {
    if !matches!(target, Value::BigInt64Array(_) | Value::BigUint64Array(_)) {
        return None;
    }
    let index = typed_array_index(key)?;
    let bits = match crate::construct::bigint_bits(value) {
        Ok(bits) => bits,
        Err(error) => return Some(Err(error)),
    };
    match target {
        Value::BigInt64Array(view) => view.set(index, bits as i64),
        Value::BigUint64Array(view) => view.set(index, bits),
        _ => return None,
    };
    Some(Ok(target.clone()))
}

pub(crate) fn execute(
    builtin: Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    match builtin {
        Builtin::TypedArrayFill => Some(fill(receiver, arguments)),
        Builtin::ArrayBufferResize => Some(resize_buffer(receiver, arguments)),
        Builtin::Uint8ArrayFromBase64
        | Builtin::Uint8ArrayFromHex
        | Builtin::Uint8ArraySetFromBase64
        | Builtin::Uint8ArraySetFromHex
        | Builtin::Uint8ArrayToBase64
        | Builtin::Uint8ArrayToHex
        | Builtin::Uint8ArraySubarray => {
            crate::typed_array_base64::execute(builtin, receiver, arguments)
        }
        Builtin::ArrayBufferTransferToImmutable => Some(transfer_to_immutable(receiver)),
        Builtin::ArrayBufferSliceToImmutable => Some(
            crate::vm::vm_builtin_shared_buffer::slice_to_immutable(receiver, arguments),
        ),
        _ => None,
    }
}

fn transfer_to_immutable(receiver: Option<&Value>) -> Result<Value, VmError> {
    let Some(Value::ArrayBuffer(buffer)) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "transferToImmutable requires an ArrayBuffer",
        ));
    };
    Ok(Value::ArrayBuffer(std::rc::Rc::new(
        buffer.transfer_to_immutable(),
    )))
}

fn resize_buffer(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::ArrayBuffer(buffer)) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "resize requires an ArrayBuffer",
        ));
    };
    if buffer.immutable {
        return Err(crate::value::error::throw_type_error(
            "Cannot resize an immutable ArrayBuffer",
        ));
    }
    let length = crate::intl::tolocale::value::to_number_result(arguments.first())?;
    let length = crate::construct::to_index(length)?;
    if *buffer.detached.borrow() {
        return Err(crate::value::error::throw_type_error(
            "Cannot resize a detached ArrayBuffer",
        ));
    }
    if buffer.max_byte_length.is_none() {
        return Err(crate::value::error::throw_type_error(
            "ArrayBuffer is not resizable",
        ));
    }
    buffer
        .resize(length)
        .map_err(|_| crate::value::error::throw_range_error("Invalid ArrayBuffer resize"))?;
    Ok(Value::Undefined)
}

fn fill(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let receiver = receiver.ok_or_else(crate::vm::not_callable)?;
    let number = crate::intl::tolocale::value::to_number_result(arguments.first())?;
    match receiver {
        Value::Float64Array(view) => fill_view!(view, number, |value| value),
        Value::Float32Array(view) => fill_view!(view, number, |value| value as f32),
        Value::Int8Array(view) => fill_view!(view, number, crate::construct::to_int8),
        Value::Int16Array(view) => fill_view!(view, number, crate::construct::to_int16),
        Value::Int32Array(view) => fill_view!(view, number, crate::construct::to_int32),
        Value::Uint8Array(view) => fill_view!(view, number, crate::construct::to_uint8),
        Value::Uint16Array(view) => fill_view!(view, number, crate::construct::to_uint16),
        Value::Uint32Array(view) => fill_view!(view, number, crate::construct::to_uint32),
        Value::Uint8ClampedArray(view) => fill_view!(view, number, |value| value),
        _ => return Err(crate::vm::not_callable()),
    }
    Ok(receiver.clone())
}
