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
        Error => "Error", TypeError => "TypeError", RangeError => "RangeError",
        ReferenceError => "ReferenceError", SyntaxError => "SyntaxError", EvalError => "EvalError",
        URIError => "URIError", AggregateError => "AggregateError", _ => return None,
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
