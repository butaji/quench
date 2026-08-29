use crate::{execute::VmError, ops::Builtin, value::Value};

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

pub(crate) fn typed_array_index(key: &str) -> Option<usize> {
    if key.is_empty() || (key.len() > 1 && key.as_bytes()[0] == b'0') {
        return None;
    }
    let mut index = 0usize;
    for byte in key.bytes() {
        if !byte.is_ascii_digit() {
            return None;
        }
        index = index
            .checked_mul(10)?
            .checked_add(usize::from(byte - b'0'))?;
    }
    Some(index)
}

/// Classify CanonicalNumericIndexString keys before ordinary prototype lookup.
/// Invalid numeric indices (for example `-1` or `1.1`) are still handled by
/// the integer-indexed exotic object and must not expose inherited properties.
pub(crate) fn canonical_numeric_index(key: &str) -> bool {
    if key == "-0" {
        return true;
    }
    if matches!(key, "NaN" | "Infinity" | "-Infinity") {
        return true;
    }
    if key.starts_with('+')
        || key
            .as_bytes()
            .iter()
            .any(|byte| matches!(byte, b'e' | b'E'))
    {
        return false;
    }
    let Ok(number) = key.parse::<f64>() else {
        return false;
    };
    if !number.is_finite() || number.abs() >= 1e21 {
        return false;
    }
    if key.contains('.') {
        number.abs() >= 1e-6 && !key.ends_with('0') && !key.ends_with('.')
    } else {
        key == "0" || !key.trim_start_matches('-').starts_with('0')
    }
}

pub(crate) fn is_index_key(key: &str) -> bool {
    typed_array_index(key).is_some()
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

pub(crate) fn logical_len(value: &Value) -> Option<usize> {
    macro_rules! length {
        ($($variant:ident),+ $(,)?) => {
            match value {
                $(Value::$variant(data) => Some(data.logical_len()),)+
                _ => None,
            }
        };
    }
    length!(
        Float64Array,
        Float32Array,
        Int8Array,
        Int16Array,
        Int32Array,
        BigInt64Array,
        BigUint64Array,
        Uint32Array,
        Uint8Array,
        Uint8ClampedArray,
        Uint16Array
    )
}

pub(crate) fn set_property(
    target: &Value,
    key: &str,
    value: &Value,
) -> Option<Result<Value, VmError>> {
    if target.typed_array_meta().is_none() {
        return None;
    }
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
    if canonical_numeric_index(key) && typed_array_index(key).is_none() {
        return Some(Ok(target.clone()));
    }
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
    if canonical_numeric_index(key) && typed_array_index(key).is_none() {
        return Some(Ok(target.clone()));
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
        Builtin::TypedArraySet => Some(set(receiver, arguments)),
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
        Builtin::ArrayBufferTransfer | Builtin::ArrayBufferTransferToFixedLength => Some(transfer(
            receiver,
            arguments,
            builtin == Builtin::ArrayBufferTransfer,
        )),
        Builtin::ArrayBufferSliceToImmutable => {
            Some(crate::vm::slice_to_immutable(receiver, arguments))
        }
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
            "transferToImmutable requires an ArrayBuffer",
        ));
    }
    let new_length = match arguments.first() {
        None | Some(Value::Undefined) => buffer.byte_length(),
        Some(value) => crate::construct::to_index(crate::intl::tolocale::value::to_number_result(
            Some(value),
        )?)?,
    };
    if buffer.immutable || *buffer.detached.borrow() {
        return Err(crate::value::error::throw_type_error(
            "ArrayBuffer is not transferable",
        ));
    }
    let bytes = buffer.bytes.borrow().clone();
    let Some(mut result) = crate::value::ArrayBufferData::try_new(new_length) else {
        return Err(crate::value::error::throw_range_error(
            "ArrayBuffer length is too large",
        ));
    };
    let copy_length = bytes.len().min(new_length);
    result.bytes.borrow_mut()[..copy_length].copy_from_slice(&bytes[..copy_length]);
    result.immutable = true;
    buffer.detach();
    Ok(Value::ArrayBuffer(std::rc::Rc::new(result)))
}

