//! Declarative builtin/primordial metadata.
//!
//! Provides static metadata about JavaScript builtins: constructor names,
//! prototypes, function lengths, and short names.

use crate::ops::Builtin;

pub mod array;
pub mod atomics;
pub mod bigint;
pub mod collections;
pub mod dataview;
pub mod date;
pub mod disposable;
pub mod error;
pub mod finalization_registry;
pub mod function;
pub mod intl;
pub mod json;
pub mod math;
pub mod methods;
pub mod number;
pub mod object;
pub mod promise;
pub mod reflect;
pub mod regexp;
pub mod string;
pub mod symbol;
pub mod temporal;

/// Returns the constructor name for a builtin.
///
/// Returns `None` for builtins that are not constructors.
pub fn constructor_name(builtin: Builtin) -> Option<&'static str> {
    if let Some(name) = collection_constructor_name(builtin) {
        return Some(name);
    }
    if let Some(name) = intl_constructor_name(builtin) {
        return Some(name);
    }
    match builtin {
        Builtin::Array => Some("Array"),
        Builtin::Iterator => Some("Iterator"),
        Builtin::ArrayBuffer => Some("ArrayBuffer"),
        Builtin::Boolean => Some("Boolean"),
        Builtin::BigInt => Some("BigInt"),
        Builtin::DataView => Some("DataView"),
        Builtin::Proxy => Some("Proxy"),
        Builtin::Promise => Some("Promise"),
        Builtin::Date => Some("Date"),
        Builtin::DisposableStack => Some("DisposableStack"),
        Builtin::AsyncDisposableStack => Some("AsyncDisposableStack"),
        Builtin::FinalizationRegistry => Some("FinalizationRegistry"),
        Builtin::Function => Some("Function"),
        Builtin::AsyncFunction => Some("AsyncFunction"),
        Builtin::GeneratorFunction => Some("GeneratorFunction"),
        Builtin::AsyncGeneratorFunction => Some("AsyncGeneratorFunction"),
        Builtin::Math => Some("Math"),
        Builtin::Number => Some("Number"),
        Builtin::Object => Some("Object"),
        Builtin::RegExp => Some("RegExp"),
        Builtin::String => Some("String"),
        Builtin::Symbol => Some("Symbol"),
        Builtin::DOMException => Some("DOMException"),
        _ => constructor_name_tail(builtin),
    }
}

/// Returns whether a builtin uses the shared error-constructor path.
/// Keeping this fact here prevents constructor dispatch and metadata from
/// carrying separate, drifting lists of error types.
pub const fn is_error_constructor(builtin: Builtin) -> bool {
    matches!(
        builtin,
        Builtin::Error
            | Builtin::TypeError
            | Builtin::RangeError
            | Builtin::ReferenceError
            | Builtin::SyntaxError
            | Builtin::EvalError
            | Builtin::URIError
            | Builtin::AggregateError
            | Builtin::SuppressedError
            | Builtin::DOMException
    )
}

fn constructor_name_tail(builtin: Builtin) -> Option<&'static str> {
    match builtin {
        Builtin::Float64Array => Some("Float64Array"),
        Builtin::Float32Array => Some("Float32Array"),
        Builtin::Int8Array => Some("Int8Array"),
        Builtin::Int16Array => Some("Int16Array"),
        Builtin::Int32Array => Some("Int32Array"),
        Builtin::Uint8Array => Some("Uint8Array"),
        Builtin::Uint8ClampedArray => Some("Uint8ClampedArray"),
        Builtin::Uint16Array => Some("Uint16Array"),
        Builtin::Uint32Array => Some("Uint32Array"),
        Builtin::BigInt64Array => Some("BigInt64Array"),
        Builtin::BigUint64Array => Some("BigUint64Array"),
        Builtin::AbstractModuleSource => Some("AbstractModuleSource"),
        Builtin::ShadowRealm => Some("ShadowRealm"),
        Builtin::TypeError
        | Builtin::Error
        | Builtin::RangeError
        | Builtin::ReferenceError
        | Builtin::SyntaxError
        | Builtin::EvalError
        | Builtin::URIError
        | Builtin::AggregateError => error_constructor_name(builtin),
        Builtin::SuppressedError => Some("SuppressedError"),
        _ => None,
    }
}

