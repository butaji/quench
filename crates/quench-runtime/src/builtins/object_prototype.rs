pub(crate) fn get_prototype_of(value: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    let value = require_object_coercible(value)?;
    if matches!(value, Value::Proxy(_)) {
        return crate::proxy::proxy_get_prototype_of(value);
    }
    Ok(prototype_for_value(value))
}

fn prototype_for_value(value: &Value) -> Value {
    if let Value::Function(function) = value {
        if let Some((_, prototype)) = function
            .properties
            .borrow()
            .iter()
            .find(|(key, _)| key == "\0function_prototype")
        {
            return prototype.clone();
        }
    }
    if let Value::BoundFunction(function) = value {
        if let Some((_, prototype)) = function
            .properties
            .borrow()
            .iter()
            .find(|(key, _)| key == "\0function_prototype")
        {
            return prototype.clone();
        }
    }
    if let Some(prototype) = slot_prototype(value) {
        return prototype;
    }
    match value {
        Value::Builtin(Builtin::ObjectPrototype) => Value::Null,
        Value::Builtin(
            Builtin::Math
            | Builtin::Reflect
            | Builtin::Json
            | Builtin::DisposableStackPrototype
            | Builtin::AsyncDisposableStackPrototype,
        ) => Value::Builtin(Builtin::ObjectPrototype),
        Value::Builtin(builtin) if is_typed_array_constructor(*builtin) => {
            Value::Builtin(Builtin::TypedArray)
        }
        Value::Builtin(builtin) if is_typed_array_prototype(*builtin) => {
            Value::Builtin(Builtin::TypedArray)
        }
        Value::Builtin(
            Builtin::RangeErrorPrototype
            | Builtin::TypeErrorPrototype
            | Builtin::EvalErrorPrototype
            | Builtin::ReferenceErrorPrototype
            | Builtin::SyntaxErrorPrototype
            | Builtin::URIErrorPrototype,
        ) => Value::Builtin(Builtin::ErrorPrototype),
        Value::Builtin(Builtin::AsyncFunctionPrototype) => {
            Value::Builtin(Builtin::FunctionPrototype)
        }
        Value::Builtin(builtin) if is_intrinsic_prototype(*builtin) => {
            Value::Builtin(Builtin::ObjectPrototype)
        }
        Value::Builtin(Builtin::FunctionPrototype) => Value::Builtin(Builtin::ObjectPrototype),
        Value::Builtin(Builtin::SuppressedError) => Value::Builtin(Builtin::Error),
        Value::Builtin(Builtin::AggregateError) => Value::Builtin(Builtin::Error),
        Value::Builtin(Builtin::SuppressedErrorPrototype) => {
            Value::Builtin(Builtin::ErrorPrototype)
        }
        Value::Builtin(Builtin::AggregateErrorPrototype) => Value::Builtin(Builtin::ErrorPrototype),
        Value::Builtin(
            builtin @ (Builtin::ArrayIteratorPrototype
            | Builtin::StringIteratorPrototype
            | Builtin::RegExpStringIteratorPrototype
            | Builtin::SetIteratorPrototype
            | Builtin::MapIteratorPrototype
            | Builtin::IteratorPrototype),
        ) => iterator_prototype(*builtin),
        _ => prototype_for_value_tail(value),
    }
}

