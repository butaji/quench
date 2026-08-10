use crate::{ops::Builtin, value::Value};

/// Lookup a property on a builtin, checking intl first, then special, then callable.
pub(crate) fn lookup(builtin: Builtin, key: &str) -> Value {
    if let Some(value) = crate::intl::property(builtin, key) {
        return value;
    }
    if let Some(value) = special(builtin, key) {
        return value;
    }
    if let Some(value) = callable(builtin, key) {
        return value;
    }
    Value::Undefined
}

fn special(builtin: Builtin, key: &str) -> Option<Value> {
    use Builtin::*;
    if builtin == Math {
        return crate::math::property(key).map(Value::Builtin);
    }
    special_match(builtin, key)
}

fn special_match(builtin: Builtin, key: &str) -> Option<Value> {
    use Builtin::*;
    match (builtin, key) {
        (Symbol, "prototype") => Some(Value::Builtin(SymbolPrototype)),
        (Symbol, k) => crate::builtin_meta::symbol::symbol_prop(k).map(Value::Builtin),
        (MapPrototype | SetPrototype, k) => collections_prop(builtin, k).map(Value::Builtin),
        (DatePrototype, k) => crate::builtin_meta::date::date_prop(k).map(Value::Builtin),
        _ => builtin_method(builtin, key).map(Value::Builtin),
    }
}

fn builtin_method(builtin: Builtin, key: &str) -> Option<Builtin> {
    use Builtin::*;
    if builtin == ArrayPrototype {
        return array_method(key);
    }
    if builtin == StringPrototype {
        return crate::strings::property_method(key);
    }
    match (builtin, key) {
        (Array, "prototype") => Some(ArrayPrototype),
        (ArrayBuffer, "isView") => Some(ArrayBufferIsView),
        (Float64Array, "prototype") => Some(Float64ArrayPrototype),
        (Float64ArrayPrototype, "constructor") => Some(Float64Array),
        (Float32Array, "prototype") => Some(Float32ArrayPrototype),
        (Float32ArrayPrototype, "constructor") => Some(Float32Array),
        (DataView, "prototype") => Some(DataViewPrototype),
        (DataViewPrototype, "constructor") => Some(DataView),
        (Function, "prototype") => Some(FunctionPrototype),
        (FunctionPrototype, "call") => Some(FunctionCall),
        (FunctionPrototype, "bind") => Some(FunctionBind),
        (FunctionCall, "bind") => Some(FunctionBind),
        (Object, "prototype") => Some(ObjectPrototype),
        (ObjectPrototype, "hasOwnProperty") => Some(ObjectHasOwnProperty),
        (ObjectPrototype, "propertyIsEnumerable") => Some(ObjectPropertyIsEnumerable),
        (ObjectPrototype, "toString") => Some(ObjectPrototypeToString),
        (ObjectPrototype, "valueOf") => Some(ObjectPrototypeValueOf),
        (Date, "prototype") => Some(DatePrototype),
        (Date, "now") => Some(DateNow),
        (Date, "parse") => Some(DateParse),
        (Date, "UTC") => Some(DateUTC),
        (Promise, "prototype") => Some(PromisePrototype),
        (PromisePrototype, "constructor") => Some(Promise),
        (PromisePrototype, "then") => Some(PromiseThen),
        (PromisePrototype, "catch") => Some(PromiseCatch),
        (PromisePrototype, "finally") => Some(PromiseFinally),
        (Promise, "resolve") => Some(PromiseResolve),
        (Promise, "reject") => Some(PromiseReject),
        (Reflect, "construct") => Some(ReflectConstruct),
        (RegExp, "prototype") => Some(RegExpPrototype),
        (RegExpPrototype, "test") => Some(RegExpTest),
        (RegExpPrototype, "exec") => Some(RegExpExec),
        _ => builtin_method2(builtin, key),
    }
}

