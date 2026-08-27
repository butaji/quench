use crate::{ops::Builtin, value::Value};
include!("props_modules.rs");
include!("props_own_names.rs");
include!("props_collections.rs");
pub(crate) fn lookup(builtin: Builtin, key: &str) -> Value {
    if crate::builtins::builtin_prototype_property_is_removed(builtin, key) {
        return Value::Undefined;
    }
    if let Some(value) = crate::builtins::read_descriptor_value(builtin, key) {
        return value;
    }
    if let Some(value) = crate::intl::property(builtin, key) {
        return value;
    }
    if let Some(value) = iterator_property(builtin, key) {
        return value;
    }
    if let Some(value) =
        crate::builtin_meta::collections::iterator_prototype_property_callable(builtin, key)
    {
        return value;
    }
    if let Some(value) = typed_array_prototype_tag(builtin, key) {
        return value;
    }
    if builtin == Builtin::StringPrototype && key == "length" {
        return Value::Number(0.0);
    }
    if let Some(value) = special(builtin, key) {
        return value;
    }
    if let Some(value) = callable(builtin, key) {
        return value;
    }
    Value::Undefined
}
include!("props_special_core.rs");
fn iterator_property(builtin: Builtin, key: &str) -> Option<Value> {
    if builtin != Builtin::IteratorPrototype {
        return None;
    }
    match key {
        "Symbol.iterator" => Some(Value::Builtin(Builtin::IteratorSelf)),
        "Symbol.toStringTag" => Some(Value::String("Iterator".into())),
        "constructor" => Some(Value::Builtin(Builtin::Iterator)),
        "Symbol.dispose" => Some(Value::Builtin(Builtin::IteratorDispose)),
        "filter" => Some(Value::Builtin(Builtin::IteratorFilter)),
        _ => None,
    }
}
fn typed_array_prototype_tag(builtin: Builtin, key: &str) -> Option<Value> {
    let name = match builtin {
        Builtin::Float64ArrayPrototype => "Float64Array",
        Builtin::Float32ArrayPrototype => "Float32Array",
        Builtin::Int8ArrayPrototype => "Int8Array",
        Builtin::Int16ArrayPrototype => "Int16Array",
        Builtin::Int32ArrayPrototype => "Int32Array",
        Builtin::Uint8ArrayPrototype => "Uint8Array",
        Builtin::Uint8ClampedArrayPrototype => "Uint8ClampedArray",
        Builtin::Uint16ArrayPrototype => "Uint16Array",
        Builtin::Uint32ArrayPrototype => "Uint32Array",
        Builtin::BigInt64ArrayPrototype => "BigInt64Array",
        Builtin::BigUint64ArrayPrototype => "BigUint64Array",
        _ => return None,
    };
    if key == "Symbol.toStringTag" {
        Some(Value::String(name.to_string()))
    } else {
        None
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
    if builtin == Array && key == "fromAsync" {
        return Some(ArrayFromAsync);
    }
    if builtin == Array && key == "of" {
        return Some(ArrayOf);
    }
    if builtin == ArrayBuffer && key == "prototype" {
        return Some(ArrayBufferPrototype);
    }
    if builtin == Iterator && key == "prototype" {
        return Some(IteratorPrototype);
    }
    if builtin == Iterator && key == "concat" {
        return Some(IteratorConcat);
    }
    if builtin == Iterator && key == "from" {
        return Some(IteratorFrom);
    }
    if builtin == Iterator && key == "zip" {
        return Some(IteratorZip);
    }
    if builtin == Iterator && key == "zipKeyed" {
        return Some(IteratorZipKeyed);
    }
    if builtin == SharedArrayBuffer && key == "prototype" {
        return Some(SharedArrayBufferPrototype);
    }
    if builtin == Proxy && key == "revocable" {
        return Some(ProxyRevocable);
    }
    if builtin == StringPrototype {
        return crate::strings::property_method(key);
    }
    if builtin == String {
        return string_static_method(key).or_else(|| builtin_method_core(builtin, key));
    }
    if builtin == Number && key == "parseFloat" {
        return Some(ParseFloat);
    }
    if builtin == Number && key == "parseInt" {
        return Some(ParseInt);
    }
    if let Some(method) = data_view_method_for(builtin, key) {
        return Some(method);
    }
    builtin_method_core(builtin, key)
}
fn data_view_method_for(builtin: Builtin, key: &str) -> Option<Builtin> {
    (builtin == Builtin::DataViewPrototype)
        .then(|| data_view_method(key))
        .flatten()
}
fn string_static_method(key: &str) -> Option<Builtin> {
    Some(match key {
        "fromCharCode" => Builtin::StringFromCharCode,
        "fromCodePoint" => Builtin::StringFromCodePoint,
        "raw" => Builtin::StringRaw,
        _ => return None,
    })
}
fn builtin_method_core(builtin: Builtin, key: &str) -> Option<Builtin> {
    if let Some(method) = builtin_method_special(builtin, key) {
        return Some(method);
    }
    use Builtin::*;
    match (builtin, key) {
        (Array, "prototype") => Some(ArrayPrototype),
        (ArrayBuffer, "isView") => Some(ArrayBufferIsView),
        (ArrayBufferPrototype, "constructor") => Some(ArrayBuffer),
        (ArrayBufferPrototype, "byteLength") => Some(ArrayBufferByteLengthGetter),
        (ArrayBufferPrototype, "detached") => Some(ArrayBufferDetachedGetter),
        (ArrayBufferPrototype, "immutable") => Some(ArrayBufferImmutableGetter),
        (ArrayBufferPrototype, "maxByteLength") => Some(ArrayBufferMaxByteLengthGetter),
        (ArrayBufferPrototype, "resizable") => Some(ArrayBufferResizableGetter),
        (ArrayBufferPrototype, "slice") => Some(ArrayBufferSlice),
        (ArrayBufferPrototype, "resize") => Some(ArrayBufferResize),
        (ArrayBufferPrototype, "transferToImmutable") => Some(ArrayBufferTransferToImmutable),
        (ArrayBufferPrototype, "transfer") => Some(ArrayBufferTransfer),
        (ArrayBufferPrototype, "transferToFixedLength") => Some(ArrayBufferTransferToFixedLength),
        (ArrayBufferPrototype, "sliceToImmutable") => Some(ArrayBufferSliceToImmutable),
        (SharedArrayBufferPrototype, "constructor") => Some(SharedArrayBuffer),
        (SharedArrayBufferPrototype, "byteLength") => Some(SharedArrayBufferByteLengthGetter),
        (SharedArrayBufferPrototype, "growable") => Some(SharedArrayBufferGrowableGetter),
        (SharedArrayBufferPrototype, "maxByteLength") => Some(SharedArrayBufferMaxByteLengthGetter),
        (SharedArrayBufferPrototype, "grow") => Some(SharedArrayBufferGrow),
        (SharedArrayBufferPrototype, "slice") => Some(SharedArrayBufferSlice),
        (DataView, "prototype") => Some(DataViewPrototype),
        (DataViewPrototype, "constructor") => Some(DataView),
        (Function, "prototype") => Some(FunctionPrototype),
        (AsyncFunction, "prototype") => Some(AsyncFunctionPrototype),
        (GeneratorFunction, "prototype") => Some(GeneratorFunctionPrototype),
        (AsyncGeneratorFunction, "prototype") => Some(AsyncGeneratorFunctionPrototype),
        (FunctionPrototype, "constructor") => Some(Function),
        (GeneratorFunctionPrototype, "constructor") => Some(GeneratorFunction),
        (AsyncGeneratorFunctionPrototype, "constructor") => Some(AsyncGeneratorFunction),
        (AsyncFunctionPrototype, "constructor") => Some(AsyncFunction),
        (FunctionPrototype, "apply") => Some(FunctionApply),
        (FunctionPrototype, "call") => Some(FunctionCall),
        (FunctionPrototype, "bind") => Some(FunctionBind),
        (FunctionCall, "bind") => Some(FunctionBind),
        (Object, "prototype") => Some(ObjectPrototype),
        (Object, "fromEntries") => Some(ObjectFromEntries),
        (Object, "groupBy") => Some(ObjectGroupBy),
        (Date, "prototype") => Some(DatePrototype),
        (Date, "now") => Some(DateNow),
        (Date, "parse") => Some(DateParse),
        _ => builtin_method_core_tail(builtin, key),
    }
}

fn builtin_method_special(builtin: Builtin, key: &str) -> Option<Builtin> {
    use Builtin::*;
    if let Some(method) = builtin_method_prefix(builtin, key) {
        return Some(method);
    }
    if let Some(method) = promise_builtin_method(builtin, key) {
        return Some(method);
    }
    if builtin == ObjectPrototype {
        return object_prototype_method(key);
    }
    regexp_method(builtin, key)
}

fn builtin_method_core_tail(builtin: Builtin, key: &str) -> Option<Builtin> {
    use Builtin::*;
    match (builtin, key) {
        (Date, "UTC") => Some(DateUTC),
        _ => builtin_method2(builtin, key),
    }
}
fn regexp_method(builtin: Builtin, key: &str) -> Option<Builtin> {
    use Builtin::*;
    Some(match (builtin, key) {
        (RegExp, "prototype") => RegExpPrototype,
        (RegExpPrototype, "constructor") => RegExp,
        (RegExpPrototype, "compile") => RegExpCompile,
        (RegExp, "escape") => RegExpEscape,
        (RegExpPrototype, "test") => RegExpTest,
        (RegExpPrototype, "exec") => RegExpExec,
        (RegExpPrototype, "Symbol.match") => RegExpSymbolMatch,
        (RegExpPrototype, "Symbol.search") => RegExpSymbolSearch,
        (RegExpPrototype, "Symbol.replace") => RegExpSymbolReplace,
        (RegExpPrototype, "Symbol.split") => RegExpSymbolSplit,
        (RegExpPrototype, "Symbol.matchAll") => RegExpSymbolMatchAll,
        (RegExpStringIteratorPrototype, "next") => RegExpStringIteratorNext,
        (StringIteratorPrototype, "next") => StringIteratorNext,
        _ => return None,
    })
}
fn builtin_method_prefix(builtin: Builtin, key: &str) -> Option<Builtin> {
    if let Builtin::HostCapability(kind) = builtin {
        return host_capability_method(kind, key);
    }
    specialized_method(builtin, key).or_else(|| error_prototype(builtin, key))
}
fn error_prototype(builtin: Builtin, key: &str) -> Option<Builtin> {
    (key == "prototype"
        && matches!(
            builtin,
            Builtin::Error
                | Builtin::RangeError
                | Builtin::ReferenceError
                | Builtin::SyntaxError
                | Builtin::EvalError
                | Builtin::URIError
                | Builtin::TypeError
        ))
    .then_some(match builtin {
        Builtin::RangeError => Builtin::RangeErrorPrototype,
        Builtin::TypeError => Builtin::TypeErrorPrototype,
        Builtin::ReferenceError => Builtin::ReferenceErrorPrototype,
        Builtin::SyntaxError => Builtin::SyntaxErrorPrototype,
        Builtin::EvalError => Builtin::EvalErrorPrototype,
        Builtin::URIError => Builtin::URIErrorPrototype,
        _ => Builtin::ErrorPrototype,
    })
    .or_else(|| {
        (builtin == Builtin::AggregateError && key == "prototype")
            .then_some(Builtin::AggregateErrorPrototype)
    })
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
    typed_array_constructor_property(builtin, key)
        .or_else(|| {
            (builtin == Builtin::TypedArrayPrototype && key == "toString")
                .then_some(Builtin::ArrayToString)
        })
        .or_else(|| {
            (builtin == Builtin::TypedArrayPrototype && key == "slice").then_some(Builtin::ArraySlice)
        })
        .or_else(|| {
            (builtin == Builtin::TypedArrayPrototype && key == "values")
                .then_some(Builtin::TypedArrayIterator)
        })
        .or_else(|| {
            (builtin == Builtin::TypedArrayPrototype && key == "Symbol.iterator")
                .then_some(Builtin::TypedArrayIterator)
        })
        .or_else(|| {
            (builtin == Builtin::TypedArrayPrototype && key == "keys")
                .then_some(Builtin::TypedArrayKeys)
        })
        .or_else(|| {
            (builtin == Builtin::TypedArrayPrototype && key == "entries")
                .then_some(Builtin::TypedArrayEntries)
        })
        .or_else(|| {
            (builtin == Builtin::TypedArrayPrototype && key == "set").then_some(Builtin::TypedArraySet)
        })
        .or_else(|| {
            (builtin == Builtin::TypedArrayPrototype && key == "toLocaleString")
                .then_some(Builtin::TypedArrayToLocaleString)
        })
        .or_else(|| {
            (builtin == Builtin::TypedArrayPrototype && key == "fill").then_some(Builtin::TypedArrayFill)
        })
        .or_else(|| {
            (builtin == Builtin::TypedArrayPrototype && key == "indexOf")
                .then_some(Builtin::TypedArrayIndexOf)
        })
        .or_else(|| {
            (builtin == Builtin::TypedArrayPrototype && key == "every")
                .then_some(Builtin::TypedArrayEvery)
        })
        .or_else(|| {
            (builtin == Builtin::TypedArrayPrototype && key == "forEach")
                .then_some(Builtin::TypedArrayForEach)
        })
        .or_else(|| {
            (builtin == Builtin::TypedArrayPrototype && key == "find")
                .then_some(Builtin::TypedArrayFind)
        })
        .or_else(|| {
            (builtin == Builtin::TypedArrayPrototype && key == "findIndex")
                .then_some(Builtin::TypedArrayFindIndex)
        })
        .or_else(|| {
            (builtin == Builtin::TypedArrayPrototype && key == "findLast")
                .then_some(Builtin::TypedArrayFindLast)
        })
        .or_else(|| {
            (builtin == Builtin::TypedArrayPrototype && key == "findLastIndex")
                .then_some(Builtin::TypedArrayFindLastIndex)
        })
        .or_else(|| {
            (builtin == Builtin::TypedArrayPrototype && key == "includes")
                .then_some(Builtin::TypedArrayIncludes)
        })
        .or_else(|| {
            (builtin == Builtin::TypedArrayPrototype && key == "indexOf")
                .then_some(Builtin::TypedArrayIndexOf)
        })
        .or_else(|| {
            (builtin == Builtin::TypedArrayPrototype && key == "lastIndexOf")
                .then_some(Builtin::TypedArrayLastIndexOf)
        })
        .or_else(|| {
            (builtin == Builtin::TypedArrayPrototype && key == "at").then_some(Builtin::TypedArrayAt)
        })
        .or_else(|| {
            (builtin == Builtin::TypedArrayPrototype && key == "map").then_some(Builtin::ArrayMap)
        })
        .or_else(|| {
            (builtin == Builtin::TypedArrayPrototype && key == "filter").then_some(Builtin::ArrayFilter)
        })
        .or_else(|| {
            (builtin == Builtin::TypedArrayPrototype)
                .then(|| typed_array_generic_method(key))
                .flatten()
        })
        .or_else(|| uint8_array_base64_method(builtin, key))
}
fn typed_array_generic_method(key: &str) -> Option<Builtin> {
    use Builtin::*;
    Some(match key {
        "at" => ArrayAt,
        "copyWithin" => ArrayCopyWithin,
        "find" => ArrayFind,
        "findIndex" => ArrayFindIndex,
        "findLast" => ArrayFindLast,
        "findLastIndex" => ArrayFindLastIndex,
        "forEach" => ArrayForEach,
        "includes" => ArrayIncludes,
        "join" => TypedArrayJoin,
        "lastIndexOf" => ArrayLastIndexOf,
        "reduce" => ArrayReduce,
        "reduceRight" => ArrayReduceRight,
        "reverse" => TypedArrayReverse,
        "some" => ArraySome,
        "sort" => ArraySort,
        "toReversed" => ArrayToReversed,
        "toSorted" => ArrayToSorted,
        "with" => ArrayWith,
        _ => return None,
    })
}
fn uint8_array_base64_method(builtin: Builtin, key: &str) -> Option<Builtin> {
    if builtin == Builtin::Uint8ArrayPrototype {
        if let Some(method) = match key {
            "setFromBase64" => Some(Builtin::Uint8ArraySetFromBase64),
            "setFromHex" => Some(Builtin::Uint8ArraySetFromHex),
            "toBase64" => Some(Builtin::Uint8ArrayToBase64),
            "toHex" => Some(Builtin::Uint8ArrayToHex),
            _ => None,
        } {
            return Some(method);
        }
    }
    (key == "subarray" && builtin == Builtin::TypedArrayPrototype).then_some(Builtin::Uint8ArraySubarray)
}
fn host_capability_method(_kind: crate::ops::HostCapabilityKind, key: &str) -> Option<Builtin> {
    use crate::ops::HostCapabilityKind::*;
    if let Custom(id) = _kind {
        let custom = match (id, key) {
            (1, "basename") => Custom(2),
            (3, "log") => Custom(4),
            (5, "cwd") => Custom(6),
            _ => return None,
        };
        return Some(Builtin::HostCapability(custom));
    }
    let kind = match key {
        "global" => GetGlobal,
        "createRealm" => CreateRealm,
        "evalScript" => EvalScript,
        "detachArrayBuffer" => DetachArrayBuffer,
        "agent" => Agent,
        "start" => AgentStart,
        "broadcast" => AgentBroadcast,
        "report" => AgentReport,
        "getReport" => AgentGetReport,
        "leaving" => AgentLeaving,
        "receiveBroadcast" => AgentReceiveBroadcast,
        "sleep" => AgentSleep,
        "tryYield" => AgentTryYield,
        "trySleep" => AgentTrySleep,
        "setTimeout" => AgentSetTimeout,
        "monotonicNow" => AgentMonotonicNow,
        "IsHTMLDDA" => IsHTMLDDA,
        "AbstractModuleSource" => return Some(Builtin::AbstractModuleSource),
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
        "getBigInt64" => Some(DataViewGetBigInt64),
        "getBigUint64" => Some(DataViewGetBigUint64),
        "setInt8" => Some(DataViewSetInt8),
        "setUint8" => Some(DataViewSetUint8),
        "setInt16" => Some(DataViewSetInt16),
        "setUint16" => Some(DataViewSetUint16),
        "setInt32" => Some(DataViewSetInt32),
        "setUint32" => Some(DataViewSetUint32),
        "setFloat16" => Some(DataViewSetFloat16),
        "setFloat32" => Some(DataViewSetFloat32),
        "setFloat64" => Some(DataViewSetFloat64),
        "setBigInt64" => Some(DataViewSetBigInt64),
        "setBigUint64" => Some(DataViewSetBigUint64),
        _ => None,
    }
}
include!("props_builtin_methods.rs");
pub(crate) fn special_property(builtin: Builtin, key: &str) -> Option<Value> {
    special(builtin, key)
}
pub(crate) fn callable(builtin: Builtin, key: &str) -> Option<Value> {
    if !crate::conversion::is_callable(&Value::Builtin(builtin)) {
        return None;
    }
    if crate::builtin_meta::is_prototype(builtin)
        && !matches!(
            builtin,
            Builtin::FunctionPrototype | Builtin::StringPrototype
        )
        && matches!(key, "length" | "name")
    {
        return None;
    }
    match key {
        "call" => Some(Value::Builtin(Builtin::FunctionCall)),
        "bind" => Some(Value::Builtin(Builtin::FunctionBind)),
        "length" => Some(Value::Number(if builtin == Builtin::StringPrototype {
            0.0
        } else {
            builtin_length(builtin)
        })),
        "name" => Some(Value::String(builtin_name(builtin).to_string())),
        _ => None,
    }
}
include!("props_metadata.rs");
include!("props_builtin_names.rs");
