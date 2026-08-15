fn builtin_method2(builtin: Builtin, key: &str) -> Option<Builtin> {
    use Builtin::*;
    match (builtin, key) {
        (Object, "defineProperty") => Some(ObjectDefineProperty),
        (Object, "defineProperties") => Some(ObjectDefineProperties),
        (Object, "getOwnPropertyDescriptor") => Some(ObjectGetOwnPropertyDescriptor),
        (Object, "keys") => Some(ObjectKeys),
        (Object, "values") => Some(ObjectValues),
        (Object, "entries") => Some(ObjectEntries),
        (Object, "hasOwn") => Some(ObjectHasOwn),
        (Object, "getOwnPropertyNames") => Some(ObjectGetOwnPropertyNames),
        (Object, "getOwnPropertySymbols") => Some(ObjectGetOwnPropertySymbols),
        (Object, "create") => Some(ObjectCreate),
        (Object, "freeze") => Some(ObjectFreeze),
        (Object, "seal") => Some(ObjectSeal),
        (Object, "preventExtensions") => Some(ObjectPreventExtensions),
        (Object, "isFrozen") => Some(ObjectIsFrozen),
        (Object, "isSealed") => Some(ObjectIsSealed),
        (Object, "isExtensible") => Some(ObjectIsExtensible),
        (Object, "getPrototypeOf") => Some(ObjectGetPrototypeOf),
        (Object, "is") => Some(ObjectIs),
        (Object, "assign") => Some(ObjectAssign),
        (Object, "setPrototypeOf") => Some(ObjectSetPrototypeOf),
        (Map, "prototype") => Some(MapPrototype),
        (Set, "prototype") => Some(SetPrototype),
        (FunctionPrototype, "toString") => Some(FunctionPrototypeToString),
        (FunctionPrototype, "valueOf") => Some(FunctionPrototypeValueOf),
        (RegExpPrototype, "toString") => Some(RegExpPrototypeToString),
        _ => builtin_method3(builtin, key),
    }
}
fn builtin_method3(builtin: Builtin, key: &str) -> Option<Builtin> {
    use Builtin::*;
    match (builtin, key) {
        (Number, "prototype") => Some(NumberPrototype),
        (NumberPrototype, "toLocaleString") => Some(NumberToLocaleString),
        (NumberPrototype, "toString") => Some(NumberToString),
        (NumberPrototype, "valueOf") => Some(NumberValueOf),
        (NumberPrototype, "toFixed") => Some(NumberToFixed),
        (NumberPrototype, "toPrecision") => Some(NumberToPrecision),
        (NumberPrototype, "toExponential") => Some(NumberToExponential),
        (Number, "isNaN") => Some(IsNaN),
        (Number, "isFinite") => Some(IsFinite),
        (Number, key @ ("isInteger" | "isSafeInteger")) => Some(if key == "isInteger" {
            NumberIsInteger
        } else {
            NumberIsSafeInteger
        }),
        (Boolean, "prototype") => Some(BooleanPrototype),
        (BooleanPrototype, "valueOf") => Some(BooleanValueOf),
        (BooleanPrototype, "toString") => Some(BooleanToString),
        (BooleanPrototype, "constructor") => Some(Boolean),
        (ObjectPrototype, "constructor") => Some(Object),
        (ObjectPrototype, "toLocaleString") => Some(ObjectPrototypeToString),
        (Symbol, "prototype") => Some(SymbolPrototype),
        (SymbolPrototype, "toString") => Some(SymbolToString),
        (SymbolPrototype, "valueOf") => Some(SymbolValueOf),
        (SymbolPrototype, "Symbol.toPrimitive") => Some(SymbolPrototypeToPrimitive),
        (SymbolPrototype, "constructor") => Some(Symbol),
        (String, "prototype") => Some(StringPrototype),
        (StringPrototype, "valueOf") => Some(StringValueOf),
        (BigInt, "prototype") => Some(BigIntPrototype),
        (BigInt, "asIntN") => Some(BigIntAsIntN),
        (BigInt, "asUintN") => Some(BigIntAsUintN),
        (BigIntPrototype, "valueOf") => Some(BigIntValueOf),
        (BigIntPrototype, "constructor") => Some(BigInt),
        (BigIntPrototype, "toString" | "toLocaleString") => Some(BigIntToString),
        _ => None,
    }
}
pub(crate) fn special_property(builtin: Builtin, key: &str) -> Option<Value> {
    special(builtin, key)
}
pub(crate) fn callable(builtin: Builtin, key: &str) -> Option<Value> {
    match key {
        "call" => Some(Value::Builtin(Builtin::FunctionCall)),
        "bind" => Some(Value::Builtin(Builtin::FunctionBind)),
        "length" => Some(Value::Number(builtin_length(builtin))),
        "name" => Some(Value::String(builtin_name(builtin).to_string())),
        _ => None,
    }
}
pub(crate) fn is_builtin_deletable(_builtin: Builtin, key: &str) -> bool {
    if key == "prototype" {
        return false;
    }
    if crate::builtins::object::is_well_known_symbol_property(_builtin, key) {
        return false;
    }
    if matches!(
        (_builtin, key),
        (
            Builtin::Math,
            "E" | "LN2" | "LN10" | "LOG2E" | "LOG10E" | "PI" | "SQRT1_2" | "SQRT2"
        )
    ) {
        return false;
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
        | DateSetYear | GeneratorNext | GeneratorReturn | GeneratorThrow => 1.0,
        ArrayBuffer => 1.0,
        Object => 1.0,
        Float64Array => 3.0,
        Float32Array => 3.0,
        Int8Array => 3.0,
        Int16Array => 3.0,
        Uint16Array => 3.0,
        Int32Array => 3.0,
        Uint8Array => 3.0,
        Uint32Array => 3.0,
        Uint8ClampedArray => 3.0,
        BigInt64Array | BigUint64Array => 3.0,
        MapEntries | MapKeys | MapValues => 0.0,
        DataView => 1.0,
        DateNow => 0.0,
        RegExp => 2.0,
        DateParse => 1.0,
        DateUTC => 7.0,
        _ => 0.0,
    }
}
include!("props_builtin_names.rs");
