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
        "art-lojban" => return Ok("jbo".to_string()),
        "cel-gaulish" => return Ok("xtg".to_string()),
        "zh-guoyu" => return Ok("zh".to_string()),
        "zh-hakka" => return Ok("hak".to_string()),
        "zh-xiang" => return Ok("hsn".to_string()),
        "en-gb-oed" | "zh-min" | "i-default" => {
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
    let mut parts: Vec<&str> = parts.collect();
    let mut out = Vec::new();
    let mut script_done = false;
    if language.eq_ignore_ascii_case("sh") {
        out.push("sr".to_string());
        if !parts.first().is_some_and(|part| {
            part.len() == 4
                && part
                    .chars()
                    .all(|character| character.is_ascii_alphabetic())
        }) {
            out.push("Latn".to_string());
            script_done = true;
        }
    } else if language.eq_ignore_ascii_case("cnr") {
        out.push("sr".to_string());
        if !parts.first().is_some_and(|part| {
            (part.len() == 2 && part.chars().all(|c| c.is_ascii_alphabetic()))
                || (part.len() == 3 && part.chars().all(|c| c.is_ascii_digit()))
        }) {
            out.push("ME".to_string());
        }
    } else {
        out.push(language_alias(language.to_ascii_lowercase()));
    }
    apply_armenian_variant_alias(&mut out, &mut parts);
    validate_transformed_extensions(&parts)?;
    Ok(canonicalize_subtags(parts, out, script_done)?.join("-"))
}

fn apply_armenian_variant_alias(out: &mut Vec<String>, parts: &mut Vec<&str>) {
    if out.first().map(String::as_str) != Some("hy") {
        return;
    }
    match parts.first().copied() {
        Some("arevela") => {
            let _ = parts.remove(0);
        }
        Some("arevmda") => {
            out[0] = "hyw".to_string();
            let _ = parts.remove(0);
        }
        _ => {}
    }
}

fn validate_transformed_extensions(parts: &[&str]) -> Result<(), VmError> {
    for (index, part) in parts.iter().enumerate() {
        if part.eq_ignore_ascii_case("x") {
            break;
        }
        if part.eq_ignore_ascii_case("t") {
            let end = parts[index + 1..]
                .iter()
                .position(|part| part.len() == 1)
                .map_or(parts.len(), |offset| index + 1 + offset);
            if end < parts.len() && end + 1 == parts.len() {
                return Err(runtime_error("RangeError: invalid language tag"));
            }
            validate_transformed_fields(&parts[index + 1..end])?;
        }
    }
    Ok(())
}

fn validate_transformed_fields(parts: &[&str]) -> Result<(), VmError> {
    if parts.is_empty() {
        return Err(runtime_error("RangeError: invalid language tag"));
    }
    let mut index = if is_transformed_language(parts[0]) {
        transformed_language_length(parts)?
    } else {
        0
    };
    while index < parts.len() {
        let key = parts[index];
        if key.len() != 2 || !key.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(runtime_error("RangeError: invalid language tag"));
        }
        index += 1;
        let start = index;
        while index < parts.len() && parts[index].len() != 2 {
            if !(3..=8).contains(&parts[index].len())
                || !parts[index].chars().all(|c| c.is_ascii_alphanumeric())
            {
                return Err(runtime_error("RangeError: invalid language tag"));
            }
            index += 1;
        }
        if start == index {
            return Err(runtime_error("RangeError: invalid language tag"));
        }
    }
    Ok(())
}

fn is_transformed_language(part: &str) -> bool {
    (2..=3).contains(&part.len()) && part.chars().all(|c| c.is_ascii_alphabetic())
        || (5..=8).contains(&part.len()) && part.chars().all(|c| c.is_ascii_alphanumeric())
}

fn transformed_language_length(parts: &[&str]) -> Result<usize, VmError> {
    let mut index = 1;
    if parts
        .get(index)
        .is_some_and(|part| part.len() == 4 && part.chars().all(|c| c.is_ascii_alphabetic()))
    {
        index += 1;
    }
    if parts.get(index).is_some_and(|part| {
        (part.len() == 2 && part.chars().all(|c| c.is_ascii_alphabetic()))
            || (part.len() == 3 && part.chars().all(|c| c.is_ascii_digit()))
    }) {
        index += 1;
    }
    while parts.get(index).is_some_and(|part| {
        (5..=8).contains(&part.len()) && part.chars().all(|c| c.is_ascii_alphanumeric())
            || part.len() == 4
                && part.chars().next().is_some_and(|c| c.is_ascii_digit())
                && part.chars().skip(1).all(|c| c.is_ascii_alphanumeric())
    }) {
        index += 1;
    }
    Ok(index)
}

fn canonicalize_unicode_aliases(parts: &[&str]) -> Vec<String> {
    let mut result = Vec::new();
    let mut index = 0;
    while index < parts.len() {
        result.push(parts[index].to_ascii_lowercase());
        if parts[index].eq_ignore_ascii_case("u") {
            index += 1;
            append_unicode_extension(parts, &mut index, &mut result);
        } else {
            index += 1;
        }
    }
    result
}

