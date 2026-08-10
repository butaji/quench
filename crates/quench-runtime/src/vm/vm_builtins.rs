fn early_dispatch(
    builtin: Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    crate::intl::tolocale::symbol::dispatch(builtin, arguments, receiver)
        .or_else(|| crate::json::execute(builtin, arguments))
        .or_else(|| crate::typed_array_ops::execute(builtin, receiver, arguments))
        .or_else(|| crate::arrays::execute_builtin(builtin, receiver, arguments))
        .or_else(|| crate::intl::tolocale::dispatch(builtin, receiver, arguments))
        .or_else(|| crate::collections::execute_builtin(builtin, receiver, arguments))
        .or_else(|| crate::promise::execute_builtin(builtin, receiver, arguments))
        .or_else(|| crate::date::execute(builtin, receiver, arguments))
}

fn is_function_builtin(builtin: Builtin) -> bool {
    matches!(
        builtin,
        Builtin::FunctionCall
            | Builtin::FunctionApply
            | Builtin::FunctionBind
            | Builtin::ArrayJoin
            | Builtin::ArrayPush
    )
}

pub(crate) fn execute_function_apply(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let target = receiver.filter(|value| crate::conversion::is_callable(value));
    let target = target.ok_or_else(|| {
        crate::value::error::throw_type_error("Function.prototype.apply called on non-callable")
    })?;
    let receiver = arguments.first().unwrap_or(&Value::Undefined);
    let list = create_list_from_array_like(arguments.get(1))?;
    crate::functions::execute_target(target, receiver, &list)
}

fn create_list_from_array_like(value: Option<&Value>) -> Result<Vec<Value>, VmError> {
    let Some(value) = value.filter(|value| !matches!(value, Value::Null | Value::Undefined)) else {
        return Ok(Vec::new());
    };
    if !crate::value::is_object(value) {
        return Err(crate::value::error::throw_type_error(
            "Function.prototype.apply requires an object argument list",
        ));
    }
    let length = crate::execute::get_property_result(value, "length")?;
    let length = array_like_length(&length)?;
    (0..length)
        .map(|index| crate::execute::get_property_result(value, &index.to_string()))
        .collect()
}

fn array_like_length(value: &Value) -> Result<usize, VmError> {
    let number = crate::conversion::to_number(value)?;
    if number.is_nan() || number <= 0.0 {
        return Ok(0);
    }
    const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
    Ok(number.floor().min(MAX_SAFE_INTEGER).min(usize::MAX as f64) as usize)
}

fn is_simple_builtin(builtin: Builtin) -> bool {
    matches!(
        builtin,
        Builtin::Boolean
            | Builtin::Eval
            | Builtin::Escape
            | Builtin::IsFinite
            | Builtin::IsNaN
            | Builtin::Number
            | Builtin::BigInt
            | Builtin::NumberToString
            | Builtin::NumberValueOf
            | Builtin::BoxedValueOf
            | Builtin::ObjectPrototypeToString
            | Builtin::ObjectPrototypeValueOf
            | Builtin::FunctionPrototypeToString
            | Builtin::FunctionPrototypeValueOf
            | Builtin::RegExpPrototypeToString
            | Builtin::Function
            | Builtin::AsyncFunction
            | Builtin::GeneratorFunction
            | Builtin::AsyncGeneratorFunction
            | Builtin::NumberToFixed
            | Builtin::NumberToPrecision
            | Builtin::NumberToExponential
            | Builtin::ArrayBufferIsView
            | Builtin::Object
            | Builtin::Date
            | Builtin::Error
            | Builtin::RangeError
            | Builtin::ReferenceError
            | Builtin::SyntaxError
            | Builtin::EvalError
            | Builtin::URIError
            | Builtin::AggregateError
            | Builtin::TypeError
    )
}

