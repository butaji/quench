//! ECMA-402 (Intl) semantic owner.
//!
//! This module owns the `Intl` global object and every `Intl.*` constructor and
//! prototype method. Constructed Intl objects are ordinary `Value::Object`s; a
//! hidden `__intl` property carries the internal slots. Prototype methods read
//! that slot through the call receiver.

use crate::{execute::VmError, ops::Builtin, value::Value};

pub(crate) mod collator;
pub(crate) mod datetime;
pub(crate) mod displaynames;
pub(crate) mod duration;
pub(crate) mod list;
pub(crate) mod locale;
pub(crate) mod number;
pub(crate) mod number_format;
pub(crate) mod plural;
pub(crate) mod relative;
pub(crate) mod segmenter;
mod support;
mod supported_values;
pub(crate) mod tolocale;

pub(crate) use support::{
    intl_slots, make_array, make_object, runtime_error, slot_bool, slot_number, slot_string,
};
pub(crate) use supported_values::{
    supported_calendars, supported_collations, supported_currencies, supported_numbering_systems,
    supported_time_zones, supported_units,
};

/// Internal slot key stored on constructed Intl objects.
pub(crate) const SLOT: &str = "__intl";

/// Resolve a property on an `Intl`-related builtin.
pub(crate) fn property(builtin: Builtin, key: &str) -> Option<Value> {
    let value = global_property(builtin, key)
        .or_else(|| constructor_property(builtin, key))
        .or_else(|| prototype_property(builtin, key))?;
    Some(Value::Builtin(value))
}

fn global_property(builtin: Builtin, key: &str) -> Option<Builtin> {
    if builtin != Builtin::Intl {
        return None;
    }
    Some(match key {
        "NumberFormat" => Builtin::IntlNumberFormat,
        "DateTimeFormat" => Builtin::IntlDateTimeFormat,
        "Collator" => Builtin::IntlCollator,
        "PluralRules" => Builtin::IntlPluralRules,
        "ListFormat" => Builtin::IntlListFormat,
        "RelativeTimeFormat" => Builtin::IntlRelativeTimeFormat,
        "Segmenter" => Builtin::IntlSegmenter,
        "DisplayNames" => Builtin::IntlDisplayNames,
        "DurationFormat" => Builtin::IntlDurationFormat,
        "Locale" => Builtin::IntlLocale,
        "getCanonicalLocales" => Builtin::IntlGetCanonicalLocales,
        "supportedValuesOf" => Builtin::IntlSupportedValuesOf,
        _ => return None,
    })
}

fn constructor_property(builtin: Builtin, key: &str) -> Option<Builtin> {
    if key == "supportedLocalesOf" {
        return match builtin {
            Builtin::IntlNumberFormat => Some(Builtin::IntlNumberFormatSupportedLocalesOf),
            Builtin::IntlDateTimeFormat => Some(Builtin::IntlDateTimeFormatSupportedLocalesOf),
            Builtin::IntlCollator => Some(Builtin::IntlCollatorSupportedLocalesOf),
            Builtin::IntlPluralRules => Some(Builtin::IntlPluralRulesSupportedLocalesOf),
            Builtin::IntlSegmenter => Some(Builtin::IntlSegmenterSupportedLocalesOf),
            Builtin::IntlListFormat => Some(Builtin::IntlListFormatSupportedLocalesOf),
            Builtin::IntlRelativeTimeFormat => {
                Some(Builtin::IntlRelativeTimeFormatSupportedLocalesOf)
            }
            Builtin::IntlDurationFormat => Some(Builtin::IntlDurationFormatSupportedLocalesOf),
            _ => None,
        };
    }
    if key != "prototype" {
        return None;
    }
    Some(match builtin {
        Builtin::IntlLocale => Builtin::IntlLocalePrototype,
        Builtin::IntlNumberFormat => Builtin::IntlNumberFormatPrototype,
        Builtin::IntlPluralRules => Builtin::IntlPluralRulesPrototype,
        Builtin::IntlDateTimeFormat => Builtin::IntlDateTimeFormatPrototype,
        Builtin::IntlCollator => Builtin::IntlCollatorPrototype,
        Builtin::IntlListFormat => Builtin::IntlListFormatPrototype,
        Builtin::IntlRelativeTimeFormat => Builtin::IntlRelativeTimeFormatPrototype,
        Builtin::IntlSegmenter => Builtin::IntlSegmenterPrototype,
        Builtin::IntlDisplayNames => Builtin::IntlDisplayNamesPrototype,
        Builtin::IntlDurationFormat => Builtin::IntlDurationFormatPrototype,
        _ => return None,
    })
}