fn append_unicode_extension(parts: &[&str], index: &mut usize, result: &mut Vec<String>) {
    while *index < parts.len() && parts[*index].len() != 1 {
        let key = parts[*index].to_ascii_lowercase();
        result.push(key.clone());
        *index += 1;
        if key.len() != 2 {
            continue;
        }
        let start = *index;
        while *index < parts.len() && parts[*index].len() != 2 && parts[*index].len() != 1 {
            *index += 1;
        }
        let values = &parts[start..*index];
        if is_true_alias(&key, values) {
            continue;
        }
        if let Some(alias) = unicode_alias(&key, values) {
            result.push(alias.to_string());
        } else {
            result.extend(values.iter().map(|value| value.to_ascii_lowercase()));
        }
    }
}

fn is_true_alias(key: &str, values: &[&str]) -> bool {
    matches!(key, "kb" | "kc" | "kh" | "kk" | "kn") && values == ["yes"]
}

fn unicode_alias(key: &str, values: &[&str]) -> Option<&'static str> {
    match (key, values) {
        ("ca", ["ethiopic", "amete", "alem"]) => Some("ethioaa"),
        ("ca", ["islamicc"]) => Some("islamic-civil"),
        ("ks", ["primary"]) => Some("level1"),
        ("ks", ["secondary"]) => Some("level2"),
        ("ks", ["tertiary"]) => Some("level3"),
        ("ks", ["quaternary" | "quarternary"]) => Some("level4"),
        ("ks", ["identical"]) => Some("identic"),
        ("ms", ["imperial"]) => Some("uksystem"),
        ("rg", ["no23"]) | ("sd", ["no23"]) => Some("no50"),
        ("rg", ["cn11"]) | ("sd", ["cn11"]) => Some("cnbj"),
        ("rg", ["cz10a"]) | ("sd", ["cz10a"]) => Some("cz110"),
        ("rg", ["fra"]) | ("sd", ["fra"]) => Some("frges"),
        ("rg", ["frg"]) | ("sd", ["frg"]) => Some("frges"),
        ("rg", ["lud"]) | ("sd", ["lud"]) => Some("lucl"),
        ("tz", ["cnckg"]) => Some("cnsha"),
        ("tz", ["eire"]) => Some("iedub"),
        ("tz", ["est"]) => Some("papty"),
        ("tz", ["gmt0"]) => Some("gmt"),
        ("tz", ["uct" | "zulu"]) => Some("utc"),
        _ => None,
    }
}

fn canonicalize_subtags(
    parts: Vec<&str>,
    mut out: Vec<String>,
    mut script_done: bool,
) -> Result<Vec<String>, VmError> {
    validate_unicode_extension_keys(&parts)?;
    let aliased = canonicalize_unicode_aliases(&parts);
    let variant_aliased = canonicalize_variant_aliases(&aliased);
    let parts: Vec<&str> = variant_aliased.iter().map(String::as_str).collect();
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
        if region_done && is_region_shape(part) {
            return Err(runtime_error("RangeError: invalid language tag"));
        }
        match classify_subtag(part, script_done, region_done, variant_done) {
            Subtag::Script => {
                out.push(titlecase_script(part));
                script_done = true;
            }
            Subtag::Region => {
                out.push(canonical_region(part, &out));
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

fn is_region_shape(part: &str) -> bool {
    let alphabetic = part.len() == 2 && part.chars().all(|c| c.is_ascii_alphabetic());
    let numeric = part.len() == 3 && part.chars().all(|c| c.is_ascii_digit());
    alphabetic || numeric
}

pub(crate) fn canonical_region(part: &str, emitted: &[String]) -> String {
    let region = part.to_ascii_uppercase();
    match region.as_str() {
        "CS" => "RS".to_string(),
        "NT" => "SA".to_string(),
        "554" => "NZ".to_string(),
        "SU" | "810"
            if emitted.first().is_some_and(|language| language == "hy")
                || emitted.iter().any(|subtag| subtag == "Armn") =>
        {
            "AM".to_string()
        }
        "SU" | "810" => "RU".to_string(),
        _ => region,
    }
}

fn validate_unicode_extension_keys(parts: &[&str]) -> Result<(), VmError> {
    for (index, part) in parts.iter().enumerate() {
        if !part.eq_ignore_ascii_case("u") {
            continue;
        }
        for value in &parts[index + 1..] {
            if value.len() == 1 {
                break;
            }
            if value.len() == 2
                && !value
                    .chars()
                    .nth(1)
                    .is_some_and(|character| character.is_ascii_alphabetic())
            {
                return Err(runtime_error("RangeError: invalid language tag"));
            }
        }
    }
    Ok(())
}

fn canonicalize_variant_aliases(parts: &[String]) -> Vec<String> {
    let mut result = Vec::new();
    let mut index = 0;
    let mut extension = false;
    while index < parts.len() {
        if parts[index].len() == 1 {
            extension = true;
        }
        if !extension
            && parts[index].eq_ignore_ascii_case("hepburn")
            && parts
                .get(index + 1)
                .is_some_and(|part| part.eq_ignore_ascii_case("heploc"))
        {
            result.push("alalc97".to_string());
            index += 2;
        } else {
            result.push(parts[index].to_ascii_lowercase());
            index += 1;
        }
    }
    result
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
        "aar" => "aa".to_string(),
        "ces" => "cs".to_string(),
        "heb" => "he".to_string(),
        "iw" => "he".to_string(),
        "in" => "id".to_string(),
        "ji" => "yi".to_string(),
        "tl" => "fil".to_string(),
        "mo" => "ro".to_string(),
        other => other.to_string(),
    }
}
