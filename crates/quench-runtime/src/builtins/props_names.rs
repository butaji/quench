fn typed_array_name(builtin: Builtin) -> Option<&'static str> {
    use Builtin::*;
    Some(match builtin {
        TypedArray => "TypedArray", Float64Array => "Float64Array", Float32Array => "Float32Array",
        Int8Array => "Int8Array", Int16Array => "Int16Array", Uint16Array => "Uint16Array",
        Int32Array => "Int32Array", Uint8Array => "Uint8Array", Uint32Array => "Uint32Array",
        Uint8ClampedArray => "Uint8ClampedArray", BigInt64Array => "BigInt64Array",
        BigUint64Array => "BigUint64Array", WeakMap => "WeakMap", _ => return None,
    })
}

fn generator_name(builtin: Builtin) -> Option<&'static str> {
    Some(match builtin {
        Builtin::GeneratorNext => "next", Builtin::GeneratorReturn => "return",
        Builtin::GeneratorThrow => "throw", _ => return None,
    })
}

fn error_name(builtin: Builtin) -> Option<&'static str> {
    use Builtin::*;
    Some(match builtin {
        Error => "Error", TypeError | TypeErrorPrototype => "TypeError", RangeError | RangeErrorPrototype => "RangeError",
        ReferenceError | ReferenceErrorPrototype => "ReferenceError", SyntaxError | SyntaxErrorPrototype => "SyntaxError", EvalError | EvalErrorPrototype => "EvalError",
        URIError | URIErrorPrototype => "URIError", AggregateError | AggregateErrorPrototype => "AggregateError", SuppressedError => "SuppressedError", _ => return None,
    })
}
fn object_prototype_method(key: &str) -> Option<crate::ops::Builtin> {
    use crate::ops::Builtin::*;
    Some(match key {
        "hasOwnProperty" => ObjectHasOwnProperty,
        "isPrototypeOf" => ObjectPrototypeIsPrototypeOf,
        "__defineGetter__" => ObjectPrototypeDefineGetter,
        "__defineSetter__" => ObjectPrototypeDefineSetter,
        "__lookupGetter__" => ObjectPrototypeLookupGetter,
        "__lookupSetter__" => ObjectPrototypeLookupSetter,
        "propertyIsEnumerable" => ObjectPropertyIsEnumerable,
        "toString" => ObjectPrototypeToString,
        "valueOf" => ObjectPrototypeValueOf,
        _ => return None,
    })
}
fn data_view_length(builtin: crate::ops::Builtin) -> Option<f64> {
    use crate::ops::Builtin::*;
    match builtin {
        DataViewGetInt8 | DataViewGetUint8 | DataViewGetInt16 | DataViewGetUint16
        | DataViewGetInt32 | DataViewGetUint32 | DataViewGetFloat16 | DataViewGetFloat32
        | DataViewGetFloat64 | DataViewGetBigInt64 | DataViewGetBigUint64 => Some(1.0),
        DataViewSetInt8 | DataViewSetUint8 | DataViewSetInt16 | DataViewSetUint16
        | DataViewSetInt32 | DataViewSetUint32 | DataViewSetFloat16 | DataViewSetFloat32
        | DataViewSetFloat64 | DataViewSetBigInt64 | DataViewSetBigUint64 => Some(2.0),
        _ => None,
    }
}
