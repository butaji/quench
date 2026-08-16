pub(crate) fn to_object(value: &Value) -> Result<Value, crate::execute::VmError> {
    match value {
        Value::Object(_)
        | Value::Array(_)
        | Value::ObjectAlias(_)
        | Value::Function(_)
        | Value::BoundFunction(_)
        | Value::Builtin(_)
        | Value::Proxy(_)
        | Value::Promise(_)
        | Value::Map(_)
        | Value::Set(_)
        | Value::ArrayBuffer(_)
        | Value::DataView(_)
        | Value::Float32Array(_)
        | Value::Float64Array(_)
        | Value::Int8Array(_)
        | Value::Int16Array(_)
        | Value::Int32Array(_)
        | Value::Uint8Array(_)
        | Value::Uint8ClampedArray(_)
        | Value::Uint16Array(_)
        | Value::Uint32Array(_)
        | Value::BigInt64Array(_)
        | Value::BigUint64Array(_)
        | Value::Iterator(_)
        | Value::Generator(_)
        | Value::HostCapability(_) => Ok(value.clone()),
        Value::Number(value) => Ok(boxed_primitive(Value::Number(*value), crate::ops::Builtin::Number)),
        Value::Boolean(value) => Ok(boxed_primitive(Value::Boolean(*value), crate::ops::Builtin::Boolean)),
        Value::String(value) if crate::conversion::is_symbol_string(value) => {
            Ok(boxed_primitive(Value::String(value.clone()), crate::ops::Builtin::Symbol))
        }
        Value::String(value) => Ok(boxed_primitive(Value::String(value.clone()), crate::ops::Builtin::String)),
        Value::StringUnits(value) => Ok(boxed_primitive(Value::StringUnits(value.clone()), crate::ops::Builtin::String)),
        Value::BigInt(value) => Ok(boxed_primitive(Value::BigInt(value.clone()), crate::ops::Builtin::BigInt)),
        Value::BindingCell(_) | Value::Undefined | Value::Null => Err(
            crate::value::error::throw_type_error("Cannot convert undefined or null to object"),
        ),
    }
}
