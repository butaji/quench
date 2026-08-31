fn instanceof(value: &Value, constructor: &Value) -> Result<bool, VmError> {
    let value = dereference_binding(value);
    let constructor = dereference_binding(constructor);
    if !crate::value::is_object(&constructor) {
        return Err(type_error("Right-hand side of instanceof is not an object"));
    }
    if builtin_error_instance(&value, &constructor) {
        return Ok(true);
    }
    if !instanceof_callable(&constructor) {
        let Some(handler) = has_instance_handler(&constructor)? else {
            return Err(type_error("Right-hand side of instanceof is not callable"));
        };
        return call_has_instance(&handler, &constructor, &value);
    }
    if !crate::value::is_object(&value) {
        return Ok(false);
    }
    if (matches!(crate::execute::get_property(&constructor, "name"), Value::String(name) if name == "DOMException")
        || intrinsic_builtin(&constructor) == Some(Builtin::Error))
        && matches!(
            crate::execute::get_property(&value, "\0domexception"),
            Value::Boolean(true)
        )
    {
        return Ok(true);
    }
    if let Some(result) = builtin_instanceof(&value, &constructor) {
        return Ok(result);
    }
    // A foreign realm's plain Error must not satisfy the root Error
    // constructor even though both values carry the same builtin tag.
    if intrinsic_builtin(&constructor) == Some(Builtin::Error)
        && error_constructor_builtin(&value) == Some(Builtin::Error)
    {
        return Ok(builtin_error_instance(&value, &constructor));
    }
    if let Some(handler) = has_instance_handler(&constructor)? {
        return call_has_instance(&handler, &constructor, &value);
    }
    ordinary_instanceof(&value, &constructor)
}

fn call_has_instance(handler: &Value, constructor: &Value, value: &Value) -> Result<bool, VmError> {
    let arguments = [value.clone()];
    let result = crate::functions::execute_target(handler, constructor, &arguments)?;
    Ok(is_truthy(&result))
}

fn dereference_binding(value: &Value) -> Value {
    match value {
        Value::BindingCell(cell) => {
            crate::module_bindings::ModuleBindingCell::from_shared(std::rc::Rc::clone(cell)).get()
        }
        _ => value.clone(),
    }
}

fn builtin_error_instance(value: &Value, constructor: &Value) -> bool {
    let Some(constructor) = intrinsic_builtin(constructor) else {
        return false;
    };
    if !matches!(
        constructor,
        Builtin::Error
            | Builtin::DOMException
            | Builtin::RangeError
            | Builtin::ReferenceError
            | Builtin::SyntaxError
            | Builtin::EvalError
            | Builtin::URIError
            | Builtin::AggregateError
            | Builtin::TypeError
    ) || error_constructor_builtin(value) != Some(constructor)
    {
        return false;
    }
    // Error objects carry the prototype of the realm that created them.
    // Matching only the builtin tag would make a foreign-realm Error pass
    // an instanceof check against the root realm constructor.
    let expected = crate::execute::get_property(&Value::Builtin(constructor), "prototype");
    let actual = crate::execute::get_property(value, "\0prototype");
    crate::builtins::same_value(Some(&expected), Some(&actual))
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
    let handler = unbind_function_prototype_handler(&handler);
    Ok(Some(handler))
}

// `Get(C, @@hasInstance)` walks the prototype chain when C is a subclass.
// For a built-in parent like Set, the chain yields a `BoundFunction` whose
// `this` is bound to the parent (Set), not to C. Per ES `InstanceofOperator`,
// the handler is invoked with `this = C`, so we must rebind it before the
// call. We only rebind when the target is the canonical `Function.prototype
// [@@hasInstance]` because that handler is the only one we know to be safe
// to redirect; user-defined handlers keep their original receiver.
fn unbind_function_prototype_handler(handler: &Value) -> Value {
    let Value::BoundFunction(bound) = handler else {
        return handler.clone();
    };
    if !matches!(
        bound.target,
        Value::Builtin(Builtin::FunctionPrototypeHasInstance)
    ) {
        return handler.clone();
    }
    if !bound.arguments.is_empty() {
        return handler.clone();
    }
    Value::Builtin(Builtin::FunctionPrototypeHasInstance)
}

fn has_temporal_plain_date_fields(properties: &crate::value::ObjectData) -> bool {
    has_property(properties, "year")
        && has_property(properties, "month")
        && has_property(properties, "day")
}

fn builtin_instanceof(value: &Value, constructor: &Value) -> Option<bool> {
    let constructor = intrinsic_builtin(constructor)?;
    if constructor == Builtin::Function {
        return Some(function_instanceof(value));
    }
    Some(match (value, constructor) {
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
        (Value::Object(properties), Builtin::Date) if has_property(properties, "timeValue") => true,
        (Value::Object(properties), Builtin::RegExp) if has_property(properties, "source") => true,
        (Value::Map(data), Builtin::Map | Builtin::WeakMap) => {
            data.weak == (constructor == Builtin::WeakMap)
        }
        (Value::ArrayBuffer(data), Builtin::SharedArrayBuffer | Builtin::ArrayBuffer) => {
            data.shared == (constructor == Builtin::SharedArrayBuffer)
        }
        (Value::Set(data), Builtin::Set | Builtin::WeakSet) => {
            data.weak == (constructor == Builtin::WeakSet)
        }
        (Value::Object(properties), Builtin::WeakRef) => has_property(properties, "\0weakref"),
        (Value::Object(properties), Builtin::ShadowRealm) => is_shadow_realm(properties),
        (Value::Object(properties), Builtin::TemporalPlainDate) => {
            has_temporal_plain_date_fields(properties)
        }
        _ => return None,
    })
}

