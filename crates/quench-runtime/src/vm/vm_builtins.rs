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
        .or_else(|| crate::disposable_stack::execute(builtin, receiver, arguments))
        .or_else(|| crate::finalization_registry::execute(builtin, receiver, arguments))
        .or_else(|| {
            (builtin != Builtin::Date)
                .then(|| crate::date::execute(builtin, receiver, arguments))?
        })
}
fn is_function_builtin(builtin: Builtin) -> bool {
    matches!(
        builtin,
        Builtin::FunctionCall
            | Builtin::FunctionApply
            | Builtin::FunctionBind
            | Builtin::ArrayJoin
            | Builtin::ArrayPush
            | Builtin::ArrayShift
            | Builtin::ArrayReverse
            | Builtin::ArrayPop
            | Builtin::ArrayUnshift
            | Builtin::ArrayFill
            | Builtin::ArrayCopyWithin
            | Builtin::ArrayFindLast
            | Builtin::ArrayFindLastIndex
            | Builtin::ArrayToSorted
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
pub(crate) fn create_list_from_array_like(value: Option<&Value>) -> Result<Vec<Value>, VmError> {
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
            | Builtin::BooleanValueOf
            | Builtin::BooleanToString
            | Builtin::Eval
            | Builtin::Escape
            | Builtin::EncodeURI
            | Builtin::EncodeURIComponent
            | Builtin::DecodeURI
            | Builtin::DecodeURIComponent
            | Builtin::IsFinite
            | Builtin::IsNaN
            | Builtin::NumberIsInteger
            | Builtin::NumberIsSafeInteger
            | Builtin::Number
            | Builtin::BigInt
            | Builtin::BigIntAsIntN
            | Builtin::BigIntAsUintN
            | Builtin::BigIntToString
            | Builtin::NumberToString
            | Builtin::NumberValueOf
            | Builtin::BigIntValueOf
            | Builtin::SymbolToString
            | Builtin::SymbolValueOf
            | Builtin::SymbolPrototypeToPrimitive
            | Builtin::SymbolDescriptionGetter
            | Builtin::StringToString
            | Builtin::StringValueOf
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
            | Builtin::SuppressedError
            | Builtin::ErrorIsError
            | Builtin::ErrorPrototypeToString
            | Builtin::ErrorPrototypeNameGetter
            | Builtin::ErrorPrototypeMessageGetter
            | Builtin::ErrorPrototypeCauseGetter
            | Builtin::ErrorPrototypeStackGetter
            | Builtin::ErrorPrototypeStackSetter
            | Builtin::WeakRefDeref
    )
}
fn execute_simple_builtin(
    builtin: Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Result<Value, VmError> {
    if let Some(result) = simple_prelude(builtin, arguments, receiver) {
        return result;
    }
    match builtin {
        Builtin::Boolean => Ok(Value::Boolean(arguments.first().is_some_and(is_truthy))),
        Builtin::BooleanValueOf => boolean_value_of(receiver),
        Builtin::BooleanToString => boolean_to_string(receiver),
        Builtin::Eval => crate::reflect::builtin(builtin, arguments, receiver),
        Builtin::Escape => Ok(crate::builtins::escape(arguments.first())),
        Builtin::EncodeURI => crate::builtins::encode_uri(arguments.first(), true),
        Builtin::EncodeURIComponent => crate::builtins::encode_uri(arguments.first(), false),
        Builtin::DecodeURI => crate::builtins::decode_uri(arguments.first(), true),
        Builtin::DecodeURIComponent => crate::builtins::decode_uri(arguments.first(), false),
        Builtin::IsFinite => Ok(Value::Boolean(is_finite_check(
            arguments.first(),
            receiver,
        )?)),
        Builtin::IsNaN => Ok(Value::Boolean(is_nan_check(arguments.first(), receiver)?)),
        Builtin::Number => Ok(Value::Number(explicit_number(arguments.first())?)),
        Builtin::BigInt => explicit_bigint(arguments.first()),
        Builtin::BigIntAsIntN | Builtin::BigIntAsUintN => {
            bigint_as_n(arguments, builtin == Builtin::BigIntAsIntN)
        }
        Builtin::BigIntToString => bigint_to_string(receiver, arguments),
        Builtin::NumberToString => boolean_or_number_string(receiver, arguments),
        Builtin::NumberValueOf => number_value_of(receiver),
        Builtin::BigIntValueOf => bigint_value_of(receiver),
        Builtin::SymbolToString => symbol_to_string(receiver),
        Builtin::SymbolValueOf => symbol_value_of(receiver),
        Builtin::SymbolPrototypeToPrimitive => symbol_value_of(receiver),
        Builtin::SymbolDescriptionGetter => symbol_description(receiver),
        Builtin::StringToString | Builtin::StringValueOf => string_value_of(receiver),
        Builtin::BoxedValueOf => Ok(boxed_value(receiver)),
        Builtin::ObjectPrototypeToString => Ok(crate::builtins::prototype_to_string(receiver)),
        Builtin::ObjectPrototypeValueOf => crate::builtins::prototype_value_of(receiver),
        Builtin::FunctionPrototypeToString | Builtin::FunctionPrototypeValueOf => {
            function_prototype_builtin(builtin, receiver)
        }
        Builtin::RegExpPrototypeToString => regexp_prototype_to_string(receiver),
        Builtin::NumberToFixed | Builtin::NumberToPrecision | Builtin::NumberToExponential => {
            crate::number_fmt::number_format(receiver, arguments.first(), builtin)
        }
        Builtin::Error
        | Builtin::RangeError
        | Builtin::ReferenceError
        | Builtin::SyntaxError
        | Builtin::EvalError
        | Builtin::URIError
        | Builtin::AggregateError
        | Builtin::TypeError
        | Builtin::SuppressedError
        | Builtin::ErrorIsError
        | Builtin::ErrorPrototypeToString
        | Builtin::ErrorPrototypeNameGetter
        | Builtin::ErrorPrototypeMessageGetter
        | Builtin::ErrorPrototypeCauseGetter
        | Builtin::ErrorPrototypeStackGetter
        | Builtin::ErrorPrototypeStackSetter => Ok(error_builtin(builtin, arguments, receiver)?),
        Builtin::Object => Ok(crate::builtins::object(arguments)),
        Builtin::Date => Ok(crate::date::call()),
        _ => Ok(Value::Undefined),
    }
}

fn error_builtin(
    builtin: Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Result<Value, VmError> {
    match builtin {
        Builtin::Error
        | Builtin::RangeError
        | Builtin::ReferenceError
        | Builtin::SyntaxError
        | Builtin::EvalError
        | Builtin::URIError
        | Builtin::AggregateError
        | Builtin::TypeError
        | Builtin::SuppressedError => {
            crate::construct::construct_value(&Value::Builtin(builtin), arguments)
        }
        Builtin::ErrorIsError => Ok(error_is_error(arguments.first())),
        Builtin::ErrorPrototypeToString => error_to_string(receiver),
        Builtin::ErrorPrototypeNameGetter => Ok(error_name_getter(receiver)?),
        Builtin::ErrorPrototypeMessageGetter => Ok(error_message_getter(receiver)?),
        Builtin::ErrorPrototypeCauseGetter => Ok(error_cause_getter(receiver)?),
        Builtin::ErrorPrototypeStackGetter => error_stack_getter(receiver),
        Builtin::ErrorPrototypeStackSetter => error_stack_setter(receiver, arguments),
        _ => Ok(Value::Undefined),
    }
}

fn error_name_getter(receiver: Option<&Value>) -> Result<Value, VmError> {
    let value = error_receiver(receiver, "Error.prototype.name")?;
    crate::execute::get_property_result(value, "name")
}

fn error_message_getter(receiver: Option<&Value>) -> Result<Value, VmError> {
    let value = error_receiver(receiver, "Error.prototype.message")?;
    crate::execute::get_property_result(value, "message")
}

fn error_cause_getter(receiver: Option<&Value>) -> Result<Value, VmError> {
    let value = error_receiver(receiver, "Error.prototype.cause")?;
    crate::execute::get_property_result(value, "cause")
}

fn error_stack_getter(receiver: Option<&Value>) -> Result<Value, VmError> {
    let value = error_receiver(receiver, "Error.prototype.stack")?;
    if has_error_slot(value) {
        Ok(Value::String("Error".to_string()))
    } else {
        Ok(Value::Undefined)
    }
}

fn error_stack_setter(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let value = error_receiver(receiver, "Error.prototype.stack")?;
    let stack = arguments.first().ok_or_else(|| {
        crate::value::error::throw_type_error("Cannot set property 'stack' of error")
    })?;
    if let Some(home) = set_error_stack_home() {
        if crate::builtins::same_value(Some(&home), Some(value)) {
            return Err(crate::value::error::throw_type_error(
                "Cannot set property 'stack' of error",
            ));
        }
    }
    let Value::String(_) = stack else {
        return Err(crate::value::error::throw_type_error(
            "Stack value must be a string",
        ));
    };
    if matches!(value, Value::Proxy(_)) {
        define_proxy_stack(value, stack.clone())?;
        return Ok(Value::Undefined);
    }
    let key = Value::String("stack".to_string());
    if !matches!(
        crate::builtins::object::descriptor(Some(value), Some(&key))?,
        Value::Undefined
    ) {
        define_own_stack(value, stack.clone())?;
    } else {
        define_own_stack(value, stack.clone())?;
    }
    Ok(Value::Undefined)
}

fn define_own_stack(value: &Value, stack: Value) -> Result<(), VmError> {
    let updated = crate::builtins::set_property(value.clone(), "stack", stack);
    crate::locals::replace_value(value, &updated);
    Ok(())
}

fn define_proxy_stack(value: &Value, stack: Value) -> Result<Value, VmError> {
    let descriptor = Value::Object(Rc::new(crate::value::ObjectData::new(vec![
        ("value".to_string(), stack),
        ("writable".to_string(), Value::Boolean(true)),
        ("enumerable".to_string(), Value::Boolean(true)),
        ("configurable".to_string(), Value::Boolean(true)),
    ])));
    let result = crate::proxy::proxy_define_property(value, "stack", &descriptor)?;
    if matches!(result, Value::Boolean(false)) {
        return Err(crate::value::error::throw_type_error(
            "Proxy defineProperty trap returned false",
        ));
    }
    Ok(result)
}

fn set_error_stack_home() -> Option<Value> {
    let value = crate::execute::get_property(&crate::vm::current_global_object(), "Error");
    let Ok(value) = crate::execute::get_property_result(&value, "prototype") else {
        return None;
    };
    if !crate::value::is_object(&value) {
        return None;
    }
    Some(value)
}

fn has_error_slot(value: &Value) -> bool {
    match value {
        Value::Object(value) => value
            .iter()
            .any(|(key, _)| key == crate::builtins::ERROR_SLOT),
        Value::ObjectAlias(alias) => alias.0.borrow().upgrade().is_some_and(|value| {
            value
                .iter()
                .any(|(key, _)| key == crate::builtins::ERROR_SLOT)
        }),
        _ => false,
    }
}

fn error_to_string(receiver: Option<&Value>) -> Result<Value, VmError> {
    let value = error_receiver(receiver, "Error.prototype.toString")?;
    let name = match error_to_string_property(value, "name")? {
        Value::Undefined => "Error".to_string(),
        value => crate::conversion::to_string(&value)?,
    };
    let message = match error_to_string_property(value, "message")? {
        Value::Undefined => String::new(),
        value => crate::conversion::to_string(&value)?,
    };
    if name.is_empty() && message.is_empty() {
        Ok(Value::String(String::new()))
    } else if name.is_empty() {
        Ok(Value::String(message))
    } else if message.is_empty() {
        Ok(Value::String(name))
    } else {
        Ok(Value::String(format!("{name}: {message}")))
    }
}

fn error_to_string_property(value: &Value, key: &str) -> Result<Value, VmError> {
    let result = crate::execute::get_property_result(value, key)?;
    if !matches!(value, Value::Object(_)) || !matches!(key, "name" | "message") {
        return Ok(result);
    }
    let own = crate::builtins::object::descriptor(
        Some(value),
        Some(&Value::String(key.to_string())),
    )?;
    if !matches!(own, Value::Undefined) {
        return Ok(result);
    }
    let prototype = crate::builtins::object::get_prototype_of(Some(value))?;
    if matches!(prototype, Value::Builtin(crate::ops::Builtin::ObjectPrototype)) {
        return Ok(Value::Undefined);
    }
    Ok(result)
}

fn error_is_error(value: Option<&Value>) -> Value {
    let Some(value) = value else {
        return Value::Boolean(false);
    };
    if !crate::value::is_object(value) {
        return Value::Boolean(false);
    }
    Value::Boolean(has_error_slot(value))
}

fn error_receiver<'a>(receiver: Option<&'a Value>, name: &str) -> Result<&'a Value, VmError> {
    let value = receiver.ok_or_else(|| {
        crate::value::error::throw_type_error(&format!("{name} called on non-object"))
    })?;
    if matches!(value, Value::Null | Value::Undefined) {
        return Err(crate::value::error::throw_type_error(&format!(
            "{name} called on non-object"
        )));
    }
    if !crate::value::is_object(value) {
        return Err(crate::value::error::throw_type_error(&format!(
            "{name} called on non-object"
        )));
    }
    Ok(value)
}
fn boolean_or_number_string(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    if let Some(value) = boolean_receiver(receiver) {
        return Ok(Value::String(value.to_string()));
    }
    let Value::Number(value) = number_value_of(receiver)? else {
        return Err(crate::value::error::throw_type_error(
            "Number.prototype.toString called on incompatible receiver",
        ));
    };
    let radix = radix(arguments.first())?;
    Ok(Value::String(number_to_string(value, radix)))
}

fn radix(value: Option<&Value>) -> Result<u32, VmError> {
    let Some(value) = value.filter(|value| !matches!(value, Value::Undefined)) else {
        return Ok(10);
    };
    let radix = crate::conversion::to_number(value)?.trunc();
    if !(2.0..=36.0).contains(&radix) {
        return Err(crate::value::error::throw_range_error("Invalid radix"));
    }
    Ok(radix as u32)
}

fn number_to_string(value: f64, radix: u32) -> String {
    if radix == 10 || !value.is_finite() || value.fract() != 0.0 {
        return crate::conversion::number_to_string(value);
    }
    let sign = if value.is_sign_negative() { "-" } else { "" };
    format!("{sign}{}", radix_digits(value.abs() as u64, radix))
}

fn radix_digits(mut value: u64, radix: u32) -> String {
    if value == 0 {
        return "0".to_string();
    }
    let mut digits = Vec::new();
    while value != 0 {
        let digit = (value % u64::from(radix)) as u8;
        digits.push(char::from(if digit < 10 {
            b'0' + digit
        } else {
            b'a' + digit - 10
        }));
        value /= u64::from(radix);
    }
    digits.iter().rev().collect()
}
fn boolean_receiver(receiver: Option<&Value>) -> Option<bool> {
    match receiver {
        Some(Value::Boolean(value)) => Some(*value),
        Some(Value::Builtin(Builtin::BooleanPrototype)) => Some(false),
        _ => None,
    }
}
fn boolean_to_string(receiver: Option<&Value>) -> Result<Value, VmError> {
    let value = match receiver {
        Some(Value::Builtin(Builtin::BooleanPrototype)) => false,
        Some(Value::Boolean(value)) => *value,
        Some(Value::Object(object)) => match object
            .iter()
            .rev()
            .find_map(|(name, value)| (name == "_value").then_some(value))
        {
            Some(Value::Boolean(value)) => *value,
            _ => return Err(boolean_receiver_error()),
        },
        _ => return Err(boolean_receiver_error()),
    };
    Ok(Value::String(value.to_string()))
}

fn boolean_receiver_error() -> VmError {
    crate::value::error::throw_type_error(
        "Boolean.prototype.toString called on incompatible receiver",
    )
}
fn weak_ref_deref(receiver: Option<&Value>) -> Result<Value, VmError> {
    let Some(Value::Object(object)) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "WeakRef.prototype.deref called on incompatible receiver",
        ));
    };
    let Some((_, target)) = object.iter().rev().find(|(key, _)| key == "\0weakref") else {
        return Err(crate::value::error::throw_type_error(
            "WeakRef.prototype.deref called on incompatible receiver",
        ));
    };
    Ok(target.clone())
}
pub(crate) fn realm_id_for_intrinsic_receiver(receiver: Option<&Value>) -> Option<RealmId> {
    let Some(Value::HostCapability(token)) = receiver else {
        return None;
    };
    realm::id_for_token(token)
}
pub(crate) fn explicit_number(value: Option<&Value>) -> Result<f64, VmError> {
    let Some(value) = value else {
        return Ok(0.0);
    };
    if let Value::BigInt(value) = value {
        return Ok(value.parse().unwrap_or(f64::NAN));
    }
    crate::intl::tolocale::value::to_number_result(Some(value))
}
include!("vm_bigint.rs");

