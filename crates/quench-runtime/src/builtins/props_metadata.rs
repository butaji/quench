pub(crate) fn number_constant(key: &str) -> Option<Value> {
    props_number::constant(key)
}

pub(crate) fn is_builtin_deletable(_builtin: Builtin, key: &str) -> bool {
    if key == "BYTES_PER_ELEMENT"
        && crate::builtin_meta::prototype(_builtin) == Some(Builtin::TypedArrayPrototype)
    {
        return false;
    }
    if _builtin == Builtin::ArrayPrototype && key == "length" {
        return false;
    }
    if _builtin == Builtin::FunctionPrototype && key == "Symbol.hasInstance" {
        return false;
    }
    if _builtin == Builtin::Number && props_number::constant(key).is_some() {
        return false;
    }
    if matches!(
        (_builtin, key),
        (Builtin::ThrowTypeError, "length" | "name")
    ) {
        return false;
    }
    if matches!(
        (_builtin, key),
        (Builtin::GeneratorFunctionPrototype, "prototype")
            | (Builtin::AsyncGeneratorFunctionPrototype, "prototype")
    ) {
        return true;
    }
    if matches!(
        (_builtin, key),
        (Builtin::Math, "E" | "LN2" | "LN10" | "LOG2E" | "LOG10E" | "PI" | "SQRT1_2" | "SQRT2")
    ) {
        return false;
    }
    if key == "prototype" {
        return false;
    }
    if crate::builtins::object::is_well_known_symbol_property(_builtin, key)
        && key != "Symbol.toStringTag"
    {
        return false;
    }
    if key == "Symbol.toStringTag"
        && crate::builtin_meta::instance_prototype(_builtin).is_none()
        && crate::builtin_meta::prototype(_builtin).is_some()
    {
        return true;
    }
    true
}

fn builtin_length(builtin: Builtin) -> f64 {
    use Builtin::*;
    if let Some(length) = data_view_length(builtin) {
        return length;
    }
    if let Some(length) = crate::builtin_meta::constructor_length(builtin) {
        return length;
    }
    if let Some(length) = crate::builtin_meta::methods::function_length(builtin) {
        return length;
    }
    match builtin {
        Escape | Unescape | EncodeURI | EncodeURIComponent | DecodeURI | DecodeURIComponent
        | DateSetYear | GeneratorNext | GeneratorReturn | GeneratorThrow | AsyncGeneratorNext
        | AsyncGeneratorReturn | AsyncGeneratorThrow => 1.0,
        ArrayBuffer | Object | DataView => 1.0,
        ObjectCreate => 2.0,
        Float64Array | Float32Array | Int8Array | Int16Array | Uint16Array | Int32Array
        | Uint32Array | Uint8ClampedArray | BigInt64Array | BigUint64Array => 3.0,
        MapEntries | MapKeys | MapValues | DateNow => 0.0,
        RegExp => 2.0,
        DateParse => 1.0,
        DateUTC => 7.0,
        _ => 0.0,
    }
}
