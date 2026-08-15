pub(crate) fn get_prototype_of(value: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    let value = require_object_coercible(value)?;
    if matches!(value, Value::Proxy(_)) {
        return crate::proxy::proxy_get_prototype_of(value);
    }
    Ok(prototype_for_value(value))
}

fn prototype_for_value(value: &Value) -> Value {
    if let Value::BindingCell(cell) = value {
        return prototype_for_value(&cell.borrow());
    }
    if let Value::ObjectAlias(alias) = value {
        return alias
            .0
            .borrow()
            .upgrade()
            .map(|object| prototype_for_value(&Value::Object(object)))
            .unwrap_or(Value::Null);
    }
    if let Some(prototype) = slot_prototype(value) {
        return prototype;
    }
    match value {
        Value::Builtin(Builtin::ObjectPrototype) => Value::Null,
        Value::Builtin(
            Builtin::Error
            | Builtin::EvalError
            | Builtin::RangeError
            | Builtin::ReferenceError
            | Builtin::SyntaxError
            | Builtin::TypeError
            | Builtin::URIError
            | Builtin::AggregateError,
        ) => Value::Builtin(Builtin::Error),
        Value::Builtin(
            Builtin::EvalErrorPrototype
            | Builtin::RangeErrorPrototype
            | Builtin::ReferenceErrorPrototype
            | Builtin::SyntaxErrorPrototype
            | Builtin::TypeErrorPrototype
            | Builtin::URIErrorPrototype
            | Builtin::AggregateErrorPrototype,
        ) => Value::Builtin(Builtin::ErrorPrototype),
        Value::Builtin(Builtin::Math | Builtin::Reflect | Builtin::Json | Builtin::DisposableStackPrototype | Builtin::AsyncDisposableStackPrototype) => {
            Value::Builtin(Builtin::ObjectPrototype)
        }
        Value::Builtin(builtin) if is_typed_array_constructor(*builtin) => {
            Value::Builtin(Builtin::TypedArray)
        }
        Value::Builtin(Builtin::AsyncFunctionPrototype) => Value::Builtin(Builtin::FunctionPrototype),
        Value::Builtin(Builtin::AsyncFunction) => Value::Builtin(Builtin::Function),
        Value::Builtin(Builtin::AsyncGeneratorPrototype) => {
            Value::Builtin(Builtin::AsyncIteratorPrototype)
        }
        Value::Builtin(builtin) if is_intrinsic_prototype(*builtin) => {
            Value::Builtin(Builtin::ObjectPrototype)
        }
        Value::Builtin(Builtin::FunctionPrototype) => Value::Builtin(Builtin::ObjectPrototype),
        Value::Builtin(Builtin::AggregateError) => Value::Builtin(Builtin::Error),
        Value::Builtin(Builtin::AggregateErrorPrototype) => Value::Builtin(Builtin::ErrorPrototype),
        Value::Builtin(Builtin::SuppressedError) => Value::Builtin(Builtin::Error),
        Value::Builtin(Builtin::SuppressedErrorPrototype) => {
            Value::Builtin(Builtin::ErrorPrototype)
        }
        Value::Builtin(
            Builtin::RangeErrorPrototype
            | Builtin::ReferenceErrorPrototype
            | Builtin::SyntaxErrorPrototype
            | Builtin::EvalErrorPrototype
            | Builtin::URIErrorPrototype
            | Builtin::TypeErrorPrototype,
        ) => Value::Builtin(Builtin::ErrorPrototype),
        Value::Builtin(builtin @ (Builtin::ArrayIteratorPrototype | Builtin::RegExpStringIteratorPrototype | Builtin::SetIteratorPrototype | Builtin::MapIteratorPrototype | Builtin::IteratorPrototype)) => iterator_prototype(*builtin),
        Value::Function(function) if function.kind == crate::ops::FunctionKind::Generator => {
            Value::Builtin(Builtin::GeneratorFunctionPrototype)
        }
        Value::Function(function) => {
            internal_prototype(&function.properties.borrow(), Builtin::FunctionPrototype)
        }
        Value::Builtin(_) | Value::BoundFunction(_) => Value::Builtin(Builtin::FunctionPrototype),
        Value::Promise(_) => Value::Builtin(Builtin::PromisePrototype),
        Value::Generator(generator) => generator_prototype(generator),
        Value::Iterator(_) => crate::collections::iterator::prototype_of(value),
        Value::Array(values) if values.is_arguments() => Value::Builtin(Builtin::ObjectPrototype),
        Value::Array(_) => Value::Builtin(Builtin::ArrayPrototype),
        Value::String(value) if crate::conversion::is_symbol_string(value) => {
            Value::Builtin(Builtin::SymbolPrototype)
        }
        Value::Object(properties) => internal_prototype(properties, Builtin::ObjectPrototype),
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
    };
    result
}

