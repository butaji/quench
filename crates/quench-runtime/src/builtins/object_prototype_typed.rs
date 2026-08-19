fn typed_array_slot_prototype(value: &Value) -> Option<Value> {
    Some(match value {
        Value::Float64Array(view) => view
            .prototype()
            .unwrap_or(Value::Builtin(Builtin::Float64ArrayPrototype)),
        Value::Float32Array(view) => view
            .prototype()
            .unwrap_or(Value::Builtin(Builtin::Float32ArrayPrototype)),
        Value::Int8Array(view) => view
            .prototype()
            .unwrap_or(Value::Builtin(Builtin::Int8ArrayPrototype)),
        Value::Int16Array(view) => view
            .prototype()
            .unwrap_or(Value::Builtin(Builtin::Int16ArrayPrototype)),
        Value::Int32Array(view) => view
            .prototype()
            .unwrap_or(Value::Builtin(Builtin::Int32ArrayPrototype)),
        Value::Uint8Array(view) => view
            .prototype()
            .unwrap_or(Value::Builtin(Builtin::Uint8ArrayPrototype)),
        Value::Uint16Array(view) => view
            .prototype()
            .unwrap_or(Value::Builtin(Builtin::Uint16ArrayPrototype)),
        Value::Uint32Array(view) => view
            .prototype()
            .unwrap_or(Value::Builtin(Builtin::Uint32ArrayPrototype)),
        Value::Uint8ClampedArray(view) => view
            .prototype()
            .unwrap_or(Value::Builtin(Builtin::Uint8ClampedArrayPrototype)),
        Value::BigInt64Array(view) => view
            .prototype()
            .unwrap_or(Value::Builtin(Builtin::BigInt64ArrayPrototype)),
        Value::BigUint64Array(view) => view
            .prototype()
            .unwrap_or(Value::Builtin(Builtin::BigUint64ArrayPrototype)),
        _ => return None,
    })
}