include!("prototype_property.rs");
/// Dispatch a builtin call received with a receiver.
pub(crate) fn execute(
    builtin: Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Option<Result<Value, VmError>> {
    match builtin {
        Builtin::IntlNumberFormatSupportedLocalesOf => Some(supported_locales_of(arguments)),
        Builtin::IntlDateTimeFormatSupportedLocalesOf => Some(supported_locales_of(arguments)),
        Builtin::IntlCollatorSupportedLocalesOf => Some(supported_locales_of(arguments)),
        Builtin::IntlPluralRulesSupportedLocalesOf => Some(supported_locales_of(arguments)),
        Builtin::IntlSegmenterSupportedLocalesOf => Some(segmenter_supported_locales_of(arguments)),
        Builtin::IntlListFormatSupportedLocalesOf => Some(list_supported_locales_of(arguments)),
        Builtin::IntlDurationFormatSupportedLocalesOf => Some(supported_locales_of(arguments)),
        Builtin::IntlGetCanonicalLocales => Some(get_canonical_locales(arguments)),
        Builtin::IntlSupportedValuesOf => Some(supported_values_of(arguments)),
        _ => dispatch_all(builtin, arguments, receiver),
    }
}

fn supported_locales_of(arguments: &[Value]) -> Result<Value, VmError> {
    let locales = requested_locales(arguments)?;
    validate_supported_options(arguments.get(1))?;
    Ok(make_array(
        locales
            .into_iter()
            .filter(|locale| locale == "en" || locale.starts_with("en-"))
            .map(Value::String)
            .collect(),
    ))
}

fn segmenter_supported_locales_of(arguments: &[Value]) -> Result<Value, VmError> {
    let locales = requested_locales(arguments)?;
    validate_supported_options(arguments.get(1))?;
    Ok(make_array(
        locales
            .into_iter()
            .filter(|locale| supported_segmenter_locale(locale))
            .map(Value::String)
            .collect(),
    ))
}

fn list_supported_locales_of(arguments: &[Value]) -> Result<Value, VmError> {
    let locales = requested_locales(arguments)?;
    validate_supported_options(arguments.get(1))?;
    Ok(make_array(
        locales
            .into_iter()
            .filter(|locale| locale == "en" || locale.starts_with("en-"))
            .map(Value::String)
            .collect(),
    ))
}

pub(crate) fn supported_segmenter_locale(locale: &str) -> bool {
    ["ar", "de", "en", "fr", "sr", "zh"]
        .iter()
        .any(|language| locale == *language || locale.starts_with(&format!("{language}-")))
}

fn requested_locales(arguments: &[Value]) -> Result<Vec<String>, VmError> {
    if matches!(arguments.first(), Some(Value::Null)) {
        return Err(crate::value::error::throw_type_error(
            "Cannot convert null to object",
        ));
    }
    if arguments.is_empty() {
        return Ok(Vec::new());
    }
    resolve_locales(arguments)
}

fn validate_supported_options(options: Option<&Value>) -> Result<(), VmError> {
    let Some(options) = options else {
        return Ok(());
    };
    if matches!(options, Value::Undefined) {
        return Ok(());
    }
    if matches!(options, Value::Null) {
        return Err(crate::value::error::throw_type_error(
            "Cannot convert null to object",
        ));
    }
    let options = crate::construct::to_object(options)?;
    let value = crate::execute::get_property_result(&options, "localeMatcher")?;
    if !matches!(value, Value::Undefined) {
        let matcher = crate::conversion::to_string(&value)?;
        if matcher != "lookup" && matcher != "best fit" {
            return Err(runtime_error("RangeError: invalid localeMatcher"));
        }
    }
    Ok(())
}

type Handler = fn(Builtin, &[Value], Option<&Value>) -> Option<Result<Value, VmError>>;

fn dispatch_all(
    builtin: Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Option<Result<Value, VmError>> {
    const HANDLERS: [Handler; 10] = [
        locale::dispatch,
        number::dispatch,
        plural::dispatch,
        datetime::dispatch,
        collator::dispatch,
        list::dispatch,
        relative::dispatch,
        segmenter::dispatch,
        displaynames::dispatch,
        duration::dispatch,
    ];
    for handler in HANDLERS {
        if let Some(result) = handler(builtin, arguments, receiver) {
            return Some(result);
        }
    }
    None
}

/// Implement `Intl.getCanonicalLocales`.
fn get_canonical_locales(arguments: &[Value]) -> Result<Value, VmError> {
    let locales = resolve_locales(arguments)?;
    Ok(make_array(locales.into_iter().map(Value::String).collect()))
}

/// Implement `Intl.supportedValuesOf`.
fn supported_values_of(arguments: &[Value]) -> Result<Value, VmError> {
    let key =
        crate::conversion::to_string(arguments.first().map_or(&Value::Undefined, |value| value))?;
    let values: Vec<Value> = match key.as_str() {
        "calendar" => supported_calendars(),
        "collation" => supported_collations(),
        "currency" => supported_currencies(),
        "numberingSystem" => supported_numbering_systems(),
        "timeZone" => supported_time_zones(),
        "unit" => supported_units(),
        _ => return Err(runtime_error("RangeError: unknown key")),
    };
    Ok(make_array(values))
}

/// Resolve the `locales` argument to a canonical list of BCP-47 tags.
pub(crate) fn resolve_locales(arguments: &[Value]) -> Result<Vec<String>, VmError> {
    let Some(locales) = arguments.first() else {
        return Ok(vec![default_locale()]);
    };
    match locales {
        Value::String(value) if crate::conversion::is_symbol_string(value) => {
            resolve_locale_list(&crate::construct::to_object(locales)?)
        }
        Value::String(_) => Ok(vec![canonicalize(&crate::conversion::to_string(locales)?)?]),
        locales if crate::value::is_object(locales) => resolve_locale_list(locales),
        Value::Null => Err(runtime_error("TypeError: invalid locales")),
        Value::Undefined => Ok(vec![default_locale()]),
        _ => resolve_locale_list(&crate::construct::to_object(locales)?),
    }
}

fn resolve_locale_list(locales: &Value) -> Result<Vec<String>, VmError> {
    let length = crate::execute::get_property_result(locales, "length")?;
    let length = locale_list_length(&length)?;
    let mut out = Vec::new();
    for index in 0..length {
        if !crate::with_scope::has_property(locales, &index.to_string())? {
            continue;
        }
        let value = crate::execute::get_property_result(locales, &index.to_string())?;
        let value = match value {
            Value::String(_) | Value::StringUnits(_) => crate::conversion::to_string(&value)?,
            value if crate::value::is_object(&value) => crate::conversion::to_string(&value)?,
            _ => return Err(crate::value::error::throw_type_error("invalid locale")),
        };
        out.push(canonicalize(&value)?);
    }
    Ok(dedupe(out))
}

fn locale_list_length(value: &Value) -> Result<usize, VmError> {
    let number = crate::conversion::to_number(value)?;
    if number.is_nan() || number <= 0.0 {
        return Ok(0);
    }
    Ok(number.floor().min(9_007_199_254_740_991.0) as usize)
}

pub(crate) fn default_locale() -> String {
    "en".to_string()
}

pub(crate) fn number_locale(locale: &str) -> String {
    let Some((base, extension)) = locale.split_once("-u-") else {
        return locale.to_string();
    };
    let parts: Vec<&str> = extension.split('-').collect();
    let mut index = 0;
    while index < parts.len() {
        let key = parts[index];
        index += 1;
        let start = index;
        while index < parts.len() && parts[index].len() != 2 {
            index += 1;
        }
        if key == "nu"
            && start < index
            && supported_values::NUMBERING_SYSTEMS.contains(&parts[start])
        {
            return format!("{base}-u-nu-{}", parts[start]);
        }
    }
    base.to_string()
}

pub(crate) fn numbering_system(locale: &str) -> Option<&str> {
    let (_, extension) = locale.split_once("-u-")?;
    let parts: Vec<&str> = extension.split('-').collect();
    let index = parts.iter().position(|part| *part == "nu")? + 1;
    let value = parts.get(index).copied()?;
    supported_values::NUMBERING_SYSTEMS
        .contains(&value)
        .then_some(value)
}

include!("locale_canonicalization.rs");
