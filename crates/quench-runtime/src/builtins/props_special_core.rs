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
    if let Some(value) = special_match_prefix(builtin, key) {
        return Some(value);
    }
    match (builtin, key) {
        (Temporal, "Duration") => Some(Value::Builtin(TemporalDuration)),
        (IntlSegmenterPrototype, "Symbol.toStringTag") => {
            Some(Value::String("Intl.Segmenter".into()))
        }
        (Intl, "Symbol.toStringTag") => Some(Value::String("Intl".into())),
        (IntlDisplayNamesPrototype, "Symbol.toStringTag") => {
            Some(Value::String("Intl.DisplayNames".into()))
        }
        (IntlListFormatPrototype, "Symbol.toStringTag") => {
            Some(Value::String("Intl.ListFormat".into()))
        }
        (IntlLocalePrototype, "Symbol.toStringTag") => Some(Value::String("Intl.Locale".into())),
        (Temporal, "PlainDate") => Some(Value::Builtin(TemporalPlainDate)),
        (Temporal, "Symbol.toStringTag") => Some(Value::String("Temporal".into())),
        (TemporalDuration, "prototype") => Some(Value::Builtin(TemporalDurationPrototype)),
        (TemporalDuration, "from") => Some(Value::Builtin(TemporalDurationFrom)),
        (TemporalDuration, "compare") => Some(Value::Builtin(TemporalDurationCompare)),
        (TemporalDurationPrototype, "abs") => Some(Value::Builtin(TemporalDurationAbs)),
        (TemporalPlainDate, "prototype") => Some(Value::Builtin(TemporalPlainDatePrototype)),
        (TemporalPlainDate, "from") => Some(Value::Builtin(TemporalPlainDateFrom)),
        (TemporalPlainDatePrototype, "constructor") => Some(Value::Builtin(TemporalPlainDate)),
        (AbstractModuleSource, "prototype") => Some(Value::Builtin(AbstractModuleSourcePrototype)),
        (AbstractModuleSourcePrototype, "constructor") => {
            Some(Value::Builtin(AbstractModuleSource))
        }
        (ShadowRealmPrototype, "constructor") => Some(Value::Builtin(ShadowRealm)),
        (ShadowRealm, "prototype") => Some(Value::Builtin(ShadowRealmPrototype)),
        (ShadowRealmPrototype, "evaluate") => Some(Value::Builtin(ShadowRealmEvaluate)),
        (ShadowRealmPrototype, "importValue") => Some(Value::Builtin(ShadowRealmImportValue)),
        (ShadowRealmPrototype, "Symbol.toStringTag") => Some(Value::String("ShadowRealm".into())),
        (String, "prototype") => Some(Value::Builtin(StringPrototype)),
        (StringPrototype, "constructor") => Some(Value::Builtin(String)),
        _ => special_match_middle(builtin, key),
    }
}

