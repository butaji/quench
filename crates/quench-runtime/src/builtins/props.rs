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
    if builtin == Json && key == "stringify" {
        return Some(Value::Builtin(JsonStringify));
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
    if builtin == Array && key == "isArray" {
        return Some(ArrayIsArray);
    }
    if builtin == Array && key == "from" {
        return Some(ArrayFrom);
    }
    if builtin == StringPrototype {
        return crate::strings::property_method(key);
    }
    if builtin == String && key == "fromCharCode" {
        return Some(StringFromCharCode);
    }
    if builtin == DataViewPrototype {
        return data_view_method(key);
    }
    builtin_method_core(builtin, key)
}

fn builtin_method_core(builtin: Builtin, key: &str) -> Option<Builtin> {
    use Builtin::*;
    if let HostCapability(kind) = builtin {
        return host_capability_method(kind, key);
    }
    if let Some(method) = specialized_method(builtin, key) {
        return Some(method);
    }
    match (builtin, key) {
        (Array, "prototype") => Some(ArrayPrototype),
        (ArrayBuffer, "isView") => Some(ArrayBufferIsView),
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
        (RegExp, "prototype") => Some(RegExpPrototype),
        (RegExpPrototype, "test") => Some(RegExpTest),
        (RegExpPrototype, "exec") => Some(RegExpExec),
        _ => builtin_method2(builtin, key),
    }
}

fn specialized_method(builtin: Builtin, key: &str) -> Option<Builtin> {
    typed_array_property(builtin, key).or_else(|| {
        (builtin == Builtin::Reflect)
            .then(|| reflect_method(key))
            .flatten()
    })
}

fn reflect_method(key: &str) -> Option<Builtin> {
    use Builtin::*;
    Some(match key {
        "construct" => ReflectConstruct,
        "get" => ReflectGet,
        "set" => ReflectSet,
        "has" => ReflectHas,
        "deleteProperty" => ReflectDeleteProperty,
        "getPrototypeOf" => ReflectGetPrototypeOf,
        "setPrototypeOf" => ReflectSetPrototypeOf,
        "isExtensible" => ReflectIsExtensible,
        "preventExtensions" => ReflectPreventExtensions,
        "getOwnPropertyDescriptor" => ReflectGetOwnPropertyDescriptor,
        "defineProperty" => ReflectDefineProperty,
        "ownKeys" => ReflectOwnKeys,
        "apply" => ReflectApply,
        _ => return None,
    })
}

fn typed_array_property(builtin: Builtin, key: &str) -> Option<Builtin> {
    typed_array_constructor_property(builtin, key).or_else(|| {
        (is_typed_array_prototype(builtin) && key == "fill").then_some(Builtin::TypedArrayFill)
    })
}

fn is_typed_array_prototype(builtin: Builtin) -> bool {
    use Builtin::*;
    matches!(
        builtin,
        Float64ArrayPrototype
            | Float32ArrayPrototype
            | Int8ArrayPrototype
            | Int16ArrayPrototype
            | Int32ArrayPrototype
            | Uint8ArrayPrototype
            | Uint16ArrayPrototype
            | Uint32ArrayPrototype
            | Uint8ClampedArrayPrototype
    )
}

fn typed_array_constructor_property(builtin: Builtin, key: &str) -> Option<Builtin> {
    use Builtin::*;
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
        _ => return None,
    })
}

fn host_capability_method(_kind: crate::ops::HostCapabilityKind, key: &str) -> Option<Builtin> {
    use crate::ops::HostCapabilityKind::*;
    let kind = match key {
        "global" => GetGlobal,
        "createRealm" => CreateRealm,
        "evalScript" => EvalScript,
        "detachArrayBuffer" => DetachArrayBuffer,
        _ => return None,
    };
    Some(Builtin::HostCapability(kind))
}

fn data_view_method(key: &str) -> Option<Builtin> {
    use Builtin::*;
    match key {
        "getInt8" => Some(DataViewGetInt8),
        "getUint8" => Some(DataViewGetUint8),
        "getInt16" => Some(DataViewGetInt16),
        "getUint16" => Some(DataViewGetUint16),
        "getInt32" => Some(DataViewGetInt32),
        "getUint32" => Some(DataViewGetUint32),
        "getFloat16" => Some(DataViewGetFloat16),
        "getFloat32" => Some(DataViewGetFloat32),
        "getFloat64" => Some(DataViewGetFloat64),
        "setInt8" => Some(DataViewSetInt8),
        "setUint8" => Some(DataViewSetUint8),
        "setInt16" => Some(DataViewSetInt16),
        "setUint16" => Some(DataViewSetUint16),
        "setInt32" => Some(DataViewSetInt32),
        "setUint32" => Some(DataViewSetUint32),
        "setFloat16" => Some(DataViewSetFloat16),
        "setFloat32" => Some(DataViewSetFloat32),
        "setFloat64" => Some(DataViewSetFloat64),
        _ => None,
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
        "splice" => Some(ArraySplice),
        _ => None,
    }
}

