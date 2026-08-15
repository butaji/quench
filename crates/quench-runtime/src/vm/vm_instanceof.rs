fn instanceof(value: &Value, constructor: &Value) -> Result<bool, VmError> {
    if !crate::value::is_object(constructor) {
        return Err(type_error("Right-hand side of instanceof is not an object"));
    }
    if builtin_error_instance(value, constructor) {
        return Ok(true);
    }
    if let Some(handler) = has_instance_handler(constructor)? {
        let arguments = [value.clone()];
        let result = crate::functions::execute_target(&handler, constructor, &arguments)?;
        return Ok(is_truthy(&result));
    }
    if !instanceof_callable(constructor) {
        return Err(type_error("Right-hand side of instanceof is not callable"));
    }
    if !crate::value::is_object(value) {
        return Ok(false);
    }
    if let Some(result) = builtin_instanceof(value, constructor) {
        return Ok(result);
    }
    ordinary_instanceof(value, constructor)
}

fn builtin_error_instance(value: &Value, constructor: &Value) -> bool {
    let Some(constructor) = intrinsic_builtin(constructor) else {
        return false;
    };
    matches!(
        constructor,
        Builtin::Error
            | Builtin::RangeError
            | Builtin::ReferenceError
            | Builtin::SyntaxError
            | Builtin::EvalError
            | Builtin::URIError
            | Builtin::AggregateError
            | Builtin::TypeError
    ) && (own_constructor(value) == Some(Value::Builtin(constructor))
        || (own_constructor(value).is_none()
            && crate::vm::has_error_slot(value)))
}

fn intrinsic_builtin(value: &Value) -> Option<Builtin> {
    match value {
        Value::Builtin(builtin) => Some(*builtin),
        Value::BoundFunction(bound) if crate::vm::realm::is_intrinsic(bound) => {
            let Value::Builtin(builtin) = &bound.target else {
                return None;
            };
            Some(*builtin)
        }
        _ => None,
    }
}

fn has_instance_handler(constructor: &Value) -> Result<Option<Value>, VmError> {
    let handler = crate::execute::get_property_result(constructor, "Symbol.hasInstance")?;
    if matches!(handler, Value::Undefined) {
        return Ok(None);
    }
    if !crate::conversion::is_callable(&handler) {
        return Err(type_error("@@hasInstance is not callable"));
    }
    Ok(Some(handler))
}

fn builtin_instanceof(value: &Value, constructor: &Value) -> Option<bool> {
    let constructor = intrinsic_builtin(constructor)?;
    if constructor == Builtin::Function {
        return Some(function_instanceof(value));
    }
    if constructor == Builtin::AsyncGeneratorFunction {
        return Some(
            matches!(
                value,
                Value::Function(function)
                    if function.is_async && matches!(function.kind, crate::ops::FunctionKind::Generator)
            ) || matches!(
                value,
                Value::Generator(generator) if generator.function.is_async
            ),
        );
    }
    Some(match (value, constructor) {
        (Value::Generator(_), Builtin::Object) => true,
        (Value::Array(values), Builtin::Array) if !values.is_arguments() => true,
        (Value::BigInt64Array(_), Builtin::BigInt64Array)
        | (Value::BigUint64Array(_), Builtin::BigUint64Array)
        | (Value::Float32Array(_), Builtin::Float32Array)
        | (Value::Float64Array(_), Builtin::Float64Array)
        | (Value::Int8Array(_), Builtin::Int8Array)
        | (Value::Int16Array(_), Builtin::Int16Array)
        | (Value::Int32Array(_), Builtin::Int32Array)
        | (Value::Uint8Array(_), Builtin::Uint8Array)
        | (Value::Uint8ClampedArray(_), Builtin::Uint8ClampedArray)
        | (Value::Uint16Array(_), Builtin::Uint16Array)
        | (Value::Uint32Array(_), Builtin::Uint32Array)
        | (Value::Promise(_), Builtin::Promise) => true,
        (Value::Object(properties), Builtin::Date)
            if properties.iter().any(|(name, _)| name == "timeValue") =>
        {
            true
        }
        (Value::Object(properties), Builtin::RegExp)
            if properties.iter().any(|(name, _)| name == "source") =>
        {
            true
        }
        (Value::Map(data), Builtin::Map) if !data.weak => true,
        (Value::Map(data), Builtin::WeakMap) if data.weak => true,
        (Value::ArrayBuffer(data), Builtin::SharedArrayBuffer) if data.shared => true,
        (Value::Set(_), Builtin::Set) => true,
        (Value::Set(data), Builtin::WeakSet) if data.weak => true,
        (Value::Object(properties), Builtin::WeakRef) => {
            properties.iter().any(|(name, _)| name == "\0weakref")
        }
        _ => return None,
    })
}

