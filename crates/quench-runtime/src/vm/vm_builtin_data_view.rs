fn execute_data_view_builtin(
    builtin: Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    if let Some(result) = typed_array_accessor(builtin, receiver) {
        return result;
    }
    let view = data_view_receiver(receiver)?;
    if let Some(result) = data_view_accessor(builtin, view) {
        return result;
    }
    if is_data_view_setter(builtin) && view.buffer.immutable {
        return Err(type_error("Cannot write to an immutable ArrayBuffer"));
    }
    let offset = data_view_offset(arguments.first())?;
    if !is_data_view_setter(builtin) && view.is_detached() {
        return Err(type_error("Detached DataView"));
    }
    let endian_argument = if is_data_view_setter(builtin) { 2 } else { 1 };
    let little_endian = arguments.get(endian_argument).is_some_and(is_truthy);
    if !is_data_view_setter(builtin) {
        return execute_data_view_get(builtin, view, offset, little_endian);
    }
    execute_data_view_set(builtin, view, offset, little_endian, arguments)
}

fn typed_array_accessor(
    builtin: Builtin,
    receiver: Option<&Value>,
) -> Option<Result<Value, VmError>> {
    macro_rules! access {
        ($view:expr) => {
            Some(Ok(match builtin {
                Builtin::TypedArrayByteLengthGetter => Value::Number(
                    if $view.buffer.byte_length() < $view.byte_offset + $view.byte_length() {
                        0.0
                    } else {
                        $view.byte_length() as f64
                    },
                ),
                _ => return None,
            }))
        };
    }
    match receiver {
        Some(Value::Float64Array(view)) => access!(view),
        Some(Value::Float32Array(view)) => access!(view),
        Some(Value::Int8Array(view)) => access!(view),
        Some(Value::Int16Array(view)) => access!(view),
        Some(Value::Int32Array(view)) => access!(view),
        Some(Value::Uint8Array(view)) => access!(view),
        Some(Value::Uint8ClampedArray(view)) => access!(view),
        Some(Value::Uint16Array(view)) => access!(view),
        Some(Value::Uint32Array(view)) => access!(view),
        Some(Value::BigInt64Array(view)) => access!(view),
        Some(Value::BigUint64Array(view)) => access!(view),
        _ => None,
    }
}
fn data_view_accessor(
    builtin: Builtin,
    view: &crate::value::DataViewData,
) -> Option<Result<Value, VmError>> {
    let detached = view.is_detached();
    Some(Ok(match builtin {
        Builtin::DataViewBufferGetter => Value::ArrayBuffer(view.buffer.clone()),
        Builtin::DataViewByteLengthGetter => {
            if detached || view.is_out_of_bounds() {
                return Some(Err(type_error("Detached DataView")));
            }
            Value::Number(view.byte_length() as f64)
        }
        Builtin::DataViewByteOffsetGetter => {
            if detached || view.is_out_of_bounds() {
                return Some(Err(type_error("Detached DataView")));
            }
            Value::Number(view.byte_offset as f64)
        }
        _ => return None,
    }))
}
fn data_view_receiver(receiver: Option<&Value>) -> Result<&crate::value::DataViewData, VmError> {
    match receiver {
        Some(Value::DataView(view)) => Ok(view),
        _ => Err(type_error(
            "DataView method called on incompatible receiver",
        )),
    }
}
fn execute_data_view_get(
    builtin: Builtin,
    view: &crate::value::DataViewData,
    offset: usize,
    little_endian: bool,
) -> Result<Value, VmError> {
    let result = match builtin {
        Builtin::DataViewGetInt8 => {
            Value::Number(view.get_int8(offset).map_err(data_view_error)? as f64)
        }
        Builtin::DataViewGetUint8 => {
            Value::Number(view.get_uint8(offset).map_err(data_view_error)? as f64)
        }
        _ => return execute_data_view_wide_get(builtin, view, offset, little_endian),
    };
    Ok(result)
}
fn execute_data_view_wide_get(
    builtin: Builtin,
    view: &crate::value::DataViewData,
    offset: usize,
    little_endian: bool,
) -> Result<Value, VmError> {
    let value = match builtin {
        Builtin::DataViewGetInt16 => view.get_int16(offset, little_endian).map(|v| v as f64),
        Builtin::DataViewGetUint16 => view.get_uint16(offset, little_endian).map(|v| v as f64),
        Builtin::DataViewGetInt32 => view.get_int32(offset, little_endian).map(|v| v as f64),
        Builtin::DataViewGetUint32 => view.get_uint32(offset, little_endian).map(|v| v as f64),
        Builtin::DataViewGetFloat16 => view.get_float16(offset, little_endian),
        Builtin::DataViewGetFloat32 => view.get_float32(offset, little_endian).map(|v| v as f64),
        Builtin::DataViewGetFloat64 => view.get_float64(offset, little_endian),
        Builtin::DataViewGetBigInt64 => {
            return view
                .get_bigint64(offset, little_endian)
                .map(|value| Value::BigInt(value.to_string()))
                .map_err(data_view_error);
        }
        Builtin::DataViewGetBigUint64 => {
            return view
                .get_biguint64(offset, little_endian)
                .map(|value| Value::BigInt(value.to_string()))
                .map_err(data_view_error);
        }
        _ => return Err(VmError::NotCallable),
    };
    value.map(Value::Number).map_err(data_view_error)
}
fn is_data_view_setter(builtin: Builtin) -> bool {
    matches!(
        builtin,
        Builtin::DataViewSetInt8
            | Builtin::DataViewSetUint8
            | Builtin::DataViewSetInt16
            | Builtin::DataViewSetUint16
            | Builtin::DataViewSetInt32
            | Builtin::DataViewSetUint32
            | Builtin::DataViewSetFloat16
            | Builtin::DataViewSetFloat32
            | Builtin::DataViewSetFloat64
            | Builtin::DataViewSetBigInt64
            | Builtin::DataViewSetBigUint64
    )
}
fn execute_data_view_set(
    builtin: Builtin,
    view: &crate::value::DataViewData,
    offset: usize,
    little_endian: bool,
    arguments: &[Value],
) -> Result<Value, VmError> {
    if matches!(
        builtin,
        Builtin::DataViewSetBigInt64 | Builtin::DataViewSetBigUint64
    ) {
        return execute_data_view_bigint_set(builtin, view, offset, little_endian, arguments);
    }
    let number = crate::intl::tolocale::value::to_number_result(arguments.get(1))?;
    let result = match builtin {
        Builtin::DataViewSetInt8 => view.set_int8(offset, to_i8(number)),
        Builtin::DataViewSetUint8 => view.set_uint8(offset, to_u8(number)),
        Builtin::DataViewSetInt16 => view.set_int16(offset, to_i16(number), little_endian),
        Builtin::DataViewSetUint16 => view.set_uint16(offset, to_u16(number), little_endian),
        Builtin::DataViewSetInt32 => view.set_int32(offset, to_i32(number), little_endian),
        Builtin::DataViewSetUint32 => view.set_uint32(offset, to_u32(number), little_endian),
        Builtin::DataViewSetFloat16 => view.set_float16(offset, number, little_endian),
        Builtin::DataViewSetFloat32 => view.set_float32(offset, number as f32, little_endian),
        Builtin::DataViewSetFloat64 => view.set_float64(offset, number, little_endian),
        _ => return Err(VmError::NotCallable),
    };
    result.map_err(data_view_error).map(|()| Value::Undefined)
}
fn execute_data_view_bigint_set(
    builtin: Builtin,
    view: &crate::value::DataViewData,
    offset: usize,
    little_endian: bool,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let input = arguments.get(1).unwrap_or(&Value::Undefined);
    if matches!(input, Value::Number(_)) {
        return Err(type_error("Cannot convert Number to BigInt"));
    }
    let value = explicit_bigint(Some(input))?;
    if view.is_detached() {
        return Err(type_error("Detached DataView"));
    }
    let bits = crate::construct::bigint_bits(&value)?;
    let result = match builtin {
        Builtin::DataViewSetBigInt64 => view.set_bigint64(offset, bits as i64, little_endian),
        Builtin::DataViewSetBigUint64 => view.set_biguint64(offset, bits, little_endian),
        _ => return Err(VmError::NotCallable),
    };
    result.map_err(data_view_error).map(|()| Value::Undefined)
}
fn data_view_offset(value: Option<&Value>) -> Result<usize, VmError> {
    let number = crate::intl::tolocale::value::to_number_result(value)?;
    if number.is_nan() {
        return Ok(0);
    }
    let index = number.trunc();
    if !index.is_finite() || index < 0.0 {
        return Err(range_error("Offset is outside the bounds of the DataView"));
    }
    Ok(index as usize)
}
fn data_view_error(error: crate::value::DataViewError) -> VmError {
    match error {
        crate::value::DataViewError::Detached => type_error("Detached DataView"),
        crate::value::DataViewError::ViewOutOfBounds => type_error("DataView is out of bounds"),
        crate::value::DataViewError::OutOfBounds => {
            range_error("Offset is outside the bounds of the DataView")
        }
    }
}
fn integer_modulo(value: f64, modulus: f64) -> u64 {
    if !value.is_finite() || value == 0.0 {
        return 0;
    }
    value.trunc().rem_euclid(modulus) as u64
}
fn to_u8(value: f64) -> u8 {
    integer_modulo(value, 256.0) as u8
}
fn to_i8(value: f64) -> i8 {
    let value = to_u8(value);
    if value >= 128 {
        (value as i16 - 256) as i8
    } else {
        value as i8
    }
}
fn to_u16(value: f64) -> u16 {
    integer_modulo(value, 65536.0) as u16
}
fn to_i16(value: f64) -> i16 {
    let value = to_u16(value);
    if value >= 32768 {
        (value as i32 - 65536) as i16
    } else {
        value as i16
    }
}
fn to_u32(value: f64) -> u32 {
    integer_modulo(value, 4294967296.0) as u32
}
fn to_i32(value: f64) -> i32 {
    let value = to_u32(value);
    if value >= 2147483648 {
        (value as i64 - 4294967296) as i32
    } else {
        value as i32
    }
}
fn type_error(message: &str) -> VmError {
    let arguments = [Value::String(message.to_string())];
    VmError::Thrown(crate::builtins::error(Builtin::TypeError, &arguments))
}
