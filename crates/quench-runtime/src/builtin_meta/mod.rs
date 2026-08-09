//! Declarative builtin/primordial metadata.
//!
//! Provides static metadata about JavaScript builtins: constructor names,
//! prototypes, function lengths, and short names.

#![allow(dead_code)]

use crate::ops::Builtin;

pub mod array;
pub mod bigint;
pub mod collections;
pub mod date;
pub mod function;
pub mod intl;
pub mod math;
pub mod methods;
pub mod number;
pub mod object;
pub mod reflect;
pub mod regexp;
pub mod string;
pub mod symbol;

/// Returns the constructor name for a builtin.
///
/// Returns `None` for builtins that are not constructors.
pub fn constructor_name(builtin: Builtin) -> Option<&'static str> {
    match builtin {
        Builtin::Array => Some("Array"),
        Builtin::Boolean => Some("Boolean"),
        Builtin::Date => Some("Date"),
        Builtin::Function => Some("Function"),
        Builtin::Intl => Some("Intl"),
        Builtin::IntlCollator => Some("Intl.Collator"),
        Builtin::IntlDateTimeFormat => Some("Intl.DateTimeFormat"),
        Builtin::IntlDisplayNames => Some("Intl.DisplayNames"),
        Builtin::IntlListFormat => Some("Intl.ListFormat"),
        Builtin::IntlLocale => Some("Intl.Locale"),
        Builtin::IntlNumberFormat => Some("Intl.NumberFormat"),
        Builtin::IntlPluralRules => Some("Intl.PluralRules"),
        Builtin::IntlRelativeTimeFormat => Some("Intl.RelativeTimeFormat"),
        Builtin::IntlSegmenter => Some("Intl.Segmenter"),
        Builtin::Math => Some("Math"),
        Builtin::Number => Some("Number"),
        Builtin::Object => Some("Object"),
        Builtin::RegExp => Some("RegExp"),
        Builtin::String => Some("String"),
        Builtin::Symbol => Some("Symbol"),
        Builtin::TypeError => Some("TypeError"),
        Builtin::Map => Some("Map"),
        Builtin::Set => Some("Set"),
        _ => None,
    }
}

/// Returns the prototype builtin for a constructor builtin.
///
/// Returns `None` for builtins that are not constructors with prototypes.
pub fn prototype(builtin: Builtin) -> Option<Builtin> {
    match builtin {
        Builtin::Array => Some(Builtin::ArrayPrototype),
        Builtin::Boolean => Some(Builtin::ObjectPrototype),
        Builtin::Date => Some(Builtin::DatePrototype),
        Builtin::Function => Some(Builtin::FunctionPrototype),
        Builtin::Number => Some(Builtin::ObjectPrototype),
        Builtin::Object => Some(Builtin::ObjectPrototype),
        Builtin::RegExp => Some(Builtin::RegExpPrototype),
        Builtin::String => Some(Builtin::ObjectPrototype),
        Builtin::Symbol => Some(Builtin::ObjectPrototype),
        Builtin::IntlCollator => Some(Builtin::IntlCollatorPrototype),
        Builtin::IntlDateTimeFormat => Some(Builtin::IntlDateTimeFormatPrototype),
        Builtin::IntlDisplayNames => Some(Builtin::IntlDisplayNamesPrototype),
        Builtin::IntlListFormat => Some(Builtin::IntlListFormatPrototype),
        Builtin::IntlLocale => Some(Builtin::IntlLocalePrototype),
        Builtin::IntlNumberFormat => Some(Builtin::IntlNumberFormatPrototype),
        Builtin::IntlPluralRules => Some(Builtin::IntlPluralRulesPrototype),
        Builtin::IntlRelativeTimeFormat => Some(Builtin::IntlRelativeTimeFormatPrototype),
        Builtin::IntlSegmenter => Some(Builtin::IntlSegmenterPrototype),
        Builtin::Map => Some(Builtin::MapPrototype),
        Builtin::Set => Some(Builtin::SetPrototype),
        _ => None,
    }
}

/// Returns the `length` property value for a builtin constructor.
///
/// Returns `None` for builtins that are not constructors or have no defined length.
pub fn constructor_length(builtin: Builtin) -> Option<f64> {
    match builtin {
        Builtin::Array => Some(1.0),
        Builtin::Boolean => Some(1.0),
        Builtin::Date => Some(7.0),
        Builtin::Function => Some(1.0),
        Builtin::Intl => Some(0.0),
        Builtin::IntlCollator => Some(0.0),
        Builtin::IntlDateTimeFormat => Some(2.0),
        Builtin::IntlDisplayNames => Some(2.0),
        Builtin::IntlListFormat => Some(2.0),
        Builtin::IntlLocale => Some(1.0),
        Builtin::IntlNumberFormat => Some(2.0),
        Builtin::IntlPluralRules => Some(0.0),
        Builtin::IntlRelativeTimeFormat => Some(2.0),
        Builtin::IntlSegmenter => Some(2.0),
        Builtin::Math => None,
        Builtin::Number => Some(1.0),
        Builtin::Object => Some(0.0),
        Builtin::RegExp => Some(2.0),
        Builtin::String => Some(1.0),
        Builtin::Symbol => Some(0.0),
        Builtin::TypeError => Some(1.0),
        Builtin::Map => Some(0.0),
        Builtin::Set => Some(0.0),
        _ => None,
    }
}