fn intl_constructor_name(builtin: Builtin) -> Option<&'static str> {
    // Intl itself is the namespace object, not a constructor.
    if matches!(builtin, Builtin::Intl) {
        return None;
    }
    Some(match builtin {
        Builtin::IntlCollator => "Intl.Collator",
        Builtin::IntlDateTimeFormat => "DateTimeFormat",
        Builtin::IntlDisplayNames => "Intl.DisplayNames",
        Builtin::IntlDurationFormat => "DurationFormat",
        Builtin::IntlListFormat => "ListFormat",
        Builtin::IntlLocale => "Intl.Locale",
        Builtin::IntlNumberFormat => "NumberFormat",
        Builtin::IntlPluralRules => "Intl.PluralRules",
        Builtin::IntlRelativeTimeFormat => "Intl.RelativeTimeFormat",
        Builtin::IntlSegmenter => "Intl.Segmenter",
        Builtin::TemporalDuration => "Temporal.Duration",
        Builtin::TemporalInstant => "Temporal.Instant",
        Builtin::TemporalPlainDateTime => "Temporal.PlainDateTime",
        Builtin::TemporalPlainTime => "Temporal.PlainTime",
        Builtin::TemporalPlainMonthDay => "Temporal.PlainMonthDay",
        Builtin::TemporalPlainYearMonth => "Temporal.PlainYearMonth",
        Builtin::TemporalZonedDateTime => "Temporal.ZonedDateTime",
        Builtin::TemporalPlainDate => "Temporal.PlainDate",
        Builtin::ShadowRealm => "ShadowRealm",
        _ => return None,
    })
}

fn error_constructor_name(builtin: Builtin) -> Option<&'static str> {
    Some(match builtin {
        Builtin::TypeError => "TypeError",
        Builtin::Error => "Error",
        Builtin::RangeError => "RangeError",
        Builtin::ReferenceError => "ReferenceError",
        Builtin::SyntaxError => "SyntaxError",
        Builtin::EvalError => "EvalError",
        Builtin::URIError => "URIError",
        Builtin::AggregateError => "AggregateError",
        Builtin::SuppressedError => "SuppressedError",
        _ => return None,
    })
}

/// Returns the prototype builtin for a constructor builtin.
///
/// Returns `None` for builtins that are not constructors with prototypes.
pub fn prototype(builtin: Builtin) -> Option<Builtin> {
    match builtin {
        Builtin::Array => Some(Builtin::ArrayPrototype),
        Builtin::Iterator => Some(Builtin::IteratorPrototype),
        Builtin::Boolean => Some(Builtin::ObjectPrototype),
        Builtin::Promise => Some(Builtin::PromisePrototype),
        Builtin::Date => Some(Builtin::DatePrototype),
        Builtin::DisposableStack => Some(Builtin::DisposableStackPrototype),
        Builtin::AsyncDisposableStack => Some(Builtin::AsyncDisposableStackPrototype),
        Builtin::FinalizationRegistry => Some(Builtin::FinalizationRegistryPrototype),
        Builtin::Function => Some(Builtin::FunctionPrototype),
        Builtin::TypedArray => Some(Builtin::TypedArrayPrototype),
        Builtin::Float64Array => Some(Builtin::Float64ArrayPrototype),
        Builtin::Float32Array => Some(Builtin::Float32ArrayPrototype),
        Builtin::Int8Array => Some(Builtin::Int8ArrayPrototype),
        Builtin::Int16Array => Some(Builtin::Int16ArrayPrototype),
        Builtin::Int32Array => Some(Builtin::Int32ArrayPrototype),
        Builtin::Uint8Array => Some(Builtin::Uint8ArrayPrototype),
        Builtin::Uint8ClampedArray => Some(Builtin::Uint8ClampedArrayPrototype),
        Builtin::Uint16Array => Some(Builtin::Uint16ArrayPrototype),
        Builtin::Uint32Array => Some(Builtin::Uint32ArrayPrototype),
        Builtin::BigInt64Array => Some(Builtin::BigInt64ArrayPrototype),
        Builtin::BigUint64Array => Some(Builtin::BigUint64ArrayPrototype),
        Builtin::AsyncFunction => Some(Builtin::FunctionPrototype),
        Builtin::GeneratorFunction => Some(Builtin::GeneratorFunctionPrototype),
        Builtin::AsyncGeneratorFunction => Some(Builtin::AsyncGeneratorFunctionPrototype),
        Builtin::Number => Some(Builtin::ObjectPrototype),
        Builtin::Object => Some(Builtin::ObjectPrototype),
        Builtin::RegExp => Some(Builtin::RegExpPrototype),
        Builtin::String => Some(Builtin::ObjectPrototype),
        Builtin::TemporalDuration => Some(Builtin::TemporalDurationPrototype),
        Builtin::TemporalInstant => Some(Builtin::TemporalInstantPrototype),
        Builtin::TemporalZonedDateTime => Some(Builtin::TemporalZonedDateTimePrototype),
        Builtin::TemporalPlainDateTime => Some(Builtin::TemporalPlainDateTimePrototype),
        Builtin::TemporalPlainTime => Some(Builtin::TemporalPlainTimePrototype),
        Builtin::TemporalPlainMonthDay => Some(Builtin::TemporalPlainMonthDayPrototype),
        Builtin::TemporalPlainYearMonth => Some(Builtin::TemporalPlainYearMonthPrototype),
        Builtin::TemporalPlainDate => Some(Builtin::TemporalPlainDatePrototype),
        Builtin::AbstractModuleSource => Some(Builtin::AbstractModuleSourcePrototype),
        Builtin::ShadowRealm => Some(Builtin::ShadowRealmPrototype),
        Builtin::Symbol => Some(Builtin::ObjectPrototype),
        _ => prototype_tail(builtin),
    }
}

