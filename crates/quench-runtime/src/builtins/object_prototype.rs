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