fn execute_simple_builtin(
    builtin: Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Result<Value, VmError> {
    if is_error_constructor(builtin) {
        return Ok(crate::builtins::error(builtin, arguments));
    }
    if let Some(result) = crate::functions_dynamic::construct_builtin(builtin, arguments) {
        return result;
    }
    match builtin {
        Builtin::Boolean => Ok(Value::Boolean(arguments.first().is_some_and(is_truthy))),
        Builtin::Eval => crate::reflect::builtin(builtin, arguments, receiver),
        Builtin::Escape => Ok(crate::builtins::escape(arguments.first())),
        Builtin::IsFinite => Ok(Value::Boolean(is_finite(arguments.first()))),
        Builtin::IsNaN => Ok(Value::Boolean(to_number(arguments.first()).is_nan())),
        Builtin::Number => Ok(Value::Number(explicit_number(arguments.first())?)),
        Builtin::BigInt => explicit_bigint(arguments.first()),
        Builtin::ArrayBufferIsView => Ok(Value::Boolean(is_array_buffer_view(arguments.first()))),
        Builtin::NumberToString => Ok(Value::String(to_string(arguments.first()))),
        Builtin::NumberValueOf => Ok(Value::Number(to_number(arguments.first()))),
        Builtin::BoxedValueOf => Ok(boxed_value(receiver)),
        Builtin::ObjectPrototypeToString => Ok(crate::builtins::prototype_to_string(receiver)),
        Builtin::ObjectPrototypeValueOf => Ok(crate::builtins::prototype_value_of(receiver)),
        Builtin::FunctionPrototypeToString | Builtin::FunctionPrototypeValueOf => {
            function_prototype_builtin(builtin, receiver)
        }
        Builtin::RegExpPrototypeToString => regexp_prototype_to_string(receiver),
        Builtin::NumberToFixed | Builtin::NumberToPrecision | Builtin::NumberToExponential => {
            crate::number_fmt::number_format(arguments.first(), arguments.get(1), builtin)
        }
        Builtin::Object => Ok(crate::builtins::object(arguments)),
        Builtin::Date => {
            crate::date::execute(builtin, receiver, arguments).unwrap_or(Ok(Value::Undefined))
        }
        _ => Ok(Value::Undefined),
    }
}

pub(crate) fn realm_id_for_intrinsic_receiver(receiver: Option<&Value>) -> Option<RealmId> {
    let Some(Value::HostCapability(token)) = receiver else {
        return None;
    };
    realm::id_for_token(token)
}

fn explicit_number(value: Option<&Value>) -> Result<f64, VmError> {
    if let Some(Value::BigInt(value)) = value {
        return Ok(value.parse().unwrap_or(f64::NAN));
    }
    crate::intl::tolocale::value::to_number_result(value)
}

fn explicit_bigint(value: Option<&Value>) -> Result<Value, VmError> {
    let raw = match value {
        Some(Value::BigInt(value)) => return Ok(Value::BigInt(value.clone())),
        Some(Value::Number(value)) if value.is_finite() && value.fract() == 0.0 => {
            format!("{value:.0}")
        }
        Some(Value::String(value)) => value.clone(),
        _ => return Err(crate::value::error::throw_type_error("Cannot convert value to BigInt")),
    };
    let value = raw
        .parse::<num_bigint::BigInt>()
        .map_err(|_| crate::value::error::throw_type_error("Invalid BigInt value"))?;
    Ok(Value::BigInt(value.to_string()))
}

fn boxed_value(receiver: Option<&Value>) -> Value {
    receiver.map_or(Value::Undefined, |value| match value {
        Value::Object(_) => crate::execute::get_property(value, "_value"),
        _ => value.clone(),
    })
}

fn is_error_constructor(builtin: Builtin) -> bool {
    matches!(
        builtin,
        Builtin::Error
            | Builtin::RangeError
            | Builtin::ReferenceError
            | Builtin::SyntaxError
            | Builtin::EvalError
            | Builtin::URIError
            | Builtin::AggregateError
            | Builtin::TypeError
    )
}

fn is_array_buffer_view(value: Option<&Value>) -> bool {
    matches!(
        value,
        Some(
            Value::Float64Array(_)
                | Value::Float32Array(_)
                | Value::Int8Array(_)
                | Value::Int16Array(_)
                | Value::Uint16Array(_)
                | Value::Int32Array(_)
                | Value::Uint8Array(_)
                | Value::Uint32Array(_)
                | Value::Uint8ClampedArray(_)
                | Value::DataView(_),
        )
    )
}

fn is_data_view_builtin(builtin: Builtin) -> bool {
    matches!(
        builtin,
        Builtin::DataViewGetInt8
            | Builtin::DataViewGetUint8
            | Builtin::DataViewGetInt16
            | Builtin::DataViewGetUint16
            | Builtin::DataViewGetInt32
            | Builtin::DataViewGetUint32
            | Builtin::DataViewGetFloat16
            | Builtin::DataViewGetFloat32
            | Builtin::DataViewGetFloat64
            | Builtin::DataViewSetInt8
            | Builtin::DataViewSetUint8
            | Builtin::DataViewSetInt16
            | Builtin::DataViewSetUint16
            | Builtin::DataViewSetInt32
            | Builtin::DataViewSetUint32
            | Builtin::DataViewSetFloat16
            | Builtin::DataViewSetFloat32
            | Builtin::DataViewSetFloat64
    )
}

fn execute_data_view_builtin(
    builtin: Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let view = data_view_receiver(receiver)?;
    let offset = data_view_offset(arguments.first())?;
    let endian_argument = if is_data_view_setter(builtin) { 2 } else { 1 };
    let little_endian = arguments.get(endian_argument).is_some_and(is_truthy);
    if !is_data_view_setter(builtin) {
        return execute_data_view_get(builtin, view, offset, little_endian);
    }
    execute_data_view_set(builtin, view, offset, little_endian, arguments)
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
    )
}

