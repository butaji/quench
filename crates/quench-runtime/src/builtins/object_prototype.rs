pub(crate) fn get_prototype_of(value: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    let value = require_object_coercible(value)?;
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
    let prototype = receiver
        .filter(|value| crate::value::is_object(value))
        .ok_or_else(|| {
            crate::value::error::throw_type_error(
                "Object.prototype.isPrototypeOf called on null or undefined",
            )
        })?;
    let Some(value) = arguments
        .first()
        .filter(|value| crate::value::is_object(value))
    else {
        return Ok(Value::Boolean(false));
    };
    let mut current = get_prototype_of(Some(value))?;
    while !matches!(current, Value::Null) {
        if crate::builtins::same_value(Some(&current), Some(prototype)) {
            return Ok(Value::Boolean(true));
        }
        current = get_prototype_of(Some(&current))?;
    }
    Ok(Value::Boolean(false))
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
