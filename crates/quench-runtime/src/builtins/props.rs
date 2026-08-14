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
    if builtin == Number {
        return props_number::constant(key).or_else(|| special_match(builtin, key));
    }
    if builtin == Math {
        return crate::math::constant(key)
            .or_else(|| crate::math::property(key).map(Value::Builtin))
            .or_else(|| special_match(builtin, key));
    }
    if builtin == Json && key == "stringify" {
        return Some(Value::Builtin(JsonStringify));
    }
    special_match(builtin, key)
}
fn special_match(builtin: Builtin, key: &str) -> Option<Value> {
    use Builtin::*;
    if builtin == ArrayIteratorPrototype && key == "constructor" {
        return Some(Value::Builtin(Array));
    }
    if builtin == IteratorPrototype && key == "Symbol.iterator" {
        return Some(Value::Builtin(IteratorSelf));
    }
    if let Some(value) = typed_array_static_property(builtin, key) {
        return Some(value);
    }
    if let Some(value) = weak_special(builtin, key) {
        return Some(value);
    }
    if builtin == Builtin::Error && key == "isError" {
        return Some(Value::Builtin(Builtin::ErrorIsError));
    }
    match (builtin, key) {
        (RegExpStringIteratorPrototype, "Symbol.toStringTag") => {
            Some(Value::String("RegExp String Iterator".into()))
        }
        (Math, "Symbol.toStringTag") => Some(Value::String("Math".into())),
        (Reflect, "Symbol.toStringTag") => Some(Value::String("Reflect".into())),
        (SymbolPrototype, "Symbol.toStringTag") => Some(Value::String("Symbol".into())),
        (Symbol, "prototype") => Some(Value::Builtin(SymbolPrototype)),
        (Symbol, "unscopables") => Some(Value::String("Symbol.unscopables\0".to_string())),
        (Symbol, k) => crate::builtin_meta::symbol::symbol_prop(k).map(Value::Builtin),
        (Map, "groupBy") => Some(Value::Builtin(MapGroupBy)),
        (Set, "Symbol.species") => Some(Value::Builtin(Set)),
        (Map, "Symbol.species") => Some(Value::Builtin(Map)),
        (MapPrototype | SetPrototype | SetIteratorPrototype | MapIteratorPrototype, k) => {
            collections_prop(builtin, k)
        }
        (BigIntPrototype, "Symbol.toStringTag") => Some(Value::String("BigInt".to_string())),
        (DataViewPrototype, "Symbol.toStringTag") => Some(Value::String("DataView".into())),
        (FinalizationRegistry, "prototype") => Some(Value::Builtin(FinalizationRegistryPrototype)),
        (FinalizationRegistryPrototype, "Symbol.toStringTag") => {
            Some(Value::String("FinalizationRegistry".into()))
        }
        (FinalizationRegistryPrototype, k) => {
            crate::builtin_meta::finalization_registry::property(k).map(Value::Builtin)
        }
        (ObjectPrototype, "constructor") => Some(Value::Builtin(Object)),
        (DatePrototype, k) => crate::builtin_meta::date::date_prop(k).map(Value::Builtin),
        (DisposableStack, "prototype") => Some(Value::Builtin(DisposableStackPrototype)),
        (AsyncDisposableStack, "prototype") => Some(Value::Builtin(AsyncDisposableStackPrototype)),
        (AsyncDisposableStackPrototype, "constructor") => {
            Some(Value::Builtin(AsyncDisposableStack))
        }
        (AsyncDisposableStackPrototype, "Symbol.toStringTag") => {
            Some(Value::String("AsyncDisposableStack".into()))
        }
        (AsyncDisposableStackPrototype, k) => {
            crate::builtin_meta::disposable::async_property(k).map(Value::Builtin)
        }
        (DisposableStackPrototype, "Symbol.dispose") => {
            Some(Value::Builtin(DisposableStackDispose))
        }
        (ErrorPrototype, "toString") => Some(Value::Builtin(ErrorPrototypeToString)),
        (ErrorPrototype, "name") => Some(Value::String("Error".to_string())),
        (ErrorPrototype, "message") => Some(Value::String("".to_string())),
        (ErrorPrototype, "cause") => Some(Value::Undefined),
        (ErrorPrototype, "constructor") => Some(Value::Builtin(Error)),
        (SuppressedError, "prototype") => Some(Value::Builtin(SuppressedErrorPrototype)),
        (SuppressedErrorPrototype, "name") => Some(Value::String("SuppressedError".to_string())),
        (SuppressedErrorPrototype, "message") => Some(Value::String("".to_string())),
        (SuppressedErrorPrototype, "constructor") => Some(Value::Builtin(SuppressedError)),
        (DisposableStackPrototype, "Symbol.toStringTag") => {
            Some(Value::String("DisposableStack".into()))
        }
        (DisposableStackPrototype, k) => {
            crate::builtin_meta::disposable::property(k).map(Value::Builtin)
        }
        _ => builtin_method(builtin, key).map(Value::Builtin),
    }
}

