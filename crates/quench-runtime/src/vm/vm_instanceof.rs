fn instanceof(value: &Value, constructor: &Value) -> Result<bool, VmError> {
    if !crate::value::is_object(constructor) {
        return Err(type_error("Right-hand side of instanceof is not an object"));
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
    if matches!(constructor, Value::Builtin(Builtin::Function)) {
        return Some(matches!(value, Value::Function(_) | Value::BoundFunction(_)));
    }
    Some(match (value, constructor) {
        (Value::BigInt64Array(_), Value::Builtin(Builtin::BigInt64Array))
        | (Value::BigUint64Array(_), Value::Builtin(Builtin::BigUint64Array))
        | (Value::Map(_), Value::Builtin(Builtin::Map))
        | (Value::Set(_), Value::Builtin(Builtin::Set)) => true,
        _ => return None,
    })
}

fn ordinary_instanceof(value: &Value, constructor: &Value) -> Result<bool, VmError> {
    let prototype = crate::execute::get_property_result(constructor, "prototype")?;
    if !crate::value::is_object(&prototype) {
        return Err(type_error("Function has non-object prototype"));
    }
    Ok(prototype_chain_contains(value, &prototype)
        || own_constructor(value)
            .is_some_and(|found| crate::builtins::same_value(Some(&found), Some(constructor)))
        || is_error_subclass(value, constructor))
}

fn is_error_subclass(value: &Value, constructor: &Value) -> bool {
    let (Value::Builtin(Builtin::Error), Some(Value::Builtin(actual))) =
        (constructor, own_constructor(value))
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
        ),
        _ => crate::conversion::is_callable(value),
    }
}

fn prototype_chain_contains(value: &Value, expected: &Value) -> bool {
    let mut current = internal_prototype(value);
    for _ in 0..1_024 {
        let Some(prototype) = current else {
            return false;
        };
        if crate::builtins::same_value(Some(&prototype), Some(expected)) {
            return true;
        }
        current = internal_prototype(&prototype);
    }
    false
}

fn internal_prototype(value: &Value) -> Option<Value> {
    match value {
        Value::Object(properties) => properties
            .iter()
            .rev()
            .find(|(name, _)| name == "\0prototype")
            .map(|(_, prototype)| prototype.clone())
            .or_else(|| Some(Value::Builtin(Builtin::ObjectPrototype))),
        Value::Array(values) if values.is_arguments() => {
            Some(Value::Builtin(Builtin::ObjectPrototype))
        }
        Value::Array(_) => Some(Value::Builtin(Builtin::ArrayPrototype)),
        Value::Promise(_) => Some(Value::Builtin(Builtin::PromisePrototype)),
        Value::Generator(_) => Some(Value::Builtin(Builtin::ObjectPrototype)),
        Value::Builtin(Builtin::FunctionPrototype) => Some(Value::Builtin(Builtin::ObjectPrototype)),
        Value::Function(_) | Value::BoundFunction(_) | Value::Builtin(_) => {
            Some(Value::Builtin(Builtin::FunctionPrototype))
        }
        _ => None,
    }
}

fn own_constructor(value: &Value) -> Option<Value> {
    let Value::Object(properties) = value else {
        return None;
    };
    properties
        .iter()
        .rev()
        .find_map(|(name, value)| (name == "constructor").then(|| value.clone()))
}