fn prototype_tail(builtin: Builtin) -> Option<Builtin> {
    match builtin {
        Builtin::TypedArrayPrototype => Some(Builtin::ObjectPrototype),
        Builtin::Float64ArrayPrototype
        | Builtin::Float32ArrayPrototype
        | Builtin::Int8ArrayPrototype
        | Builtin::Int16ArrayPrototype
        | Builtin::Int32ArrayPrototype
        | Builtin::Uint8ArrayPrototype
        | Builtin::Uint16ArrayPrototype
        | Builtin::Uint32ArrayPrototype
        | Builtin::Uint8ClampedArrayPrototype
        | Builtin::BigInt64ArrayPrototype
        | Builtin::BigUint64ArrayPrototype => Some(Builtin::TypedArrayPrototype),
        Builtin::ArrayBuffer => Some(Builtin::ArrayBufferPrototype),
        Builtin::DataView => Some(Builtin::DataViewPrototype),
        Builtin::IntlCollator => Some(Builtin::IntlCollatorPrototype),
        Builtin::IntlDateTimeFormat => Some(Builtin::IntlDateTimeFormatPrototype),
        Builtin::IntlDisplayNames => Some(Builtin::IntlDisplayNamesPrototype),
        Builtin::IntlDurationFormat => Some(Builtin::IntlDurationFormatPrototype),
        Builtin::IntlListFormat => Some(Builtin::IntlListFormatPrototype),
        Builtin::IntlLocale => Some(Builtin::IntlLocalePrototype),
        Builtin::IntlNumberFormat => Some(Builtin::IntlNumberFormatPrototype),
        Builtin::IntlPluralRules => Some(Builtin::IntlPluralRulesPrototype),
        Builtin::IntlRelativeTimeFormat => Some(Builtin::IntlRelativeTimeFormatPrototype),
        Builtin::IntlSegmenter => Some(Builtin::IntlSegmenterPrototype),
        Builtin::Map => Some(Builtin::MapPrototype),
        Builtin::Set => Some(Builtin::SetPrototype),
        Builtin::WeakMap => Some(Builtin::WeakMapPrototype),
        Builtin::SharedArrayBuffer => Some(Builtin::SharedArrayBufferPrototype),
        Builtin::WeakSet => Some(Builtin::WeakSetPrototype),
        Builtin::WeakRef => Some(Builtin::WeakRefPrototype),
        Builtin::Error => Some(Builtin::ErrorPrototype),
        Builtin::DOMException => Some(Builtin::DOMExceptionPrototype),
        Builtin::RangeError => Some(Builtin::RangeErrorPrototype),
        Builtin::TypeError => Some(Builtin::TypeErrorPrototype),
        Builtin::ReferenceError => Some(Builtin::ReferenceErrorPrototype),
        Builtin::SyntaxError => Some(Builtin::SyntaxErrorPrototype),
        Builtin::EvalError => Some(Builtin::EvalErrorPrototype),
        Builtin::URIError => Some(Builtin::URIErrorPrototype),
        Builtin::AggregateError => Some(Builtin::AggregateErrorPrototype),
        Builtin::SuppressedError => Some(Builtin::SuppressedErrorPrototype),
        _ => None,
    }
}