fn boxed_value(receiver: Option<&Value>) -> Value {
    receiver.map_or(Value::Undefined, |value| match value {
        Value::Object(_) => crate::execute::get_property(value, "_value"),
        _ => value.clone(),
    })
}
pub(crate) fn number_value_of(receiver: Option<&Value>) -> Result<Value, VmError> {
    match receiver {
        Some(Value::Builtin(Builtin::NumberPrototype)) => Ok(Value::Number(0.0)),
        Some(Value::Number(value)) => Ok(Value::Number(*value)),
        Some(value @ Value::Object(_)) => {
            let constructor = crate::execute::get_property_result(value, "constructor")?;
            if constructor != Value::Builtin(Builtin::Number) {
                return Err(crate::value::error::throw_type_error(
                    "Number.prototype.valueOf called on incompatible receiver",
                ));
            }
            let value = crate::execute::get_property_result(value, "_value")?;
            if let Value::Number(_) = value {
                Ok(value)
            } else {
                Err(crate::value::error::throw_type_error(
                    "Number.prototype.valueOf called on incompatible receiver",
                ))
            }
        }
        _ => Err(crate::value::error::throw_type_error(
            "Number.prototype.valueOf called on incompatible receiver",
        )),
    }
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
            | Builtin::DataViewGetBigInt64
            | Builtin::DataViewGetBigUint64
            | Builtin::DataViewSetInt8
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
            | Builtin::DataViewBufferGetter
            | Builtin::DataViewByteLengthGetter
            | Builtin::DataViewByteOffsetGetter
    )
}
fn is_number_receiver(receiver: Option<&Value>) -> bool {
    matches!(receiver, Some(Value::Builtin(Builtin::Number)))
}