fn weak_special(builtin: Builtin, key: &str) -> Option<Value> {
    use Builtin::*;
    match (builtin, key) {
        (WeakMapPrototype, "constructor") => Some(Value::Builtin(WeakMap)),
        (WeakMapPrototype, "Symbol.toStringTag") => Some(Value::String("WeakMap".into())),
        (WeakMapPrototype, k) => match crate::collections::map::weak_property(k) {
            Value::Builtin(value) => Some(Value::Builtin(value)),
            _ => None,
        },
        (WeakMap, "prototype") => Some(Value::Builtin(WeakMapPrototype)),
        (WeakSetPrototype, "constructor") => Some(Value::Builtin(WeakSet)),
        (WeakSetPrototype, "Symbol.toStringTag") => Some(Value::String("WeakSet".into())),
        (WeakSetPrototype, k) => match crate::collections::set::weak_property(k) {
            Value::Builtin(value) => Some(Value::Builtin(value)),
            _ => None,
        },
        (WeakSet, "prototype") => Some(Value::Builtin(WeakSetPrototype)),
        (WeakRef, "prototype") => Some(Value::Builtin(WeakRefPrototype)),
        (WeakRefPrototype, "constructor") => Some(Value::Builtin(WeakRef)),
        (WeakRefPrototype, "deref") => Some(Value::Builtin(WeakRefDeref)),
        (WeakRefPrototype, "Symbol.toStringTag") => Some(Value::String("WeakRef".into())),
        _ => None,
    }
}
fn iterator_property(builtin: Builtin, key: &str) -> Option<Value> {
    if builtin != Builtin::IteratorPrototype {
        return None;
    }
    match key {
        "Symbol.iterator" => Some(Value::Builtin(Builtin::IteratorSelf)),
        "Symbol.toStringTag" => Some(Value::String("Iterator".into())),
        "constructor" => Some(Value::Builtin(Builtin::Iterator)),
        _ => None,
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
    if builtin == ArrayBuffer && key == "prototype" {
        return Some(ArrayBufferPrototype);
    }
    if builtin == Iterator && key == "prototype" {
        return Some(IteratorPrototype);
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
    if let Some(method) = regexp_method(builtin, key) {
        return Some(method);
    }
    match (builtin, key) {
        (Array, "prototype") => Some(ArrayPrototype),
        (ArrayBuffer, "isView") => Some(ArrayBufferIsView),
        (ArrayBufferPrototype, "resize") => Some(ArrayBufferResize),
        (ArrayBufferPrototype, "transferToImmutable") => Some(ArrayBufferTransferToImmutable),
        (DataView, "prototype") => Some(DataViewPrototype),
        (DataViewPrototype, "constructor") => Some(DataView),
        (Function, "prototype") => Some(FunctionPrototype),
        (AsyncFunction, "prototype") => Some(FunctionPrototype),
        (GeneratorFunction, "prototype") => Some(GeneratorFunctionPrototype),
        (AsyncGeneratorFunction, "prototype") => Some(AsyncGeneratorFunctionPrototype),
        (GeneratorFunctionPrototype, "constructor") => Some(GeneratorFunction),
        (AsyncGeneratorFunctionPrototype, "constructor") => Some(AsyncGeneratorFunction),
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
        (Date, "UTC") => Some(DateUTC),
        _ => builtin_method2(builtin, key),
    }
}
fn regexp_method(builtin: Builtin, key: &str) -> Option<Builtin> {
    use Builtin::*;
    Some(match (builtin, key) {
        (RegExp, "prototype") => RegExpPrototype,
        (RegExpPrototype, "constructor") => RegExp,
        (RegExp, "escape") => RegExpEscape,
        (RegExpPrototype, "test") => RegExpTest,
        (RegExpPrototype, "exec") => RegExpExec,
        (RegExpPrototype, "Symbol.match") => RegExpSymbolMatch,
        (RegExpPrototype, "Symbol.search") => RegExpSymbolSearch,
        (RegExpPrototype, "Symbol.replace") => RegExpSymbolReplace,
        (RegExpPrototype, "Symbol.split") => RegExpSymbolSplit,
        (RegExpPrototype, "Symbol.matchAll") => RegExpSymbolMatchAll,
        (RegExpStringIteratorPrototype, "next") => RegExpStringIteratorNext,
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
                | Builtin::AggregateError
                | Builtin::TypeError
        ))
    .then_some(Builtin::ErrorPrototype)
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
            if !is_typed_array_prototype(builtin) {
                return None;
            }
            Some(match key {
                "fill" => Builtin::TypedArrayFill,
                "toLocaleString" => Builtin::ArrayToLocaleString,
                _ => return None,
            })
        })
        .or_else(|| uint8_array_base64_method(builtin, key))
}
fn uint8_array_base64_method(builtin: Builtin, key: &str) -> Option<Builtin> {
    if builtin != Builtin::Uint8ArrayPrototype {
        return None;
    }
    Some(match key {
        "setFromBase64" => Builtin::Uint8ArraySetFromBase64,
        "setFromHex" => Builtin::Uint8ArraySetFromHex,
        "toBase64" => Builtin::Uint8ArrayToBase64,
        "toHex" => Builtin::Uint8ArrayToHex,
        "subarray" => Builtin::Uint8ArraySubarray,
        _ => return None,
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
            | BigInt64ArrayPrototype
            | BigUint64ArrayPrototype
    )
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
fn array_method(key: &str) -> Option<Builtin> {
    use Builtin::*;
    match key {
        "forEach" => Some(ArrayForEach),
        "map" => Some(ArrayMap),
        "filter" => Some(ArrayFilter),
        "some" => Some(ArraySome),
        "every" => Some(ArrayEvery),
        "find" => Some(ArrayFind),
        "findLast" => Some(ArrayFindLast),
        "findLastIndex" => Some(ArrayFindLastIndex),
        "includes" => Some(ArrayIncludes),
        "indexOf" => Some(ArrayIndexOf),
        "lastIndexOf" => Some(ArrayLastIndexOf),
        "slice" => Some(ArraySlice),
        "concat" => Some(ArrayConcat),
        "flat" => Some(ArrayFlat),
        "flatMap" => Some(ArrayFlatMap),
        "at" => Some(ArrayAt),
        "toReversed" => Some(ArrayToReversed),
        "join" | "toString" => Some(ArrayJoin),
        "reduce" => Some(ArrayReduce),
        "reduceRight" => Some(ArrayReduceRight),
        "toLocaleString" => Some(ArrayToLocaleString),
        "values" => Some(ArrayIterator),
        "Symbol.iterator" => Some(ArrayIterator),
        "keys" => Some(ArrayKeys),
        "entries" => Some(ArrayEntries),
        "push" => Some(ArrayPush),
        "shift" => Some(ArrayShift),
        "reverse" => Some(ArrayReverse),
        "pop" => Some(ArrayPop),
        "unshift" => Some(ArrayUnshift),
        "fill" => Some(ArrayFill),
        "copyWithin" => Some(ArrayCopyWithin),
        "toSorted" => Some(ArrayToSorted),
        "splice" => Some(ArraySplice),
        _ => None,
    }
}
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
