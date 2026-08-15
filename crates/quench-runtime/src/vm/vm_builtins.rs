fn early_dispatch(
    builtin: Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    crate::intl::tolocale::symbol::dispatch(builtin, arguments, receiver)
        .or_else(|| {
            (builtin == Builtin::AtomicsAdd
                || builtin == Builtin::AtomicsSub
                || builtin == Builtin::AtomicsExchange
                || builtin == Builtin::AtomicsOr
                || builtin == Builtin::AtomicsXor
                || builtin == Builtin::AtomicsIsLockFree
                || builtin == Builtin::AtomicsPause
                || builtin == Builtin::AtomicsStore
                || builtin == Builtin::AtomicsLoad
                || builtin == Builtin::AtomicsAnd
                || builtin == Builtin::AtomicsCompareExchange)
                .then(|| crate::atomics::execute(builtin, receiver, arguments))
        })
        .or_else(|| crate::json::execute(builtin, arguments))
        .or_else(|| crate::typed_array_ops::execute(builtin, receiver, arguments))
        .or_else(|| crate::atomics::execute(builtin, arguments))
        .or_else(|| crate::arrays::execute_builtin(builtin, receiver, arguments))
        .or_else(|| {
            (builtin == Builtin::ArrayBufferIsView).then(|| {
                Ok(Value::Boolean(matches!(
                    arguments.first(),
                    Some(Value::DataView(_))
                        | Some(Value::Float32Array(_))
                        | Some(Value::Float64Array(_))
                        | Some(Value::Int8Array(_))
                        | Some(Value::Int16Array(_))
                        | Some(Value::Int32Array(_))
                        | Some(Value::Uint8Array(_))
                        | Some(Value::Uint8ClampedArray(_))
                        | Some(Value::Uint16Array(_))
                        | Some(Value::Uint32Array(_))
                        | Some(Value::BigInt64Array(_))
                        | Some(Value::BigUint64Array(_))
                )))
            })
        })
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
        Builtin::Atomics
            | Builtin::Boolean
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
            | Builtin::ErrorPrototype
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
            | Builtin::AbstractModuleSourceToStringTagGetter
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
        Builtin::Atomics => Err(crate::value::error::throw_type_error(
            "Atomics is not callable",
        )),
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
        Builtin::ErrorPrototype => Err(crate::value::error::throw_type_error(
            "Error.prototype is not callable",
        )),
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
        Builtin::AbstractModuleSourceToStringTagGetter => Ok(Value::Undefined),
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
    if !has_error_slot(value) {
        return Ok(Value::Undefined);
    }
    let key = Value::String("stack".to_string());
    let own = crate::builtins::object::descriptor(Some(value), Some(&key))?;
    if !matches!(own, Value::Undefined)
        && !matches!(crate::execute::get_property(&own, "value"), Value::Undefined)
    {
        return crate::execute::get_property_result(value, "stack");
    }
    Ok(Value::String("Error".to_string()))
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
    let Value::String(stack_value) = stack else {
        return Err(crate::value::error::throw_type_error(
            "Stack value must be a string",
        ));
    };
    if crate::conversion::is_symbol_string(stack_value) {
        return Err(crate::value::error::throw_type_error(
            "Stack value must be a string",
        ));
    }
    if let Some(setter) = own_stack_setter(value)? {
        let argument = stack.clone();
        crate::functions::execute_target(&setter, value, std::slice::from_ref(&argument))?;
        return Ok(Value::Undefined);
    }
    if matches!(value, Value::Proxy(_)) {
        define_proxy_stack(value, stack.clone())?;
        return Ok(Value::Undefined);
    }
    define_own_stack(value, stack.clone())?;
    Ok(Value::Undefined)
}

fn own_stack_setter(value: &Value) -> Result<Option<Value>, VmError> {
    let descriptor = crate::builtins::object::descriptor(
        Some(value),
        Some(&Value::String("stack".to_string())),
    )?;
    Ok(descriptor_field(&descriptor, "set"))
}

fn define_own_stack(value: &Value, stack: Value) -> Result<(), VmError> {
    let key = Value::String("stack".to_string());
    let descriptor = crate::builtins::object::descriptor(Some(value), Some(&key))?;
    let updated = if !matches!(descriptor, Value::Undefined) {
        if let Some(setter) = descriptor_field(&descriptor, "set") {
            if matches!(setter, Value::Undefined) {
                return Err(crate::value::error::throw_type_error(
                    "Cannot set property 'stack'",
                ));
            }
            let argument = stack.clone();
            crate::functions::execute_target(&setter, value, std::slice::from_ref(&argument))?;
            return Ok(());
        }
        if descriptor_field_is_false(&descriptor, "writable") {
            return Err(crate::value::error::throw_type_error(
                "Cannot assign to read only property 'stack'",
            ));
        }
        crate::builtins::define_own_property(value, "stack", &[("value".to_string(), stack)])?
    } else {
        if !crate::properties::object_is_extensible(value) {
            return Err(crate::value::error::throw_type_error(
                "Cannot add property 'stack'",
            ));
        }
        crate::builtins::define_own_property(
            value,
            "stack",
            &[
                ("value".to_string(), stack),
                ("writable".to_string(), Value::Boolean(true)),
                ("enumerable".to_string(), Value::Boolean(true)),
                ("configurable".to_string(), Value::Boolean(true)),
            ],
        )?
    };
    crate::locals::replace_value(value, &updated);
    Ok(())
}

fn descriptor_field(descriptor: &Value, field: &str) -> Option<Value> {
    let Value::Object(properties) = descriptor else {
        return None;
    };
    properties
        .iter()
        .rev()
        .find_map(|(name, value)| (name == field).then_some(value.clone()))
}

fn descriptor_field_is_false(descriptor: &Value, field: &str) -> bool {
    matches!(
        descriptor,
        Value::Object(properties)
            if properties
                .iter()
                .rev()
                .find(|(name, _)| name == field)
                .is_some_and(|(_, value)| matches!(value, Value::Boolean(false)))
    )
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

pub(crate) fn has_error_slot(value: &Value) -> bool {
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
    if matches!(prototype, Value::Builtin(Builtin::ObjectPrototype)) {
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

include!("vm_builtins_tail.rs");