fn function_instanceof(value: &Value) -> bool {
    matches!(value, Value::Function(_) | Value::BoundFunction(_))
        || matches!(value, Value::Builtin(_) if instanceof_callable(value))
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
    Ok(Value::Boolean(ordinary_instanceof(value, constructor)?))
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

fn is_shadow_realm(properties: &crate::value::ObjectData) -> bool {
    properties.iter().any(|(name, value)| {
        name == "\0prototype" && value == Value::Builtin(Builtin::ShadowRealmPrototype)
    })
}

fn has_property(properties: &crate::value::ObjectData, key: &str) -> bool {
    properties.iter().any(|(name, _)| name == key)
}

fn is_error_subclass(value: &Value, constructor: &Value) -> bool {
    let (Some(Builtin::Error), Some(actual)) = (
        intrinsic_builtin(constructor),
        error_constructor_builtin(value),
    ) else {
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

fn error_constructor_builtin(value: &Value) -> Option<Builtin> {
    if let Some(constructor) = own_constructor(value).as_ref().and_then(intrinsic_builtin) {
        return Some(constructor);
    }
    let Value::Object(properties) = value else {
        return None;
    };
    let prototype = properties
        .iter()
        .rev()
        .find_map(|(name, value)| (name == "\0prototype").then_some(value))?;
    match intrinsic_builtin(&prototype)? {
        Builtin::ErrorPrototype => Some(Builtin::Error),
        Builtin::RangeErrorPrototype => Some(Builtin::RangeError),
        Builtin::ReferenceErrorPrototype => Some(Builtin::ReferenceError),
        Builtin::SyntaxErrorPrototype => Some(Builtin::SyntaxError),
        Builtin::EvalErrorPrototype => Some(Builtin::EvalError),
        Builtin::URIErrorPrototype => Some(Builtin::URIError),
        Builtin::AggregateErrorPrototype => Some(Builtin::AggregateError),
        Builtin::TypeErrorPrototype => Some(Builtin::TypeError),
        _ => None,
    }
}

fn instanceof_callable(value: &Value) -> bool {
    match value {
        Value::BoundFunction(bound) if crate::vm::is_intrinsic_bound(bound) => {
            instanceof_callable(&bound.target)
        }
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
    if matches!(value, Value::ObjectAlias(_)) {
        let object = crate::builtins::object::resolve_object_alias(value.clone());
        if matches!(object, Value::Null) {
            return None;
        }
        return internal_prototype(&object);
    }
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
            .prototype()
            .or_else(|| values.property("\0prototype"))
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
        Value::Generator(generator) => generator_instance_prototype(generator),
        Value::Iterator(_) => Some(crate::collections::iterator::prototype_of(value)),
        Value::Builtin(builtin) => builtin_prototype_parent(*builtin),
        Value::Function(function) => Some(Value::Builtin(if function.is_async {
            Builtin::AsyncFunctionPrototype
        } else {
            Builtin::FunctionPrototype
        })),
        Value::BoundFunction(_) => crate::builtins::object::get_prototype_of(Some(value)).ok(),
        _ => None,
    }
}

fn generator_instance_prototype(generator: &crate::value::GeneratorData) -> Option<Value> {
    Some(crate::construct::get_prototype_from_constructor(
        &crate::value::Value::Function(std::rc::Rc::clone(&generator.function)),
        |realm| crate::construct::generator_kind_prototype(&generator.function, realm),
    ))
}

fn builtin_prototype_parent(builtin: Builtin) -> Option<Value> {
    if matches!(
        builtin,
        Builtin::GeneratorFunctionPrototype
            | Builtin::AsyncGeneratorFunctionPrototype
            | Builtin::AsyncFunctionPrototype
    ) {
        return Some(Value::Builtin(Builtin::FunctionPrototype));
    }
    if matches!(
        builtin,
        Builtin::ArrayIteratorPrototype
            | Builtin::SetIteratorPrototype
            | Builtin::MapIteratorPrototype
    ) {
        return Some(Value::Builtin(Builtin::IteratorPrototype));
    }
    if builtin == Builtin::IteratorPrototype {
        return Some(Value::Builtin(Builtin::ObjectPrototype));
    }
    if builtin == Builtin::AbstractModuleSource {
        return Some(Value::Builtin(Builtin::FunctionPrototype));
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
            | Builtin::IntlPluralRulesPrototype
            | Builtin::DisposableStackPrototype
            | Builtin::AbstractModuleSourcePrototype
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
            .find(|(name, _)| name == "\0prototype" || name == "prototype")
            .map(|(_, prototype)| dereference_binding(&prototype)),
        Value::Function(function) => {
            let properties = function.properties.borrow();
            properties
                .iter()
                .rev()
                .find(|(name, _)| name == "\0function_prototype")
                .or_else(|| {
                    properties
                        .iter()
                        .rev()
                        .find(|(name, _)| name == "\0prototype")
                })
                .map(|(_, prototype)| dereference_binding(prototype))
        }
        Value::BoundFunction(function) => {
            let properties = function.properties.borrow();
            properties
                .iter()
                .rev()
                .find(|(name, _)| name == "\0function_prototype")
                .or_else(|| {
                    properties
                        .iter()
                        .rev()
                        .find(|(name, _)| name == "\0prototype" || name == "prototype")
                })
                .map(|(_, prototype)| dereference_binding(prototype))
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