fn prototype_for_value_tail(value: &Value) -> Value {
    match value {
        Value::Function(function) => {
            if function.is_async {
                if let Some((_, prototype)) = function
                    .properties
                    .borrow()
                    .iter()
                    .rev()
                    .find(|(name, _)| name == "\0prototype")
                {
                    return prototype.clone();
                }
                return crate::vm::realm_id_for_global_value(&function.captures.get(0))
                    .map(|realm| {
                        crate::vm::realm_intrinsic_for(realm, Builtin::AsyncFunctionPrototype)
                    })
                    .unwrap_or(Value::Builtin(Builtin::AsyncFunctionPrototype));
            }
            function.properties.borrow().iter().rev()
                .find_map(|(name, value)| (name == "\0function_prototype").then(|| value.clone()))
                .unwrap_or_else(|| internal_prototype(&function.properties.borrow(), Builtin::FunctionPrototype))
        }
        Value::Builtin(Builtin::AsyncFunction) => Value::Builtin(Builtin::Function),
        Value::Builtin(
            Builtin::RangeError
            | Builtin::TypeError
            | Builtin::EvalError
            | Builtin::ReferenceError
            | Builtin::SyntaxError
            | Builtin::URIError
            | Builtin::AggregateError
            | Builtin::SuppressedError,
        ) => Value::Builtin(Builtin::Error),
        Value::BoundFunction(bound)
            if crate::vm::is_intrinsic_bound(bound)
                && matches!(bound.target, Value::Builtin(Builtin::AsyncFunction)) =>
        {
            crate::vm::realm_intrinsic_for(bound.realm, Builtin::Function)
        }
        Value::BoundFunction(bound)
            if crate::vm::is_intrinsic_bound(bound)
                && matches!(
                    bound.target,
                    Value::Builtin(Builtin::GeneratorFunction)
                        | Value::Builtin(Builtin::AsyncGeneratorFunction)
                ) =>
        {
            let prototype = match bound.target {
                Value::Builtin(Builtin::GeneratorFunction) => Builtin::GeneratorFunctionPrototype,
                _ => Builtin::AsyncGeneratorFunctionPrototype,
            };
            crate::vm::realm_intrinsic_for(bound.realm, prototype)
        }
        Value::BoundFunction(bound)
            if crate::vm::is_intrinsic_bound(bound)
                && matches!(bound.target, Value::Builtin(builtin) if is_intrinsic_prototype(builtin)) =>
        {
            crate::vm::realm_intrinsic_for(bound.realm, Builtin::ObjectPrototype)
        }
        Value::Builtin(_) | Value::BoundFunction(_) => Value::Builtin(Builtin::FunctionPrototype),
        Value::Promise(_) => Value::Builtin(Builtin::PromisePrototype),
        Value::Generator(generator) => generator_prototype(generator),
        Value::Iterator(_) => crate::collections::iterator::prototype_of(value),
        Value::Array(values) if values.is_arguments() => Value::Builtin(Builtin::ObjectPrototype),
        Value::Array(values) => values.prototype().unwrap_or(Value::Builtin(Builtin::ArrayPrototype)),
        Value::String(value) if crate::conversion::is_symbol_string(value) => {
            Value::Builtin(Builtin::SymbolPrototype)
        }
        Value::Object(properties) => internal_prototype(properties, Builtin::ObjectPrototype),
        _ => typed_array_prototype_for_value(value),
    }
}

fn typed_array_prototype_for_value(value: &Value) -> Value {
    match value {
        Value::Float64Array(_) => Value::Builtin(Builtin::Float64ArrayPrototype),
        Value::Float32Array(_) => Value::Builtin(Builtin::Float32ArrayPrototype),
        Value::Int8Array(_) => Value::Builtin(Builtin::Int8ArrayPrototype),
        Value::Int16Array(_) => Value::Builtin(Builtin::Int16ArrayPrototype),
        Value::Int32Array(_) => Value::Builtin(Builtin::Int32ArrayPrototype),
        Value::Uint8Array(_) => Value::Builtin(Builtin::Uint8ArrayPrototype),
        Value::Uint16Array(_) => Value::Builtin(Builtin::Uint16ArrayPrototype),
        Value::Uint32Array(_) => Value::Builtin(Builtin::Uint32ArrayPrototype),
        Value::Uint8ClampedArray(_) => Value::Builtin(Builtin::Uint8ClampedArrayPrototype),
        Value::BigInt64Array(_) => Value::Builtin(Builtin::BigInt64ArrayPrototype),
        Value::BigUint64Array(_) => Value::Builtin(Builtin::BigUint64ArrayPrototype),
        _ => Value::Null,
    }
}

fn slot_prototype(value: &Value) -> Option<Value> {
    Some(match value {
        Value::ArrayBuffer(buffer) => {
            buffer
                .prototype()
                .unwrap_or(Value::Builtin(if buffer.shared {
                    Builtin::SharedArrayBufferPrototype
                } else {
                    Builtin::ArrayBufferPrototype
                }))
        }
        Value::DataView(view) => view
            .prototype()
            .unwrap_or(Value::Builtin(Builtin::DataViewPrototype)),
        Value::Map(data) => data.prototype().unwrap_or(Value::Builtin(if data.weak {
            Builtin::WeakMapPrototype
        } else {
            Builtin::MapPrototype
        })),
        Value::Set(data) => data.prototype().unwrap_or(Value::Builtin(if data.weak {
            Builtin::WeakSetPrototype
        } else {
            Builtin::SetPrototype
        })),
        _ => return None,
    })
}

fn generator_prototype(generator: &crate::value::GeneratorData) -> Value {
    let properties = generator.function.properties.borrow();
    let prototype = properties
        .iter()
        .rev()
        .find_map(|(name, value)| (name == "prototype").then(|| value.clone()));
    if prototype.as_ref().is_some_and(crate::value::is_object) {
        return prototype.unwrap_or(Value::Undefined);
    }
    properties
        .iter()
        .rev()
        .find_map(|(name, value)| (name == "\0prototype").then(|| value.clone()))
        .map_or(Value::Builtin(Builtin::ObjectPrototype), |value| {
            crate::execute::get_property(&value, "prototype")
        })
}

