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
        "Locale" => Builtin::IntlLocale,
        "getCanonicalLocales" => Builtin::IntlGetCanonicalLocales,
        "supportedValuesOf" => Builtin::IntlSupportedValuesOf,
        _ => return None,
    })
}

fn constructor_property(builtin: Builtin, key: &str) -> Option<Builtin> {
    if key == "supportedLocalesOf" {
        return match builtin {
            Builtin::IntlDateTimeFormat => Some(Builtin::IntlDateTimeFormatSupportedLocalesOf),
            Builtin::IntlSegmenter => Some(Builtin::IntlSegmenterSupportedLocalesOf),
            Builtin::IntlListFormat => Some(Builtin::IntlListFormatSupportedLocalesOf),
            Builtin::IntlRelativeTimeFormat => {
                Some(Builtin::IntlRelativeTimeFormatSupportedLocalesOf)
            }
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
        Builtin::IntlDateTimeFormatSupportedLocalesOf => Some(supported_locales_of(arguments)),
        Builtin::IntlSegmenterSupportedLocalesOf => Some(segmenter_supported_locales_of(arguments)),
        Builtin::IntlListFormatSupportedLocalesOf => Some(list_supported_locales_of(arguments)),
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

fn supported_segmenter_locale(locale: &str) -> bool {
    ["de", "en", "sr", "zh"]
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
    let Some(Value::Object(properties)) = options else {
        if matches!(options, Some(Value::Null)) {
            return Err(crate::value::error::throw_type_error(
                "Cannot convert null to object",
            ));
        }
        return Ok(());
    };
    if let Some((_, value)) = properties.iter().find(|(name, _)| name == "localeMatcher") {
        let matcher = to_string_value(value);
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
    const HANDLERS: [Handler; 9] = [
        locale::dispatch,
        number::dispatch,
        plural::dispatch,
        datetime::dispatch,
        collator::dispatch,
        list::dispatch,
        relative::dispatch,
        segmenter::dispatch,
        displaynames::dispatch,
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
    let Some(Value::String(key)) = arguments.first() else {
        return Err(runtime_error("TypeError: key must be a string"));
    };
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
fn resolve_locales(arguments: &[Value]) -> Result<Vec<String>, VmError> {
    let Some(locales) = arguments.first() else {
        return Ok(vec![default_locale()]);
    };
    match locales {
        Value::String(_) => Ok(vec![canonicalize(&crate::conversion::to_string(locales)?)?]),
        Value::Array(_) | Value::Object(_) => resolve_locale_list(locales),
        Value::Null => Err(runtime_error("TypeError: invalid locales")),
        _ => Ok(vec![default_locale()]),
    }
}

fn resolve_locale_list(locales: &Value) -> Result<Vec<String>, VmError> {
    let length = crate::execute::get_property_result(locales, "length")?;
    let length = locale_list_length(&length)?;
    let mut out = Vec::new();
    for index in 0..length {
        let value = crate::execute::get_property_result(locales, &index.to_string())?;
        out.push(canonicalize(&crate::conversion::to_string(&value)?)?);
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

fn dedupe(locales: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    locales
        .into_iter()
        .filter(|tag| seen.insert(tag.clone()))
        .collect()
}

pub(crate) fn to_string_value(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        Value::Boolean(value) => value.to_string(),
        Value::Null => "null".to_string(),
        Value::Undefined => "undefined".to_string(),
        Value::Array(values) => values
            .iter()
            .map(to_string_value)
            .collect::<Vec<_>>()
            .join(","),
        _ => "[object Object]".to_string(),
    }
}

/// Canonicalize a single BCP-47 language tag.
pub(crate) fn canonicalize(tag: &str) -> Result<String, VmError> {
    let tag = tag.trim();
    match tag.to_ascii_lowercase().as_str() {
        "cel-gaulish" => return Ok("xtg".to_string()),
        "zh-min" | "i-default" => {
            return Err(runtime_error("RangeError: invalid language tag"));
        }
        _ => {}
    }
    if tag.is_empty()
        || tag.eq_ignore_ascii_case("nan")
        || !tag.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err(runtime_error("RangeError: invalid language tag"));
    }
    let mut parts = tag.split('-');
    let language = parts
        .next()
        .ok_or_else(|| runtime_error("RangeError: invalid language tag"))?;
    if language.is_empty()
        || language.len() < 2
        || language.len() > 8
        || !language.chars().all(|c| c.is_ascii_alphabetic())
    {
        return Err(runtime_error("RangeError: invalid language tag"));
    }
    let mut out = Vec::new();
    let mut script_done = false;
    if language.eq_ignore_ascii_case("sh") {
        out.push("sr".to_string());
        out.push("Latn".to_string());
        script_done = true;
    } else {
        out.push(language_alias(language.to_ascii_lowercase()));
    }
    Ok(canonicalize_subtags(parts.collect(), out, script_done)?.join("-"))
}

fn canonicalize_subtags(
    parts: Vec<&str>,
    mut out: Vec<String>,
    mut script_done: bool,
) -> Result<Vec<String>, VmError> {
    let mut region_done = false;
    let mut variant_done = false;
    let mut extension = false;
    for part in parts {
        if part.is_empty() {
            return Err(runtime_error("RangeError: invalid language tag"));
        }
        if extension {
            out.push(part.to_ascii_lowercase());
            continue;
        }
        if part.len() == 1 {
            out.push(part.to_ascii_lowercase());
            extension = true;
            continue;
        }
        match classify_subtag(part, script_done, region_done, variant_done) {
            Subtag::Script => {
                out.push(titlecase_script(part));
                script_done = true;
            }
            Subtag::Region => {
                out.push(part.to_ascii_uppercase());
                region_done = true;
            }
            Subtag::Variant => {
                out.push(part.to_ascii_lowercase());
                variant_done = true;
            }
            Subtag::Extension => out.push(part.to_ascii_lowercase()),
        }
    }
    Ok(out)
}

fn titlecase_script(part: &str) -> String {
    let mut chars = part.chars();
    let first = chars.next().map_or(String::new(), |value| {
        value.to_ascii_uppercase().to_string()
    });
    format!("{first}{}", chars.as_str().to_ascii_lowercase())
}

enum Subtag {
    Script,
    Region,
    Variant,
    Extension,
}

fn classify_subtag(part: &str, script_done: bool, region_done: bool, variant_done: bool) -> Subtag {
    let all_alpha = part.chars().all(|c| c.is_ascii_alphabetic());
    let all_digit = part.chars().all(|c| c.is_ascii_digit());
    if !script_done && part.len() == 4 && all_alpha {
        Subtag::Script
    } else if !region_done && ((part.len() == 2 && all_alpha) || (part.len() == 3 && all_digit)) {
        Subtag::Region
    } else if !variant_done
        && ((part.len() >= 4 && all_alpha)
            || (part.len() >= 5
                && part.chars().next().is_some_and(|c| c.is_ascii_digit())
                && part[1..].chars().all(|c| c.is_ascii_alphanumeric())))
    {
        Subtag::Variant
    } else {
        Subtag::Extension
    }
}

fn language_alias(language: String) -> String {
    match language.as_str() {
        "iw" => "he".to_string(),
        "in" => "id".to_string(),
        "ji" => "yi".to_string(),
        "tl" => "fil".to_string(),
        "mo" => "ro".to_string(),
        other => other.to_string(),
    }
}