fn is_nan_check(value: Option<&Value>, receiver: Option<&Value>) -> Result<bool, VmError> {
    if is_number_receiver(receiver) {
        return Ok(matches!(value, Some(Value::Number(number)) if number.is_nan()));
    }
    let value = value.cloned().unwrap_or(Value::Undefined);
    Ok(crate::conversion::to_number(&value)?.is_nan())
}

fn is_finite_check(value: Option<&Value>, receiver: Option<&Value>) -> Result<bool, VmError> {
    if is_number_receiver(receiver) {
        return Ok(is_finite(value));
    }
    let value = value.cloned().unwrap_or(Value::Undefined);
    Ok(crate::conversion::to_number(&value)?.is_finite())
}

fn execute_data_view_builtin(
    builtin: Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
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
fn range_error(message: &str) -> VmError {
    let arguments = [Value::String(message.to_string())];
    VmError::Thrown(crate::builtins::error(Builtin::RangeError, &arguments))
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
fn regexp_prototype_to_string(receiver: Option<&Value>) -> Result<Value, VmError> {
    let Some(value) = receiver.filter(|value| !matches!(value, Value::Null | Value::Undefined))
    else {
        return Err(crate::value::error::throw_type_error(
            "RegExp.prototype.toString called on incompatible receiver",
        ));
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
