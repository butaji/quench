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
    let Value::Builtin(constructor) = constructor else {
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
    ) && own_constructor(value) == Some(Value::Builtin(*constructor))
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
        return Some(function_instanceof(value));
    }
    Some(match (value, constructor) {
        (Value::BigInt64Array(_), Value::Builtin(Builtin::BigInt64Array))
        | (Value::BigUint64Array(_), Value::Builtin(Builtin::BigUint64Array))
        | (Value::Promise(_), Value::Builtin(Builtin::Promise)) => true,
        (Value::Object(properties), Value::Builtin(Builtin::Date))
            if properties.iter().any(|(name, _)| name == "timeValue") => true,
        (Value::Object(properties), Value::Builtin(Builtin::RegExp))
            if properties.iter().any(|(name, _)| name == "source") => true,
        (Value::Map(data), Value::Builtin(Builtin::Map)) if !data.weak => true,
        (Value::Map(data), Value::Builtin(Builtin::WeakMap)) if data.weak => true,
        (Value::ArrayBuffer(data), Value::Builtin(Builtin::SharedArrayBuffer)) if data.shared => true,
        (Value::Set(_), Value::Builtin(Builtin::Set)) => true,
        (Value::Set(data), Value::Builtin(Builtin::WeakSet)) if data.weak => true,
        (Value::Object(properties), Value::Builtin(Builtin::WeakRef)) => {
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
        Value::Array(values) => values
            .property("\0prototype")
            .or_else(|| Some(Value::Builtin(Builtin::ArrayPrototype))),
        Value::ArrayBuffer(buffer) => buffer_prototype(buffer),
        Value::DataView(view) => view
            .prototype()
            .or_else(|| Some(Value::Builtin(Builtin::DataViewPrototype))),
        Value::Map(data) => map_prototype(data),
        Value::Set(data) => data.prototype().or_else(|| {
            Some(Value::Builtin(if data.weak {
                Builtin::WeakSetPrototype
            } else {
                Builtin::SetPrototype
            }))
        }),
        Value::Promise(data) => data
            .prototype()
            .or_else(|| Some(Value::Builtin(Builtin::PromisePrototype))),
        Value::Generator(_) => Some(Value::Builtin(Builtin::ObjectPrototype)),
        Value::Iterator(_) => Some(Value::Builtin(Builtin::IteratorPrototype)),
        Value::Builtin(builtin) => builtin_prototype_parent(*builtin),
        Value::Function(_) => Some(Value::Builtin(Builtin::FunctionPrototype)),
        Value::BoundFunction(_) => {
            Some(Value::Builtin(Builtin::FunctionPrototype))
        }
        _ => None,
    }
}

fn builtin_prototype_parent(builtin: Builtin) -> Option<Value> {
    if matches!(
        builtin,
        Builtin::GeneratorFunctionPrototype
            | Builtin::AsyncGeneratorFunctionPrototype
    ) {
        return Some(Value::Builtin(Builtin::FunctionPrototype));
    }
    if builtin == Builtin::ArrayIteratorPrototype {
        return Some(Value::Builtin(Builtin::IteratorPrototype));
    }
    if builtin == Builtin::IteratorPrototype {
        return Some(Value::Builtin(Builtin::ObjectPrototype));
    }
    matches!(
        builtin,
        Builtin::FunctionPrototype
            | Builtin::MapPrototype
            | Builtin::SetPrototype
            | Builtin::WeakMapPrototype
            | Builtin::WeakSetPrototype
            | Builtin::SharedArrayBufferPrototype
            | Builtin::WeakRefPrototype
    )
    .then_some(Value::Builtin(Builtin::ObjectPrototype))
}

fn map_prototype(data: &crate::value::MapData) -> Option<Value> {
    data.prototype().or_else(|| {
        Some(Value::Builtin(if data.weak {
            Builtin::WeakMapPrototype
        } else {
            Builtin::MapPrototype
        }))
    })
}

fn buffer_prototype(data: &crate::value::ArrayBufferData) -> Option<Value> {
    data.prototype().or_else(|| {
        Some(Value::Builtin(if data.shared {
            Builtin::SharedArrayBufferPrototype
        } else {
            Builtin::ArrayBufferPrototype
        }))
    })
}

fn custom_object_prototype(value: &Value) -> Option<Value> {
    match value {
        Value::Object(properties) => properties
            .iter()
            .rev()
            .find(|(name, _)| name == "\0prototype")
            .map(|(_, prototype)| prototype.clone()),
        Value::Function(function) => function
            .properties
            .borrow()
            .iter()
            .rev()
            .find(|(name, _)| name == "\0prototype")
            .map(|(_, prototype)| prototype.clone()),
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