pub fn is_prototype(builtin: Builtin) -> bool {
    is_runtime_prototype(builtin)
        || is_intl_prototype(builtin)
        || matches!(
            builtin,
            Builtin::MapPrototype
                | Builtin::SetPrototype
                | Builtin::WeakMapPrototype
                | Builtin::WeakSetPrototype
                | Builtin::WeakRefPrototype
                | Builtin::FinalizationRegistryPrototype
                | Builtin::ErrorPrototype
                | Builtin::RangeErrorPrototype
                | Builtin::TypeErrorPrototype
                | Builtin::EvalErrorPrototype
                | Builtin::ReferenceErrorPrototype
                | Builtin::SyntaxErrorPrototype
                | Builtin::URIErrorPrototype
                | Builtin::AggregateErrorPrototype
                | Builtin::SuppressedErrorPrototype
                | Builtin::DOMExceptionPrototype
                | Builtin::PromisePrototype
        )
}

fn is_runtime_prototype(builtin: Builtin) -> bool {
    matches!(
        builtin,
        Builtin::IteratorPrototype
            | Builtin::TypedArrayPrototype
            | Builtin::ArrayBufferPrototype
            | Builtin::SharedArrayBufferPrototype
            | Builtin::Float64ArrayPrototype
            | Builtin::Float32ArrayPrototype
            | Builtin::Int8ArrayPrototype
            | Builtin::Int16ArrayPrototype
            | Builtin::Int32ArrayPrototype
            | Builtin::Uint8ArrayPrototype
            | Builtin::Uint16ArrayPrototype
            | Builtin::Uint32ArrayPrototype
            | Builtin::Uint8ClampedArrayPrototype
            | Builtin::BigInt64ArrayPrototype
            | Builtin::BigUint64ArrayPrototype
            | Builtin::DataViewPrototype
            | Builtin::ArrayPrototype
            | Builtin::FunctionPrototype
            | Builtin::AsyncFunctionPrototype
            | Builtin::GeneratorFunctionPrototype
            | Builtin::AsyncGeneratorFunctionPrototype
            | Builtin::DatePrototype
            | Builtin::DisposableStackPrototype
            | Builtin::AsyncDisposableStackPrototype
            | Builtin::RegExpPrototype
            | Builtin::RegExpStringIteratorPrototype
            | Builtin::SetIteratorPrototype
            | Builtin::MapIteratorPrototype
            | Builtin::ObjectPrototype
            | Builtin::ArrayIteratorPrototype
            | Builtin::StringIteratorPrototype
            | Builtin::NumberPrototype
            | Builtin::BooleanPrototype
    ) || is_runtime_prototype_tail(builtin)
}

fn is_runtime_prototype_tail(builtin: Builtin) -> bool {
    matches!(
        builtin,
        Builtin::SymbolPrototype
            | Builtin::StringPrototype
            | Builtin::BigIntPrototype
            | Builtin::TemporalDurationPrototype
            | Builtin::TemporalInstantPrototype
            | Builtin::TemporalZonedDateTimePrototype
            | Builtin::TemporalPlainDatePrototype
            | Builtin::TemporalPlainDateTimePrototype
            | Builtin::TemporalPlainTimePrototype
            | Builtin::TemporalPlainMonthDayPrototype
            | Builtin::TemporalPlainYearMonthPrototype
            | Builtin::AbstractModuleSourcePrototype
            | Builtin::ShadowRealmPrototype
            | Builtin::DOMExceptionPrototype
    )
}

fn is_intl_prototype(builtin: Builtin) -> bool {
    matches!(
        builtin,
        Builtin::IntlLocalePrototype
            | Builtin::IntlNumberFormatPrototype
            | Builtin::IntlPluralRulesPrototype
            | Builtin::IntlDateTimeFormatPrototype
            | Builtin::IntlCollatorPrototype
            | Builtin::IntlListFormatPrototype
            | Builtin::IntlRelativeTimeFormatPrototype
            | Builtin::IntlSegmenterPrototype
            | Builtin::IntlDisplayNamesPrototype
            | Builtin::IntlDurationFormatPrototype
    )
}