fn function_prototype(function: &crate::value::FunctionValue) -> Value {
    let builtin = match (function.kind, function.is_async) {
        (crate::ops::FunctionKind::Generator, true) => Builtin::AsyncGeneratorFunctionPrototype,
        (crate::ops::FunctionKind::Generator, false) => Builtin::GeneratorFunctionPrototype,
        (_, true) => Builtin::AsyncFunctionPrototype,
        (_, false) => Builtin::FunctionPrototype,
    };
    internal_prototype(&function.properties.borrow(), builtin)
}

fn slot_prototype(value: &Value) -> Option<Value> {
    Some(match value {
        Value::ArrayBuffer(buffer) => buffer
            .prototype()
            .unwrap_or(Value::Builtin(Builtin::ArrayBufferPrototype)),
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

pub(crate) fn generator_prototype(generator: &crate::value::GeneratorData) -> Value {
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
    crate::collections::iterator::for_each_iterable(iterable, |entry| {
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
    })?;
    Ok(result.into_inner())
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
        let key_value = crate::functions::execute_target(
            &callback,
            &Value::Undefined,
            &[value.clone(), Value::Number(index as f64)],
        )?;
        index += 1;
        let key = crate::conversion::to_property_key(&key_value)?;
        add_group_value(&result, &key, value)?;
        Ok(())
    })?;
    Ok(result.into_inner())
}

fn add_group_value(
    result: &std::cell::RefCell<Value>,
    key: &str,
    value: Value,
) -> Result<(), crate::execute::VmError> {
    let current = result.borrow().clone();
    let previous = crate::execute::get_property_result(&current, key)?;
    let values = match previous {
        Value::Array(array) => {
            let mut values = array.snapshot();
            values.push(value);
            Value::array(values)
        }
        _ => Value::array(vec![value]),
    };
    let descriptor = vec![
        ("value".to_string(), values),
        ("writable".to_string(), Value::Boolean(true)),
        ("enumerable".to_string(), Value::Boolean(true)),
        ("configurable".to_string(), Value::Boolean(true)),
    ];
    *result.borrow_mut() = crate::builtins::define_own_property(&current, key, &descriptor)?;
    Ok(())
}

pub(crate) fn is_intrinsic_prototype(builtin: Builtin) -> bool {
    matches!(
        builtin,
        Builtin::ObjectPrototype
            | Builtin::ArrayPrototype
            | Builtin::NumberPrototype
            | Builtin::RegExpPrototype
            | Builtin::DatePrototype
            | Builtin::IteratorPrototype
            | Builtin::ArrayIteratorPrototype
            | Builtin::SetIteratorPrototype
            | Builtin::MapIteratorPrototype
            | Builtin::RegExpStringIteratorPrototype
            | Builtin::PromisePrototype
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
            | Builtin::EvalErrorPrototype
            | Builtin::RangeErrorPrototype
            | Builtin::ReferenceErrorPrototype
            | Builtin::SyntaxErrorPrototype
            | Builtin::TypeErrorPrototype
            | Builtin::URIErrorPrototype
            | Builtin::AggregateErrorPrototype
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