fn builtin_method2(builtin: Builtin, key: &str) -> Option<Builtin> {
    use Builtin::*;
    match (builtin, key) {
        (Object, "defineProperty") => Some(ObjectDefineProperty),
        (Object, "getOwnPropertyDescriptor") => Some(ObjectGetOwnPropertyDescriptor),
        (Object, "keys") => Some(ObjectKeys),
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
        (NumberPrototype, "valueOf") => Some(BoxedValueOf),
        (NumberPrototype, "toFixed") => Some(NumberToFixed),
        (NumberPrototype, "toPrecision") => Some(NumberToPrecision),
        (NumberPrototype, "toExponential") => Some(NumberToExponential),
        (Number, "isNaN") => Some(IsNaN),
        (Number, "isFinite") => Some(IsFinite),
        (Boolean, "prototype") => Some(BooleanPrototype),
        (BooleanPrototype, "valueOf") => Some(BoxedValueOf),
        #[allow(unreachable_patterns)]
        (Symbol, "prototype") => Some(SymbolPrototype),
        (SymbolPrototype, "valueOf") => Some(BoxedValueOf),
        (String, "prototype") => Some(StringPrototype),
        (StringPrototype, "valueOf") => Some(BoxedValueOf),
        (BigInt, "prototype") => Some(BigIntPrototype),
        (BigIntPrototype, "valueOf") => Some(BoxedValueOf),
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
        "bind" => Some(Value::Builtin(Builtin::FunctionBind)),
        "length" => Some(Value::Number(builtin_length(builtin))),
        "name" => Some(Value::String(builtin_name(builtin).to_string())),
        _ => None,
    }
}

fn builtin_length(builtin: Builtin) -> f64 {
    use Builtin::*;
    if let Some(length) = data_view_length(builtin) {
        return length;
    }
    match builtin {
        Escape | Unescape | DateSetYear => 1.0,
        ArrayBuffer => 1.0,
        Float64Array => 3.0,
        Float32Array => 3.0,
        Int8Array => 3.0,
        Int16Array => 3.0,
        Uint16Array => 3.0,
        Int32Array => 3.0,
        Uint8Array => 3.0,
        Uint32Array => 3.0,
        Uint8ClampedArray => 3.0,
        DataView => 3.0,
        DateNow => 0.0,
        RegExp => 2.0,
        DateParse => 1.0,
        DateUTC => 7.0,
        _ => 0.0,
    }
}

fn data_view_length(builtin: Builtin) -> Option<f64> {
    use Builtin::*;
    match builtin {
        DataViewGetInt8 | DataViewGetUint8 => Some(1.0),
        DataViewGetInt16 | DataViewGetUint16 | DataViewGetInt32 | DataViewGetUint32
        | DataViewGetFloat16 | DataViewGetFloat32 | DataViewGetFloat64 => Some(2.0),
        DataViewSetInt8 | DataViewSetUint8 => Some(2.0),
        DataViewSetInt16 | DataViewSetUint16 | DataViewSetInt32 | DataViewSetUint32
        | DataViewSetFloat16 | DataViewSetFloat32 | DataViewSetFloat64 => Some(3.0),
        _ => None,
    }
}

pub(crate) fn builtin_name(builtin: Builtin) -> &'static str {
    use Builtin::*;
    if let Some(name) = data_view_name(builtin) {
        return name;
    }
    if let Some(name) = error_name(builtin) {
        return name;
    }
    match builtin {
        Escape => "escape",
        Unescape => "unescape",
        Array => "Array",
        TypedArray => "TypedArray",
        ArrayBuffer => "ArrayBuffer",
        ArrayBufferIsView => "isView",
        Float64Array => "Float64Array",
        Float32Array => "Float32Array",
        Int8Array => "Int8Array",
        Int16Array => "Int16Array",
        Uint16Array => "Uint16Array",
        Int32Array => "Int32Array",
        Uint8Array => "Uint8Array",
        Uint32Array => "Uint32Array",
        Uint8ClampedArray => "Uint8ClampedArray",
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
        _ => "",
    }
}

fn error_name(builtin: Builtin) -> Option<&'static str> {
    use Builtin::*;
    Some(match builtin {
        Error => "Error",
        TypeError => "TypeError",
        RangeError => "RangeError",
        ReferenceError => "ReferenceError",
        SyntaxError => "SyntaxError",
        EvalError => "EvalError",
        URIError => "URIError",
        AggregateError => "AggregateError",
        _ => return None,
    })
}

fn data_view_name(builtin: Builtin) -> Option<&'static str> {
    use Builtin::*;
    Some(match builtin {
        DataView => "DataView",
        DataViewGetInt8 => "getInt8",
        DataViewGetUint8 => "getUint8",
        DataViewGetInt16 => "getInt16",
        DataViewGetUint16 => "getUint16",
        DataViewGetInt32 => "getInt32",
        DataViewGetUint32 => "getUint32",
        DataViewGetFloat16 => "getFloat16",
        DataViewGetFloat32 => "getFloat32",
        DataViewGetFloat64 => "getFloat64",
        DataViewSetInt8 => "setInt8",
        DataViewSetUint8 => "setUint8",
        DataViewSetInt16 => "setInt16",
        DataViewSetUint16 => "setUint16",
        DataViewSetInt32 => "setInt32",
        DataViewSetUint32 => "setUint32",
        DataViewSetFloat16 => "setFloat16",
        DataViewSetFloat32 => "setFloat32",
        DataViewSetFloat64 => "setFloat64",
        _ => return None,
    })
}