fn internal_prototype(properties: &[(String, Value)], fallback: Builtin) -> Value {
    properties
        .iter()
        .rev()
        .find_map(|(name, value)| (name == "\0prototype").then(|| value.clone()))
        .unwrap_or(Value::Builtin(fallback))
}

fn iterator_prototype(builtin: Builtin) -> Value {
    if matches!(
        builtin,
        Builtin::ArrayIteratorPrototype
            | Builtin::StringIteratorPrototype
            | Builtin::RegExpStringIteratorPrototype
            | Builtin::SetIteratorPrototype
            | Builtin::MapIteratorPrototype
    ) {
        return Value::Builtin(Builtin::IteratorPrototype);
    }
    Value::Builtin(Builtin::ObjectPrototype)
}

pub(crate) fn is_prototype_of(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let Some(value) = arguments
        .first()
        .filter(|value| crate::value::is_object(value))
    else {
        return Ok(Value::Boolean(false));
    };
    let prototype = receiver
        .filter(|value| crate::value::is_object(value))
        .ok_or_else(|| {
            crate::value::error::throw_type_error(
                "Object.prototype.isPrototypeOf called on null or undefined",
            )
        })?;
    prototype_chain_contains(value, prototype).map(Value::Boolean)
}

fn prototype_chain_contains(
    value: &Value,
    prototype: &Value,
) -> Result<bool, crate::execute::VmError> {
    let mut seen = Vec::new();
    let mut current = resolve_object_alias(get_prototype_of(Some(value))?);
    while !matches!(current, Value::Null) {
        if crate::builtins::same_value(Some(&current), Some(prototype)) {
            return Ok(true);
        }
        if seen
            .iter()
            .any(|seen| crate::builtins::same_value(Some(&current), Some(seen)))
        {
            return Ok(false);
        }
        seen.push(current.clone());
        current = resolve_object_alias(get_prototype_of(Some(&current))?);
    }
    Ok(false)
}