fn function_instanceof(value: &Value) -> bool {
    matches!(value, Value::Function(_) | Value::BoundFunction(_))
        || matches!(value, Value::Builtin(_) if instanceof_callable(value))
}

fn ordinary_instanceof(value: &Value, constructor: &Value) -> Result<bool, VmError> {
    let prototype = crate::execute::get_property_result(constructor, "prototype")?;
    if !crate::value::is_object(&prototype) {
        return Err(type_error("Function has non-object prototype"));
    }
    Ok(prototype_chain_contains(value, &prototype)?
        || own_constructor(value)
            .is_some_and(|found| crate::builtins::same_value(Some(&found), Some(constructor)))
        || is_error_subclass(value, constructor))
}

pub(crate) fn function_has_instance(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let constructor = receiver.unwrap_or(&Value::Undefined);
    if !instanceof_callable(constructor) {
        return Ok(Value::Boolean(false));
    }
    let value = arguments.first().unwrap_or(&Value::Undefined);
    if !crate::value::is_object(value) {
        return Ok(Value::Boolean(false));
    }
    if let Some(result) = builtin_instanceof(value, constructor) {
        return Ok(Value::Boolean(result));
    }
    Ok(Value::Boolean(ordinary_instanceof(value, constructor)?))
}

fn is_error_subclass(value: &Value, constructor: &Value) -> bool {
    let (Some(Builtin::Error), Some(Value::Builtin(actual))) =
        (intrinsic_builtin(constructor), own_constructor(value))
    else {
        return false;
    };
    matches!(
        actual,
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

fn instanceof_callable(value: &Value) -> bool {
    match value {
        Value::Builtin(builtin) => !matches!(
            builtin,
            Builtin::Math
                | Builtin::Json
                | Builtin::Reflect
                | Builtin::ObjectPrototype
                | Builtin::ArrayPrototype
                | Builtin::DatePrototype
                | Builtin::StringPrototype
                | Builtin::NumberPrototype
                | Builtin::BooleanPrototype
                | Builtin::SymbolPrototype
                | Builtin::BigIntPrototype
                | Builtin::MapPrototype
                | Builtin::SetPrototype
                | Builtin::WeakMapPrototype
                | Builtin::WeakSetPrototype
                | Builtin::SharedArrayBufferPrototype
        ),
        _ => crate::conversion::is_callable(value),
    }
}

fn prototype_chain_contains(value: &Value, expected: &Value) -> bool {
    let mut current = internal_prototype(value).map(resolve_object_alias);
    for _ in 0..1_024 {
        if matches!(current, Value::Null) {
            return Ok(false);
        }
        current = internal_prototype(&prototype).map(resolve_object_alias);
    }
    false
}

fn resolve_object_alias(value: Value) -> Value {
    let Value::ObjectAlias(alias) = value else {
        return value;
    };
    let object = alias
        .0
        .borrow()
        .upgrade()
        .map(Value::Object)
        .unwrap_or(Value::Null);
    object
}

fn internal_prototype(value: &Value) -> Option<Value> {
    if let Some(prototype) = crate::typed_array_prototype::get(value) {
        return Some(prototype);
    }
    if let Some(prototype) = custom_object_prototype(value) {
        return Some(prototype);
    }
    match value {
        Value::Object(_) => Some(Value::Builtin(Builtin::ObjectPrototype)),
        Value::Array(values) if values.is_arguments() => {
            Some(Value::Builtin(Builtin::ObjectPrototype))
        }
        current = crate::builtins::object::get_prototype_of(Some(&current))?;
    }
    Ok(false)
}

fn own_constructor(value: &Value) -> Option<Value> {
    match value {
        Value::Object(properties) => properties
            .iter()
            .rev()
            .find_map(|(name, value)| (name == "constructor").then(|| value.clone())),
        Value::ObjectAlias(alias) => alias
            .0
            .borrow()
            .upgrade()
            .and_then(|properties| {
                properties
                    .iter()
                    .rev()
                    .find_map(|(name, value)| (name == "constructor").then(|| value.clone()))
            }),
        _ => None,
    }
}