fn transfer(
    receiver: Option<&Value>,
    arguments: &[Value],
    preserve_resizability: bool,
) -> Result<Value, VmError> {
    let Some(Value::ArrayBuffer(buffer)) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "transfer requires an ArrayBuffer",
        ));
    };
    let new_length = match arguments.first() {
        None | Some(Value::Undefined) => buffer.byte_length(),
        Some(value) => crate::construct::to_index(crate::intl::tolocale::value::to_number_result(
            Some(value),
        )?)?,
    };
    if buffer.shared || buffer.immutable || buffer.untransferable || *buffer.detached.borrow() {
        return Err(crate::value::error::throw_type_error(
            "ArrayBuffer is not transferable",
        ));
    }
    let result = if preserve_resizability {
        match buffer.max_byte_length {
            Some(max) if new_length <= max => {
                crate::value::ArrayBufferData::try_new_resizable(new_length, max)
            }
            Some(_) => None,
            None => crate::value::ArrayBufferData::try_new(new_length),
        }
    } else {
        crate::value::ArrayBufferData::try_new(new_length)
    };
    let Some(result) = result else {
        return Err(crate::value::error::throw_range_error(
            "ArrayBuffer length is too large",
        ));
    };
    let bytes = buffer.bytes.borrow().clone();
    let copy_length = bytes.len().min(new_length);
    result.bytes.borrow_mut()[..copy_length].copy_from_slice(&bytes[..copy_length]);
    buffer.detach();
    Ok(Value::ArrayBuffer(std::rc::Rc::new(result)))
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

fn set_offset(arguments: &[Value]) -> Result<usize, VmError> {
    let Some(value) = arguments.get(1) else {
        return Ok(0);
    };
    if matches!(value, Value::Undefined) {
        return Ok(0);
    }
    let number = crate::intl::tolocale::value::to_number_result(Some(value))?;
    if number.is_nan() {
        return Ok(0);
    }
    let integer = number.trunc();
    if !number.is_finite() || integer < 0.0 || integer > usize::MAX as f64 {
        return Err(crate::value::error::throw_range_error(
            "offset is out of bounds",
        ));
    }
    Ok(integer as usize)
}

fn set(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let target = crate::arrays::typed_array_receiver(receiver, "set")?;
    if crate::arrays::typed_array_is_immutable(&target) {
        return Err(crate::value::error::throw_type_error(
            "TypedArray.prototype.set called on immutable buffer",
        ));
    }
    let target_length = logical_len(&target).ok_or_else(|| {
        crate::value::error::throw_type_error(
            "TypedArray.prototype.set called on incompatible receiver",
        )
    })?;
    let source = arguments.first().cloned().unwrap_or(Value::Undefined);
    let offset = set_offset(arguments)?;
    if crate::arrays::typed_array_is_detached(&target)
        || crate::typed_array_ops::is_view(&source)
            && (crate::arrays::typed_array_is_detached(&source)
                || crate::typed_array_prototype::is_out_of_bounds(&source))
    {
        return Err(crate::value::error::throw_type_error(
            "TypedArray.prototype.set called on detached buffer",
        ));
    }
    let source_length = if crate::conversion::is_symbol(&source) {
        0
    } else if let Some(length) = logical_len(&source) {
        length
    } else {
        let source_length = crate::intl::tolocale::value::to_number_result(Some(
            &crate::execute::get_property_result(&source, "length")?,
        ))?;
        source_length.max(0.0) as usize
    };
    if offset > target_length || source_length > target_length - offset {
        return Err(crate::value::error::throw_range_error(
            "offset is out of bounds",
        ));
    }
    if logical_len(&source).is_some() {
        // Typed-array sources may overlap the target, so snapshot them before
        // the first write.
        let values = (0..source_length)
            .map(|index| crate::execute::get_property_result(&source, &index.to_string()))
            .collect::<Result<Vec<_>, _>>()?;
        for (index, value) in values.iter().enumerate() {
            if let Some(result) = set_property(&target, &(offset + index).to_string(), value) {
                result?;
            }
        }
    } else {
        // Ordinary array-like sources are read and written in lockstep; the
        // ordering is observable through source getters.
        for index in 0..source_length {
            let value = crate::execute::get_property_result(&source, &index.to_string())?;
            if let Some(result) = set_property(&target, &(offset + index).to_string(), &value) {
                result?;
            }
        }
    }
    Ok(Value::Undefined)
}

