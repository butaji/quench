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
            | Builtin::BigInt64Array
            | Builtin::BigUint64Array
    )
}

fn typed_array_static_property(builtin: Builtin, key: &str) -> Option<Value> {
    if !is_typed_array_constructor(builtin) || !matches!(key, "from" | "of") {
        return None;
    }
    let target = if key == "from" {
        Builtin::TypedArrayFrom
    } else {
        Builtin::TypedArrayOf
    };
    Some(Value::BoundFunction(std::rc::Rc::new(
        crate::value::BoundFunctionValue {
            target: Value::Builtin(target),
            receiver: Value::Builtin(builtin),
            arguments: Vec::new(),
            properties: std::cell::RefCell::new(Vec::new()),
        },
    )))
}

fn typed_array_constructor_property(builtin: Builtin, key: &str) -> Option<Builtin> {
    use Builtin::*;
    if builtin == TypedArray && matches!(key, "from" | "of") {
        return Some(if key == "from" { TypedArrayFrom } else { TypedArrayOf });
    }
    if key == "from" && is_typed_array_constructor(builtin) {
        return Some(TypedArrayFrom);
    }
    if key == "of" && is_typed_array_constructor(builtin) {
        return Some(TypedArrayOf);
    }
    Some(match (builtin, key) {
        (Float64Array, "prototype") => Float64ArrayPrototype,
        (Float64ArrayPrototype, "constructor") => Float64Array,
        (Float32Array, "prototype") => Float32ArrayPrototype,
        (Float32ArrayPrototype, "constructor") => Float32Array,
        (Int8Array, "prototype") => Int8ArrayPrototype,
        (Int8ArrayPrototype, "constructor") => Int8Array,
        (Int16Array, "prototype") => Int16ArrayPrototype,
        (Int16ArrayPrototype, "constructor") => Int16Array,
        (Uint16Array, "prototype") => Uint16ArrayPrototype,
        (Uint16ArrayPrototype, "constructor") => Uint16Array,
        (Int32Array, "prototype") => Int32ArrayPrototype,
        (Int32ArrayPrototype, "constructor") => Int32Array,
        (Uint8Array, "prototype") => Uint8ArrayPrototype,
        (Uint8ArrayPrototype, "constructor") => Uint8Array,
        (Uint32Array, "prototype") => Uint32ArrayPrototype,
        (Uint32ArrayPrototype, "constructor") => Uint32Array,
        (Uint8ClampedArray, "prototype") => Uint8ClampedArrayPrototype,
        (Uint8ClampedArrayPrototype, "constructor") => Uint8ClampedArray,
        (BigInt64Array, "prototype") => BigInt64ArrayPrototype,
        (BigInt64ArrayPrototype, "constructor") => BigInt64Array,
        (BigUint64Array, "prototype") => BigUint64ArrayPrototype,
        (BigUint64ArrayPrototype, "constructor") => BigUint64Array,
        _ => return None,
    })
}