pub fn instance_prototype(builtin: Builtin) -> Option<Builtin> {
    match builtin {
        Builtin::Boolean => Some(Builtin::BooleanPrototype),
        Builtin::Number => Some(Builtin::NumberPrototype),
        Builtin::String => Some(Builtin::StringPrototype),
        Builtin::Symbol => Some(Builtin::SymbolPrototype),
        Builtin::BigInt => Some(Builtin::BigIntPrototype),
        Builtin::AsyncFunction => Some(Builtin::AsyncFunctionPrototype),
        _ => prototype(builtin),
    }
}

/// Returns the `length` property value for a builtin constructor.
///
/// Returns `None` for builtins that are not constructors or have no defined length.
pub fn constructor_length(builtin: Builtin) -> Option<f64> {
    if let Some(length) = collection_constructor_length(builtin) {
        return Some(length);
    }
    if let Some(length) = intl_constructor_length(builtin) {
        return Some(length);
    }
    match builtin {
        Builtin::Array => Some(1.0),
        Builtin::Iterator => Some(0.0),
        Builtin::ArrayBuffer => Some(1.0),
        Builtin::Boolean => Some(1.0),
        Builtin::BigInt => Some(1.0),
        Builtin::DataView => Some(1.0),
        Builtin::Proxy => Some(2.0),
        Builtin::Promise => Some(1.0),
        Builtin::Date => Some(7.0),
        Builtin::TemporalDuration => Some(0.0),
        Builtin::TemporalInstant => Some(1.0),
        Builtin::TemporalPlainDateTime => Some(3.0),
        Builtin::TemporalPlainTime => Some(0.0),
        Builtin::TemporalPlainMonthDay => Some(2.0),
        Builtin::TemporalPlainYearMonth => Some(2.0),
        Builtin::TemporalZonedDateTime => Some(2.0),
        Builtin::TemporalPlainDate => Some(3.0),
        Builtin::AbstractModuleSource | Builtin::ShadowRealm => Some(0.0),
        Builtin::DisposableStack => Some(0.0),
        Builtin::AsyncDisposableStack => Some(0.0),
        Builtin::FinalizationRegistry => Some(1.0),
        Builtin::Function => Some(1.0),
        Builtin::AsyncFunction | Builtin::GeneratorFunction | Builtin::AsyncGeneratorFunction => {
            Some(1.0)
        }
        Builtin::Math => None,
        Builtin::Number => Some(1.0),
        Builtin::Object => Some(1.0),
        Builtin::RegExp => Some(2.0),
        Builtin::String => Some(1.0),
        Builtin::Symbol => Some(0.0),
        Builtin::DOMException => Some(2.0),
        _ => constructor_length_tail(builtin),
    }
}

fn constructor_length_tail(builtin: Builtin) -> Option<f64> {
    match builtin {
        Builtin::TypeError
        | Builtin::Error
        | Builtin::RangeError
        | Builtin::ReferenceError
        | Builtin::SyntaxError
        | Builtin::EvalError
        | Builtin::URIError => Some(1.0),
        Builtin::AggregateError => Some(2.0),
        Builtin::SuppressedError => Some(3.0),
        Builtin::DOMException => Some(2.0),
        _ => None,
    }
}

fn intl_constructor_length(builtin: Builtin) -> Option<f64> {
    Some(match builtin {
        Builtin::Intl
        | Builtin::IntlCollator
        | Builtin::IntlPluralRules
        | Builtin::IntlListFormat
        | Builtin::IntlRelativeTimeFormat => 0.0,
        Builtin::IntlDateTimeFormat => 0.0,
        Builtin::IntlDisplayNames => 2.0,
        Builtin::IntlNumberFormat => 0.0,
        Builtin::IntlDurationFormat => 0.0,
        Builtin::IntlSegmenter => 0.0,
        Builtin::IntlLocale => 1.0,
        _ => return None,
    })
}

fn collection_constructor_name(builtin: Builtin) -> Option<&'static str> {
    match builtin {
        Builtin::Map => Some("Map"),
        Builtin::Set => Some("Set"),
        Builtin::WeakMap => Some("WeakMap"),
        Builtin::SharedArrayBuffer => Some("SharedArrayBuffer"),
        Builtin::WeakSet => Some("WeakSet"),
        Builtin::WeakRef => Some("WeakRef"),
        _ => None,
    }
}

fn collection_constructor_length(builtin: Builtin) -> Option<f64> {
    match builtin {
        Builtin::Map | Builtin::Set | Builtin::WeakMap | Builtin::WeakSet => Some(0.0),
        Builtin::WeakRef => Some(1.0),
        Builtin::SharedArrayBuffer => Some(1.0),
        _ => None,
    }
}