pub(crate) fn resolve_object_alias(value: Value) -> Value {
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

pub(crate) fn define_legacy_accessor(
    receiver: Option<&Value>,
    arguments: &[Value],
    field: &str,
) -> Result<Value, crate::execute::VmError> {
    let target = require_object_receiver(receiver)?;
    let key = crate::conversion::to_property_key(arguments.first().unwrap_or(&Value::Undefined))?;
    let accessor = arguments.get(1).cloned().unwrap_or(Value::Undefined);
    if !crate::conversion::is_callable(&accessor) {
        return Err(crate::value::error::throw_type_error(
            "Accessor must be callable",
        ));
    }
    let descriptor = vec![
        (field.to_string(), accessor),
        ("enumerable".to_string(), Value::Boolean(true)),
        ("configurable".to_string(), Value::Boolean(true)),
    ];
    let result = crate::builtins::define_own_property(target, &key, &descriptor)?;
    crate::locals::replace_value(target, &result);
    Ok(Value::Undefined)
}

pub(crate) fn lookup_legacy_accessor(
    receiver: Option<&Value>,
    arguments: &[Value],
    field: &str,
) -> Result<Value, crate::execute::VmError> {
    let target = require_object_receiver(receiver)?;
    let key = crate::conversion::to_property_key(arguments.first().unwrap_or(&Value::Undefined))?;
    Ok(crate::property_define::accessor(target, &key, field).unwrap_or(Value::Undefined))
}

fn require_object_receiver(receiver: Option<&Value>) -> Result<&Value, crate::execute::VmError> {
    receiver
        .filter(|value| crate::value::is_object(value))
        .ok_or_else(|| crate::value::error::throw_type_error("Object receiver required"))
}

pub(crate) fn from_entries(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let iterable = arguments.first().cloned().unwrap_or(Value::Undefined);
    let result = std::cell::RefCell::new(Value::Object(std::rc::Rc::new(
        crate::value::ObjectData::new(Vec::new()),
    )));
    crate::collections::iterator::for_each_iterable(iterable, |entry| add_entry(&result, entry))?;
    Ok(result.into_inner())
}

fn add_entry(
    result: &std::cell::RefCell<Value>,
    entry: Value,
) -> Result<(), crate::execute::VmError> {
    if !crate::value::is_object(&entry) {
        return Err(crate::value::error::throw_type_error(
            "Object.fromEntries iterator value is not an object",
        ));
    }
    let raw_key = crate::execute::get_property_result(&entry, "0")?;
    let value = crate::execute::get_property_result(&entry, "1")?;
    let key = crate::conversion::to_property_key(&raw_key)?;
    let current = result.borrow().clone();
    let descriptor = vec![
        ("value".to_string(), value),
        ("writable".to_string(), Value::Boolean(true)),
        ("enumerable".to_string(), Value::Boolean(true)),
        ("configurable".to_string(), Value::Boolean(true)),
    ];
    *result.borrow_mut() = crate::builtins::define_own_property(&current, &key, &descriptor)?;
    Ok(())
}

pub(crate) fn group_by(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let iterable = arguments.first().cloned().unwrap_or(Value::Undefined);
    let callback = arguments.get(1).cloned().unwrap_or(Value::Undefined);
    if !crate::conversion::is_callable(&callback) {
        return Err(crate::value::error::throw_type_error(
            "Object.groupBy callback is not callable",
        ));
    }
    let result = std::cell::RefCell::new(Value::Object(std::rc::Rc::new(
        crate::value::ObjectData::new(vec![("\0prototype".into(), Value::Null)]),
    )));
    let mut index = 0usize;
    crate::collections::iterator::for_each_iterable(iterable, |value| {
        add_group_entry(&result, &callback, &mut index, value)
    })?;
    Ok(result.into_inner())
}

fn add_group_entry(
    result: &std::cell::RefCell<Value>,
    callback: &Value,
    index: &mut usize,
    value: Value,
) -> Result<(), crate::execute::VmError> {
    let key_value = crate::functions::execute_target(
        callback,
        &Value::Undefined,
        &[value.clone(), Value::Number(*index as f64)],
    )?;
    *index += 1;
    let key = crate::conversion::to_property_key(&key_value)?;
    add_group_value(result, &key, value)
}

fn add_group_value(
    result: &std::cell::RefCell<Value>,
    key: &str,
    value: Value,
) -> Result<(), crate::execute::VmError> {
    let current = result.borrow().clone();
    let values = grouped_values(&current, key, value)?;
    let descriptor = vec![
        ("value".to_string(), values),
        ("writable".to_string(), Value::Boolean(true)),
        ("enumerable".to_string(), Value::Boolean(true)),
        ("configurable".to_string(), Value::Boolean(true)),
    ];
    *result.borrow_mut() = crate::builtins::define_own_property(&current, key, &descriptor)?;
    Ok(())
}

fn grouped_values(
    current: &Value,
    key: &str,
    value: Value,
) -> Result<Value, crate::execute::VmError> {
    let previous = crate::execute::get_property_result(current, key)?;
    Ok(match previous {
        Value::Array(array) => {
            let mut values = array.snapshot();
            values.push(value);
            Value::array(values)
        }
        _ => Value::array(vec![value]),
    })
}

pub(crate) fn is_intrinsic_prototype(builtin: Builtin) -> bool {
    matches!(
        builtin,
        Builtin::NumberPrototype
            | Builtin::BooleanPrototype
            | Builtin::StringPrototype
            | Builtin::MapPrototype
            | Builtin::SetPrototype
            | Builtin::WeakMapPrototype
            | Builtin::WeakSetPrototype
            | Builtin::SharedArrayBufferPrototype
            | Builtin::WeakRefPrototype
            | Builtin::FinalizationRegistryPrototype
            | Builtin::BigIntPrototype
            | Builtin::ErrorPrototype
            | Builtin::RangeErrorPrototype
            | Builtin::TypeErrorPrototype
            | Builtin::EvalErrorPrototype
            | Builtin::ReferenceErrorPrototype
            | Builtin::SyntaxErrorPrototype
            | Builtin::URIErrorPrototype
            | Builtin::AsyncFunctionPrototype
            | Builtin::AbstractModuleSourcePrototype
            | Builtin::GeneratorFunctionPrototype
            | Builtin::AsyncGeneratorFunctionPrototype
            | Builtin::ShadowRealmPrototype
            | Builtin::IntlCollatorPrototype
            | Builtin::IntlDateTimeFormatPrototype
            | Builtin::IntlNumberFormatPrototype
            | Builtin::IntlPluralRulesPrototype
            | Builtin::IntlListFormatPrototype
            | Builtin::IntlSegmenterPrototype
    )
}

fn is_typed_array_constructor(builtin: Builtin) -> bool {
    matches!(
        builtin,
        Builtin::Float64Array
            | Builtin::Float32Array
            | Builtin::Int8Array
            | Builtin::Int16Array
            | Builtin::Int32Array
            | Builtin::Uint8Array
            | Builtin::Uint16Array
            | Builtin::Uint32Array
            | Builtin::Uint8ClampedArray
    )
}

fn is_typed_array_prototype(builtin: Builtin) -> bool {
    matches!(
        builtin,
        Builtin::Float64ArrayPrototype
            | Builtin::Float32ArrayPrototype
            | Builtin::Int8ArrayPrototype
            | Builtin::Int16ArrayPrototype
            | Builtin::Int32ArrayPrototype
            | Builtin::Uint8ArrayPrototype
            | Builtin::Uint16ArrayPrototype
            | Builtin::Uint32ArrayPrototype
            | Builtin::Uint8ClampedArrayPrototype
            | Builtin::BigInt64ArrayPrototype
            | Builtin::BigUint64ArrayPrototype
    )
}
