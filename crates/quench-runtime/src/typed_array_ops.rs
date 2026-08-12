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
            let index = $key.parse::<usize>().ok()?;
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

pub(crate) fn set_property(
    target: &Value,
    key: &str,
    value: &Value,
) -> Option<Result<Value, VmError>> {
    set_number_view!(target, key, value, Float64Array, |value| value);
    set_number_view!(target, key, value, Float32Array, |value| value as f32);
    set_number_view!(target, key, value, Int8Array, crate::construct::to_int8);
    set_number_view!(target, key, value, Int16Array, crate::construct::to_int16);
    set_number_view!(target, key, value, Int32Array, crate::construct::to_int32);
    set_number_view!(target, key, value, Uint8Array, crate::construct::to_uint8);
    set_number_view!(target, key, value, Uint16Array, crate::construct::to_uint16);
    set_number_view!(target, key, value, Uint32Array, crate::construct::to_uint32);
    set_number_view!(target, key, value, Uint8ClampedArray, |value| value);
    set_bigint_property(target, key, value)
}

fn set_bigint_property(target: &Value, key: &str, value: &Value) -> Option<Result<Value, VmError>> {
    if !matches!(target, Value::BigInt64Array(_) | Value::BigUint64Array(_)) {
        return None;
    }
    let index = key.parse::<usize>().ok()?;
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
        _ => None,
    }
}

fn resize_buffer(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::ArrayBuffer(buffer)) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "resize requires an ArrayBuffer",
        ));
    };
    let length = crate::intl::tolocale::value::to_number_result(arguments.first())?;
    let length = crate::construct::to_index(length)?;
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