fn fill(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let receiver = crate::arrays::typed_array_receiver(receiver, "fill")?;
    if crate::arrays::typed_array_is_immutable(&receiver) {
        return Err(crate::value::error::throw_type_error(
            "TypedArray.prototype.fill called on immutable buffer",
        ));
    }
    let length = logical_len(&receiver).unwrap_or(0);
    if matches!(receiver, Value::BigInt64Array(_) | Value::BigUint64Array(_)) {
        let bits = crate::construct::bigint_bits(
            arguments.first().unwrap_or(&Value::BigInt("0".to_string())),
        )?;
        let start = fill_index(arguments.get(1), length, 0)?;
        let end = fill_index(
            arguments
                .get(2)
                .filter(|value| !matches!(value, Value::Undefined)),
            length,
            length,
        )?;
        if crate::arrays::typed_array_is_detached(&receiver)
            || crate::typed_array_prototype::is_out_of_bounds(&receiver)
        {
            return Err(crate::value::error::throw_type_error(
                "TypedArray.prototype.fill called on detached buffer",
            ));
        }
        match &receiver {
            Value::BigInt64Array(view) => {
                for index in start..end {
                    view.set(index, bits as i64);
                }
            }
            Value::BigUint64Array(view) => {
                for index in start..end {
                    view.set(index, bits);
                }
            }
            _ => unreachable!(),
        }
        return Ok(receiver.clone());
    }
    let number = crate::intl::tolocale::value::to_number_result(arguments.first())?;
    let start = fill_index(arguments.get(1), length, 0)?;
    let end = fill_index(
        arguments
            .get(2)
            .filter(|value| !matches!(value, Value::Undefined)),
        length,
        length,
    )?;
    if crate::arrays::typed_array_is_detached(&receiver)
        || crate::typed_array_prototype::is_out_of_bounds(&receiver)
    {
        return Err(crate::value::error::throw_type_error(
            "TypedArray.prototype.fill called on detached buffer",
        ));
    }
    match &receiver {
        Value::Float64Array(view) => {
            for index in start..end {
                view.set(index, number);
            }
        }
        Value::Float32Array(view) => {
            for index in start..end {
                view.set(index, number as f32);
            }
        }
        Value::Int8Array(view) => {
            for index in start..end {
                view.set(index, crate::construct::to_int8(number));
            }
        }
        Value::Int16Array(view) => {
            for index in start..end {
                view.set(index, crate::construct::to_int16(number));
            }
        }
        Value::Int32Array(view) => {
            for index in start..end {
                view.set(index, crate::construct::to_int32(number));
            }
        }
        Value::Uint8Array(view) => {
            for index in start..end {
                view.set(index, crate::construct::to_uint8(number));
            }
        }
        Value::Uint16Array(view) => {
            for index in start..end {
                view.set(index, crate::construct::to_uint16(number));
            }
        }
        Value::Uint32Array(view) => {
            for index in start..end {
                view.set(index, crate::construct::to_uint32(number));
            }
        }
        Value::Uint8ClampedArray(view) => {
            for index in start..end {
                view.set(index, number);
            }
        }
        _ => {
            return Err(crate::value::error::throw_type_error(
                "TypedArray.prototype.fill called on incompatible receiver",
            ))
        }
    }
    Ok(receiver.clone())
}

fn fill_index(value: Option<&Value>, length: usize, default: usize) -> Result<usize, VmError> {
    let Some(value) = value else {
        return Ok(default);
    };
    let number = crate::conversion::to_number(value)?;
    if number.is_nan() || number == 0.0 || number == f64::NEG_INFINITY {
        return Ok(0);
    }
    if number == f64::INFINITY {
        return Ok(length);
    }
    if number.is_sign_negative() {
        return Ok(length.saturating_sub(number.abs().trunc() as usize));
    }
    Ok((number.trunc() as usize).min(length))
}

#[cfg(test)]
mod tests {
    use super::set_offset;
    use crate::value::Value;

    #[test]
    fn set_offset_rejects_non_finite_values() {
        let error = set_offset(&[Value::Undefined, Value::Number(f64::INFINITY)])
            .expect_err("a non-finite offset must be rejected");
        assert!(matches!(error, crate::vm::VmError::Thrown(_)));
    }
}