fn execute_data_view_set(
    builtin: Builtin,
    view: &crate::value::DataViewData,
    offset: usize,
    little_endian: bool,
    arguments: &[Value],
) -> Result<Value, VmError> {
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

fn data_view_offset(value: Option<&Value>) -> Result<usize, VmError> {
    let number = crate::intl::tolocale::value::to_number_result(value)?;
    if !number.is_finite() || number < 0.0 {
        return Err(range_error("Offset is outside the bounds of the DataView"));
    }
    Ok(number.trunc() as usize)
}

fn data_view_error(error: crate::value::DataViewError) -> VmError {
    let message = match error {
        crate::value::DataViewError::Detached => "Detached DataView",
        crate::value::DataViewError::OutOfBounds => "Offset is outside the bounds of the DataView",
    };
    range_error(message)
}

fn type_error(message: &str) -> VmError {
    let arguments = [Value::String(message.to_string())];
    VmError::Thrown(crate::builtins::error(Builtin::TypeError, &arguments))
}

fn range_error(message: &str) -> VmError {
    let arguments = [Value::String(message.to_string())];
    VmError::Thrown(crate::builtins::error(Builtin::RangeError, &arguments))
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

fn function_prototype_builtin(
    builtin: Builtin,
    receiver: Option<&Value>,
) -> Result<Value, VmError> {
    match builtin {
        Builtin::FunctionPrototypeToString => {
            Ok(crate::builtins::function_prototype_to_string(receiver))
        }
        Builtin::FunctionPrototypeValueOf => {
            Ok(crate::builtins::function_prototype_value_of(receiver))
        }
        _ => Ok(Value::Undefined),
    }
}

fn regexp_prototype_to_string(
    receiver: Option<&Value>,
) -> Result<Value, VmError> {
    let Some(value) = receiver else {
        return Ok(Value::String("/(?:)/".to_string()));
    };
    let source = crate::execute::get_property(value, "source");
    let flags = crate::execute::get_property(value, "flags");
    let source_str = if let Value::String(s) = &source {
        s.clone()
    } else {
        String::new()
    };
    let flags_str = if let Value::String(s) = &flags {
        s.clone()
    } else {
        String::new()
    };
    Ok(Value::String(format!("/{source_str}/{flags_str}")))
}
