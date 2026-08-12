pub(crate) fn get_prototype_of(value: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    let value = require_object_coercible(value)?;
    if matches!(value, Value::Proxy(_)) {
        return crate::proxy::proxy_get_prototype_of(value);
    }
    Ok(match value {
        Value::Builtin(builtin) if is_typed_array_constructor(*builtin) => {
            Value::Builtin(Builtin::TypedArray)
        }
        Value::Builtin(builtin) if is_intrinsic_prototype(*builtin) => {
            Value::Builtin(Builtin::ObjectPrototype)
        }
        Value::Builtin(_) | Value::Function(_) | Value::BoundFunction(_) => {
            Value::Builtin(Builtin::FunctionPrototype)
        }
        Value::Promise(_) => Value::Builtin(Builtin::PromisePrototype),
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
        Value::Generator(_) => Value::Builtin(Builtin::ObjectPrototype),
        Value::Iterator(_) => Value::Builtin(Builtin::IteratorPrototype),
        Value::Array(values) if values.is_arguments() => Value::Builtin(Builtin::ObjectPrototype),
        Value::Array(_) => Value::Builtin(Builtin::ArrayPrototype),
        Value::Object(properties) => properties
            .iter()
            .rev()
            .find_map(|(name, value)| (name == "\0prototype").then(|| value.clone()))
            .unwrap_or(Value::Builtin(Builtin::ObjectPrototype)),
        _ => Value::Null,
    })
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
    let mut current = get_prototype_of(Some(value))?;
    while !matches!(current, Value::Null) {
        if crate::builtins::same_value(Some(&current), Some(prototype)) {
            return Ok(Value::Boolean(true));
        }
        current = get_prototype_of(Some(&current))?;
    }
    Ok(Value::Boolean(false))
}

pub(crate) fn define_legacy_accessor(
    receiver: Option<&Value>, arguments: &[Value], field: &str,
) -> Result<Value, crate::execute::VmError> {
    let target = require_object_receiver(receiver)?;
    let key = crate::conversion::to_property_key(arguments.first().unwrap_or(&Value::Undefined))?;
    let accessor = arguments.get(1).cloned().unwrap_or(Value::Undefined);
    if !crate::conversion::is_callable(&accessor) {
        return Err(crate::value::error::throw_type_error("Accessor must be callable"));
    }
    let descriptor = vec![(field.to_string(), accessor),
        ("enumerable".to_string(), Value::Boolean(true)),
        ("configurable".to_string(), Value::Boolean(true))];
    let result = crate::builtins::define_own_property(target, &key, &descriptor)?;
    crate::locals::replace_value(target, &result);
    Ok(Value::Undefined)
}

pub(crate) fn lookup_legacy_accessor(
    receiver: Option<&Value>, arguments: &[Value], field: &str,
) -> Result<Value, crate::execute::VmError> {
    let target = require_object_receiver(receiver)?;
    let key = crate::conversion::to_property_key(arguments.first().unwrap_or(&Value::Undefined))?;
    Ok(crate::property_define::accessor(target, &key, field).unwrap_or(Value::Undefined))
}

fn require_object_receiver(receiver: Option<&Value>) -> Result<&Value, crate::execute::VmError> {
    receiver.filter(|value| crate::value::is_object(value)).ok_or_else(|| {
        crate::value::error::throw_type_error("Object receiver required")
    })
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

fn is_intrinsic_prototype(builtin: Builtin) -> bool {
    matches!(
        builtin,
        Builtin::MapPrototype
            | Builtin::SetPrototype
            | Builtin::WeakMapPrototype
            | Builtin::WeakSetPrototype
            | Builtin::SharedArrayBufferPrototype
            | Builtin::WeakRefPrototype
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