fn special_match_prefix(builtin: Builtin, key: &str) -> Option<Value> {
    use Builtin::*;
    if builtin == ArrayIteratorPrototype && key == "constructor" {
        return Some(Value::Builtin(Array));
    }
    if builtin == ArrayIteratorPrototype && key == "next" {
        return Some(Value::Builtin(IteratorNext));
    }
    if builtin == ArrayIteratorPrototype && key == "Symbol.toStringTag" {
        return Some(Value::String("Array Iterator".into()));
    }
    if builtin == IteratorPrototype && key == "Symbol.iterator" {
        return Some(Value::Builtin(IteratorSelf));
    }
    if builtin == IteratorPrototype && key == "toArray" {
        return Some(Value::Builtin(IteratorToArray));
    }
    if builtin == IteratorPrototype && key == "map" {
        return Some(Value::Builtin(IteratorMap));
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
    None
}

fn special_match_middle(builtin: Builtin, key: &str) -> Option<Value> {
    use Builtin::*;
    match (builtin, key) {
        (ArrayBufferPrototype, "Symbol.toStringTag") => Some(Value::String("ArrayBuffer".into())),
        (RegExpStringIteratorPrototype, "Symbol.toStringTag") => {
            Some(Value::String("RegExp String Iterator".into()))
        }
        (SharedArrayBufferPrototype, "Symbol.toStringTag") => {
            Some(Value::String("SharedArrayBuffer".into()))
        }
        (StringIteratorPrototype, "Symbol.toStringTag") => {
            Some(Value::String("String Iterator".into()))
        }
        (Math, "Symbol.toStringTag") => Some(Value::String("Math".into())),
        (Atomics, "Symbol.toStringTag") => Some(Value::String("Atomics".into())),
        (Atomics, "add") => Some(Value::Builtin(AtomicsAdd)),
        (Atomics, "and") => Some(Value::Builtin(AtomicsAnd)),
        (Atomics, "or") => Some(Value::Builtin(AtomicsOr)),
        (Atomics, "sub") => Some(Value::Builtin(AtomicsSub)),
        (Atomics, "xor") => Some(Value::Builtin(AtomicsXor)),
        (Atomics, "compareExchange") => Some(Value::Builtin(AtomicsCompareExchange)),
        (Atomics, "isLockFree") => Some(Value::Builtin(AtomicsIsLockFree)),
        (Atomics, "notify") => Some(Value::Builtin(AtomicsNotify)),
        (Atomics, "wait") => Some(Value::Builtin(AtomicsWait)),
        (Atomics, "load") => Some(Value::Builtin(AtomicsLoad)),
        (Atomics, "store") => Some(Value::Builtin(AtomicsStore)),
        (Atomics, "exchange") => Some(Value::Builtin(AtomicsExchange)),
        (Atomics, "waitAsync") => Some(Value::Builtin(AtomicsWaitAsync)),
        (Atomics, "pause") => Some(Value::Builtin(AtomicsPause)),
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
        (AsyncFunctionPrototype, "Symbol.toStringTag") => {
            Some(Value::String("AsyncFunction".to_string()))
        }
        (GeneratorFunctionPrototype, "Symbol.toStringTag") => {
            Some(Value::String("GeneratorFunction".to_string()))
        }
        (GeneratorFunctionPrototype, "prototype") => Some(crate::builtins::generator_prototype()),
        (DataViewPrototype, "Symbol.toStringTag") => Some(Value::String("DataView".into())),
        (AsyncGeneratorFunctionPrototype, "Symbol.toStringTag") => {
            Some(Value::String("AsyncGeneratorFunction".into()))
        }
        (AsyncGeneratorFunctionPrototype, "prototype") => {
            Some(crate::builtins::async_generator_prototype())
        }
        _ => special_match_tail(builtin, key),
    }
}

fn special_match_tail(builtin: Builtin, key: &str) -> Option<Value> {
    use Builtin::*;
    match (builtin, key) {
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
        _ => special_match_error_tail(builtin, key),
    }
}

fn special_match_error_tail(builtin: Builtin, key: &str) -> Option<Value> {
    use Builtin::*;
    match (builtin, key) {
        (ErrorPrototype, "toString") => Some(Value::Builtin(ErrorPrototypeToString)),
        (ErrorPrototype, "name") => Some(Value::String("Error".to_string())),
        (ErrorPrototype, "message") => Some(Value::String("".to_string())),
        (ErrorPrototype, "constructor") => Some(Value::Builtin(Error)),
        (RangeErrorPrototype, "name") => Some(Value::String("RangeError".to_string())),
        (RangeErrorPrototype, "message") => Some(Value::String("".to_string())),
        (RangeErrorPrototype, "constructor") => Some(Value::Builtin(RangeError)),
        (TypeErrorPrototype, "name") => Some(Value::String("TypeError".to_string())),
        (TypeErrorPrototype, "message") => Some(Value::String("".to_string())),
        (TypeErrorPrototype, "constructor") => Some(Value::Builtin(TypeError)),
        (ReferenceErrorPrototype, "name") => Some(Value::String("ReferenceError".to_string())),
        (ReferenceErrorPrototype, "message") => Some(Value::String("".to_string())),
        (ReferenceErrorPrototype, "constructor") => Some(Value::Builtin(ReferenceError)),
        (SyntaxErrorPrototype, "name") => Some(Value::String("SyntaxError".to_string())),
        (SyntaxErrorPrototype, "message") => Some(Value::String("".to_string())),
        (SyntaxErrorPrototype, "constructor") => Some(Value::Builtin(SyntaxError)),
        (EvalErrorPrototype, "name") => Some(Value::String("EvalError".to_string())),
        (EvalErrorPrototype, "message") => Some(Value::String("".to_string())),
        (EvalErrorPrototype, "constructor") => Some(Value::Builtin(EvalError)),
        (URIErrorPrototype, "name") => Some(Value::String("URIError".to_string())),
        (URIErrorPrototype, "message") => Some(Value::String("".to_string())),
        (URIErrorPrototype, "constructor") => Some(Value::Builtin(URIError)),
        (AggregateError, "prototype") => Some(Value::Builtin(AggregateErrorPrototype)),
        (AggregateErrorPrototype, "name") => Some(Value::String("AggregateError".to_string())),
        (AggregateErrorPrototype, "message") => Some(Value::String("".to_string())),
        (AggregateErrorPrototype, "constructor") => Some(Value::Builtin(AggregateError)),
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
