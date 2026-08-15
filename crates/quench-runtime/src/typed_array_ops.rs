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
        Builtin::ArrayBufferTransferToImmutable => Some(transfer_to_immutable(receiver, arguments)),
        Builtin::ArrayBufferTransfer => Some(transfer_buffer(receiver, arguments)),
        Builtin::ArrayBufferSlice => Some(slice_buffer(receiver, arguments)),
        _ => None,
    }
}

fn transfer_to_immutable(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::ArrayBuffer(buffer)) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "transferToImmutable requires an ArrayBuffer",
        ));
    };
    if buffer.shared {
        return Err(crate::value::error::throw_type_error(
            "Cannot transfer this ArrayBuffer",
        ));
    }
    let length = match arguments.first() {
        Some(value) if !matches!(value, Value::Undefined) => {
            crate::construct::to_index(crate::conversion::to_number(value)?)?
        }
        _ => buffer.byte_length(),
    };
    if *buffer.detached.borrow() {
        return Err(crate::value::error::throw_type_error(
            "Cannot transfer this ArrayBuffer",
        ));
    }
    if buffer.immutable {
        return Err(crate::value::error::throw_type_error(
            "Cannot transfer an immutable ArrayBuffer",
        ));
    }
    let result = buffer.transfer_to_immutable();
    result.bytes.borrow_mut().resize(length, 0);
    buffer.detach();
    Ok(Value::ArrayBuffer(std::rc::Rc::new(result)))
}

fn transfer_buffer(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::ArrayBuffer(buffer)) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "transfer requires an ArrayBuffer",
        ));
    };
    if buffer.shared {
        return Err(crate::value::error::throw_type_error(
            "Cannot transfer a SharedArrayBuffer",
        ));
    }
    if *buffer.detached.borrow() {
        return Err(crate::value::error::throw_type_error(
            "Cannot transfer a detached ArrayBuffer",
        ));
    }
    let length = match arguments.first() {
        Some(value) if !matches!(value, Value::Undefined) => {
            crate::construct::to_index(crate::conversion::to_number(value)?)?
        }
        _ => buffer.byte_length(),
    };
    let result = buffer.max_byte_length.map_or_else(
        || crate::value::ArrayBufferData::new(length),
        |max| crate::value::ArrayBufferData::new_resizable(length, max),
    );
    let copy_length = length.min(buffer.byte_length());
    result.bytes.borrow_mut()[..copy_length].copy_from_slice(&buffer.bytes.borrow()[..copy_length]);
    buffer.detach();
    Ok(Value::ArrayBuffer(std::rc::Rc::new(result)))
}

fn slice_buffer(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::ArrayBuffer(buffer)) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "ArrayBuffer.prototype.slice requires an ArrayBuffer",
        ));
    };
    if buffer.shared {
        return Err(crate::value::error::throw_type_error(
            "ArrayBuffer.prototype.slice cannot be called on a SharedArrayBuffer",
        ));
    }
    if *buffer.detached.borrow() {
        return Err(crate::value::error::throw_type_error(
            "Cannot slice a detached ArrayBuffer",
        ));
    }
    let length = buffer.byte_length() as isize;
    let start = slice_index(arguments.first(), length, 0)?;
    let end = slice_index(arguments.get(1), length, length)?;
    let result_length = (end - start).max(0) as usize;
    let result = slice_result(buffer, result_length)?;
    if matches!(&result, Value::ArrayBuffer(candidate) if std::rc::Rc::ptr_eq(candidate, buffer)) {
        return Err(crate::value::error::throw_type_error(
            "ArrayBuffer species returned the source buffer",
        ));
    }
    if end <= start {
        return Ok(result);
    }
    let Value::ArrayBuffer(result) = &result else {
        return Err(crate::value::error::throw_type_error(
            "ArrayBuffer species must return an ArrayBuffer",
        ));
    };
    if result_length > result.byte_length() {
        return Err(crate::value::error::throw_type_error(
            "ArrayBuffer species result is too small",
        ));
    }
    let source_bytes = buffer.bytes.borrow();
    if end as usize > source_bytes.len() || start < 0 || start > end {
        return Err(crate::value::error::throw_type_error(
            "ArrayBuffer slice range is invalid",
        ));
    }
    let source = source_bytes[start as usize..end as usize].to_vec();
    drop(source_bytes);
    result.bytes.borrow_mut()[..result_length].copy_from_slice(&source);
    Ok(Value::ArrayBuffer(result.clone()))
}

fn slice_result(
    buffer: &std::rc::Rc<crate::value::ArrayBufferData>,
    length: usize,
) -> Result<Value, VmError> {
    let receiver = Value::ArrayBuffer(buffer.clone());
    let constructor = crate::execute::get_property_result(&receiver, "constructor")?;
    if matches!(constructor, Value::Undefined) {
        return default_slice_result(length);
    }
    if matches!(constructor, Value::Null) || !crate::value::is_object(&constructor) {
        return Err(crate::value::error::throw_type_error(
            "ArrayBuffer constructor is not an object",
        ));
    }
    let species = crate::execute::get_property_result(&constructor, "Symbol.species")?;
    if matches!(species, Value::Undefined | Value::Null) {
        return default_slice_result(length);
    }
    if !slice_species_constructible(&species) {
        return Err(crate::value::error::throw_type_error(
            "ArrayBuffer species is not a constructor",
        ));
    }
    let result = crate::construct::construct_value(&species, &[Value::Number(length as f64)])?;
    let Value::ArrayBuffer(result_buffer) = &result else {
        return Err(crate::value::error::throw_type_error(
            "ArrayBuffer species must return an ArrayBuffer",
        ));
    };
    if result_buffer.immutable || result_buffer.byte_length() < length {
        return Err(crate::value::error::throw_type_error(
            "ArrayBuffer species result is invalid",
        ));
    }
    Ok(result)
}

fn slice_species_constructible(value: &Value) -> bool {
    match value {
        Value::Function(function) => crate::functions::is_constructible(function),
        Value::BoundFunction(bound) => slice_species_constructible(&bound.target),
        Value::Builtin(_) => crate::conversion::is_callable(value),
        _ => false,
    }
}

fn default_slice_result(length: usize) -> Result<Value, VmError> {
    crate::construct::construct_value(
        &Value::Builtin(Builtin::ArrayBuffer),
        &[Value::Number(length as f64)],
    )
}

fn slice_index(value: Option<&Value>, length: isize, default: isize) -> Result<isize, VmError> {
    if value.is_none() || matches!(value, Some(Value::Undefined)) {
        return Ok(default);
    }
    let number = crate::intl::tolocale::value::to_number_result(value)?;
    if number.is_nan() {
        return Ok(0);
    }
    if number.is_infinite() {
        return Ok(if number.is_sign_positive() { length } else { 0 });
    }
    let index = number as isize;
    Ok(if index < 0 {
        (length + index).max(0)
    } else {
        index.min(length)
    })
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
    if buffer.max_byte_length.is_none() {
        return Err(crate::value::error::throw_type_error(
            "Cannot resize a non-resizable ArrayBuffer",
        ));
    }
    let length = crate::intl::tolocale::value::to_number_result(arguments.first())?;
    let length = crate::construct::to_index(length)?;
    if buffer.immutable {
        return Err(crate::value::error::throw_type_error(
            "Cannot resize an immutable ArrayBuffer",
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
