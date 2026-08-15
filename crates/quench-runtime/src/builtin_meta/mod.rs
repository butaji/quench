//! Declarative builtin/primordial metadata.
//!
//! Provides static metadata about JavaScript builtins: constructor names,
//! prototypes, function lengths, and short names.

use crate::ops::Builtin;

pub mod array;
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
        Builtin::AbstractModuleSource => Some("AbstractModuleSource"),
        Builtin::Array => Some("Array"),
        Builtin::Iterator => Some("Iterator"),
        Builtin::ArrayBuffer => Some("ArrayBuffer"),
        Builtin::Float64Array => Some("Float64Array"),
        Builtin::Float32Array => Some("Float32Array"),
        Builtin::Int8Array => Some("Int8Array"),
        Builtin::Int16Array => Some("Int16Array"),
        Builtin::Int32Array => Some("Int32Array"),
        Builtin::Uint8Array => Some("Uint8Array"),
        Builtin::Uint16Array => Some("Uint16Array"),
        Builtin::Uint32Array => Some("Uint32Array"),
        Builtin::Uint8ClampedArray => Some("Uint8ClampedArray"),
        Builtin::BigInt64Array => Some("BigInt64Array"),
        Builtin::BigUint64Array => Some("BigUint64Array"),
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
    Some(match builtin {
        Builtin::Intl => "Intl",
        Builtin::IntlCollator => "Intl.Collator",
        Builtin::IntlDateTimeFormat => "Intl.DateTimeFormat",
        Builtin::IntlDisplayNames => "Intl.DisplayNames",
        Builtin::IntlDurationFormat => "Intl.DurationFormat",
        Builtin::IntlListFormat => "Intl.ListFormat",
        Builtin::IntlLocale => "Intl.Locale",
        Builtin::IntlNumberFormat => "Intl.NumberFormat",
        Builtin::IntlPluralRules => "Intl.PluralRules",
        Builtin::IntlRelativeTimeFormat => "Intl.RelativeTimeFormat",
        Builtin::IntlSegmenter => "Intl.Segmenter",
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
        Builtin::AbstractModuleSource => Some(Builtin::AbstractModuleSourcePrototype),
        Builtin::Array => Some(Builtin::ArrayPrototype),
        Builtin::Boolean => Some(Builtin::ObjectPrototype),
        Builtin::Promise => Some(Builtin::PromisePrototype),
        Builtin::Date => Some(Builtin::DatePrototype),
        Builtin::DisposableStack => Some(Builtin::DisposableStackPrototype),
        Builtin::AsyncDisposableStack => Some(Builtin::AsyncDisposableStackPrototype),
        Builtin::FinalizationRegistry => Some(Builtin::FinalizationRegistryPrototype),
        Builtin::Function => Some(Builtin::FunctionPrototype),
        Builtin::AsyncFunction => Some(Builtin::AsyncFunctionPrototype),
        Builtin::GeneratorFunction => Some(Builtin::GeneratorFunctionPrototype),
        Builtin::AsyncGeneratorFunction => Some(Builtin::AsyncGeneratorFunctionPrototype),
        Builtin::Number => Some(Builtin::ObjectPrototype),
        Builtin::Object => Some(Builtin::ObjectPrototype),
        Builtin::RegExp => Some(Builtin::RegExpPrototype),
        Builtin::String => Some(Builtin::ObjectPrototype),
        Builtin::Symbol => Some(Builtin::ObjectPrototype),
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
        Builtin::RangeError => Some(Builtin::RangeErrorPrototype),
        Builtin::ReferenceError => Some(Builtin::ReferenceErrorPrototype),
        Builtin::SyntaxError => Some(Builtin::SyntaxErrorPrototype),
        Builtin::EvalError => Some(Builtin::EvalErrorPrototype),
        Builtin::URIError => Some(Builtin::URIErrorPrototype),
        Builtin::AggregateError => Some(Builtin::AggregateErrorPrototype),
        Builtin::SuppressedError => Some(Builtin::SuppressedErrorPrototype),
        Builtin::TypeError => Some(Builtin::TypeErrorPrototype),
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
                | Builtin::ErrorPrototype
                | Builtin::RangeErrorPrototype
                | Builtin::ReferenceErrorPrototype
                | Builtin::SyntaxErrorPrototype
                | Builtin::EvalErrorPrototype
                | Builtin::URIErrorPrototype
                | Builtin::AggregateErrorPrototype
                | Builtin::TypeErrorPrototype
                | Builtin::SuppressedErrorPrototype
                | Builtin::PromisePrototype
        )
}

fn is_runtime_prototype(builtin: Builtin) -> bool {
    matches!(
        builtin,
        Builtin::IteratorPrototype
            | Builtin::AbstractModuleSourcePrototype
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
            | Builtin::GeneratorPrototype
            | Builtin::AsyncGeneratorPrototype
            | Builtin::DatePrototype
            | Builtin::DisposableStackPrototype
            | Builtin::AsyncDisposableStackPrototype
            | Builtin::RegExpPrototype
            | Builtin::RegExpStringIteratorPrototype
            | Builtin::SetIteratorPrototype
            | Builtin::MapIteratorPrototype
            | Builtin::ObjectPrototype
            | Builtin::ArrayIteratorPrototype
            | Builtin::NumberPrototype
            | Builtin::BooleanPrototype
            | Builtin::SymbolPrototype
            | Builtin::StringPrototype
            | Builtin::BigIntPrototype
            | Builtin::ErrorPrototype
            | Builtin::EvalErrorPrototype
            | Builtin::RangeErrorPrototype
            | Builtin::ReferenceErrorPrototype
            | Builtin::SyntaxErrorPrototype
            | Builtin::TypeErrorPrototype
            | Builtin::URIErrorPrototype
            | Builtin::AggregateErrorPrototype
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
    )
}

pub fn instance_prototype(builtin: Builtin) -> Option<Builtin> {
    match builtin {
        Builtin::Boolean => Some(Builtin::BooleanPrototype),
        Builtin::Number => Some(Builtin::NumberPrototype),
        Builtin::String => Some(Builtin::StringPrototype),
        Builtin::Symbol => Some(Builtin::SymbolPrototype),
        Builtin::BigInt => Some(Builtin::BigIntPrototype),
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
        Builtin::AbstractModuleSource => Some(0.0),
        Builtin::Array => Some(1.0),
        Builtin::Iterator => Some(0.0),
        Builtin::ArrayBuffer => Some(1.0),
        Builtin::ArrayBufferIsView => Some(1.0),
        Builtin::Boolean => Some(1.0),
        Builtin::BigInt => Some(1.0),
        Builtin::DataView => Some(1.0),
        Builtin::Proxy => Some(2.0),
        Builtin::Promise => Some(1.0),
        Builtin::Date => Some(7.0),
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
        Builtin::TypeError
        | Builtin::Error
        | Builtin::RangeError
        | Builtin::ReferenceError
        | Builtin::SyntaxError
        | Builtin::EvalError
        | Builtin::URIError => Some(1.0),
        Builtin::AggregateError => Some(2.0),
        Builtin::SuppressedError => Some(3.0),
        _ => None,
    }
}

fn intl_constructor_length(builtin: Builtin) -> Option<f64> {
    Some(match builtin {
        Builtin::Intl
        | Builtin::IntlCollator
        | Builtin::IntlPluralRules
        | Builtin::IntlDateTimeFormat
        | Builtin::IntlListFormat => 0.0,
        Builtin::IntlDisplayNames
        | Builtin::IntlNumberFormat
        | Builtin::IntlRelativeTimeFormat
        | Builtin::IntlSegmenter => 0.0,
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