fn array_method(key: &str) -> Option<Builtin> {
    use Builtin::*;
    match key {
        "forEach" => Some(ArrayForEach),
        "map" => Some(ArrayMap),
        "filter" => Some(ArrayFilter),
        "some" => Some(ArraySome),
        "every" => Some(ArrayEvery),
        "find" => Some(ArrayFind),
        "includes" => Some(ArrayIncludes),
        "indexOf" => Some(ArrayIndexOf),
        "lastIndexOf" => Some(ArrayLastIndexOf),
        "slice" => Some(ArraySlice),
        "concat" => Some(ArrayConcat),
        "flat" => Some(ArrayFlat),
        "flatMap" => Some(ArrayFlatMap),
        "at" => Some(ArrayAt),
        "toReversed" => Some(ArrayToReversed),
        "join" => Some(ArrayJoin),
        "reduce" => Some(ArrayReduce),
        "reduceRight" => Some(ArrayReduceRight),
        "toLocaleString" => Some(ArrayToLocaleString),
        "push" => Some(ArrayPush),
        _ => None,
    }
}

fn builtin_method2(builtin: Builtin, key: &str) -> Option<Builtin> {
    use Builtin::*;
    match (builtin, key) {
        (Object, "defineProperty") => Some(ObjectDefineProperty),
        (Object, "getOwnPropertyDescriptor") => Some(ObjectGetOwnPropertyDescriptor),
        (Object, "getOwnPropertyNames") => Some(ObjectGetOwnPropertyNames),
        (Object, "create") => Some(ObjectCreate),
        (Object, "freeze") => Some(ObjectFreeze),
        (Object, "seal") => Some(ObjectSeal),
        (Object, "preventExtensions") => Some(ObjectPreventExtensions),
        (Object, "isFrozen") => Some(ObjectIsFrozen),
        (Object, "isSealed") => Some(ObjectIsSealed),
        (Object, "isExtensible") => Some(ObjectIsExtensible),
        (Object, "getPrototypeOf") => Some(ObjectGetPrototypeOf),
        (Object, "setPrototypeOf") => Some(ObjectSetPrototypeOf),
        (FunctionPrototype, "toString") => Some(FunctionPrototypeToString),
        (FunctionPrototype, "valueOf") => Some(FunctionPrototypeValueOf),
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
        (Boolean, "prototype") => Some(BooleanPrototype),
        #[allow(unreachable_patterns)]
        (Symbol, "prototype") => Some(SymbolPrototype),
        (String, "prototype") => Some(StringPrototype),
        (BigInt, "prototype") => Some(BigIntPrototype),
        _ => None,
    }
}

fn collections_prop(builtin: Builtin, key: &str) -> Option<Builtin> {
    crate::builtin_meta::collections::collections_property(builtin, key).and_then(|v| match v {
        Value::Builtin(b) => Some(b),
        _ => None,
    })
}

pub(crate) fn special_property(builtin: Builtin, key: &str) -> Option<Value> {
    special(builtin, key)
}

pub(crate) fn callable(builtin: Builtin, key: &str) -> Option<Value> {
    match key {
        "call" => Some(Value::Builtin(Builtin::FunctionCall)),
        "length" => Some(Value::Number(builtin_length(builtin))),
        "name" => Some(Value::String(builtin_name(builtin).to_string())),
        _ => None,
    }
}

fn builtin_length(builtin: Builtin) -> f64 {
    use Builtin::*;
    match builtin {
        Escape | Unescape | DateSetYear => 1.0,
        ArrayBuffer => 1.0,
        Float64Array => 3.0,
        Float32Array => 3.0,
        DataView => 3.0,
        DateNow => 0.0,
        RegExp => 2.0,
        DateParse => 1.0,
        DateUTC => 7.0,
        _ => 0.0,
    }
}

pub(crate) fn builtin_name(builtin: Builtin) -> &'static str {
    use Builtin::*;
    match builtin {
        Escape => "escape",
        Unescape => "unescape",
        Array => "Array",
        ArrayBuffer => "ArrayBuffer",
        ArrayBufferIsView => "isView",
        Float64Array => "Float64Array",
        Float32Array => "Float32Array",
        DataView => "DataView",
        Object => "Object",
        String => "String",
        Symbol => "Symbol",
        Number => "Number",
        Date => "Date",
        DateGetYear => "getYear",
        DateSetYear => "setYear",
        RegExp => "RegExp",
        RegExpTest => "test",
        RegExpExec => "exec",
        Error => "Error",
        TypeError => "TypeError",
        RangeError => "RangeError",
        ReferenceError => "ReferenceError",
        SyntaxError => "SyntaxError",
        EvalError => "EvalError",
        URIError => "URIError",
        AggregateError => "AggregateError",
        _ => "",
    }
}
