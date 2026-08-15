//! ECMA-402 (Intl) semantic owner.
//!
//! This module owns the `Intl` global object and every `Intl.*` constructor and
//! prototype method. Constructed Intl objects are ordinary `Value::Object`s; a
//! hidden `__intl` property carries the internal slots. Prototype methods read
//! that slot through the call receiver.

use crate::{execute::VmError, ops::Builtin, value::Value};

pub(crate) mod collator;
pub(crate) mod datetime;
mod digits;
pub(crate) mod displaynames;
pub(crate) mod duration;
pub(crate) mod list;
pub(crate) mod locale;
pub(crate) mod number;
pub(crate) mod number_format;
pub(crate) mod plural;
pub(crate) mod relative;
pub(crate) mod segmenter;
pub(crate) mod tolocale;

/// Internal slot key stored on constructed Intl objects.
pub(crate) const SLOT: &str = "__intl";

/// Resolve a property on an `Intl`-related builtin.
pub(crate) fn property(builtin: Builtin, key: &str) -> Option<Value> {
    let value = global_property(builtin, key)
        .or_else(|| constructor_property(builtin, key))
        .or_else(|| prototype_property(builtin, key))?;
    Some(Value::Builtin(value))
}

pub(crate) fn is_constructor(builtin: Builtin) -> bool {
    matches!(
        builtin,
        Builtin::IntlNumberFormat
            | Builtin::IntlDateTimeFormat
            | Builtin::IntlCollator
            | Builtin::IntlPluralRules
            | Builtin::IntlListFormat
            | Builtin::IntlRelativeTimeFormat
            | Builtin::IntlSegmenter
            | Builtin::IntlDisplayNames
            | Builtin::IntlLocale
    )
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
            Builtin::IntlSegmenter => Some(Builtin::IntlSegmenterSupportedLocalesOf),
            Builtin::IntlDurationFormat => Some(Builtin::IntlDurationFormatSupportedLocalesOf),
            Builtin::IntlListFormat => Some(Builtin::IntlListFormatSupportedLocalesOf),
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

fn prototype_property(builtin: Builtin, key: &str) -> Option<Builtin> {
    Some(match (builtin, key) {
        (Builtin::IntlListFormatPrototype, "constructor") => Builtin::IntlListFormat,
        (Builtin::IntlLocalePrototype, "constructor") => Builtin::IntlLocale,
        (Builtin::IntlNumberFormatPrototype, "constructor") => Builtin::IntlNumberFormat,
        (Builtin::IntlLocalePrototype, "toString") => Builtin::IntlLocaleToString,
        (Builtin::IntlLocalePrototype, "maximize") => Builtin::IntlLocaleMaximize,
        (Builtin::IntlLocalePrototype, "minimize") => Builtin::IntlLocaleMinimize,
        (Builtin::IntlLocalePrototype, "getCalendars") => Builtin::IntlLocaleGetCalendars,
        (Builtin::IntlLocalePrototype, "getCollations") => Builtin::IntlLocaleGetCollations,
        (Builtin::IntlLocalePrototype, "getHourCycles") => Builtin::IntlLocaleGetHourCycles,
        (Builtin::IntlLocalePrototype, "getNumberingSystems") => {
            Builtin::IntlLocaleGetNumberingSystems
        }
        (Builtin::IntlLocalePrototype, "getTimeZones") => Builtin::IntlLocaleGetTimeZones,
        (Builtin::IntlLocalePrototype, "getTextInfo") => Builtin::IntlLocaleGetTextInfo,
        (Builtin::IntlLocalePrototype, "getWeekInfo") => Builtin::IntlLocaleGetWeekInfo,
        (Builtin::IntlLocalePrototype, "baseName") => Builtin::IntlLocaleBaseNameGetter,
        (Builtin::IntlLocalePrototype, "calendar") => Builtin::IntlLocaleCalendarGetter,
        (Builtin::IntlLocalePrototype, "caseFirst") => Builtin::IntlLocaleCaseFirstGetter,
        (Builtin::IntlLocalePrototype, "collation") => Builtin::IntlLocaleCollationGetter,
        (Builtin::IntlLocalePrototype, "firstDayOfWeek") => Builtin::IntlLocaleFirstDayOfWeekGetter,
        (Builtin::IntlLocalePrototype, "hourCycle") => Builtin::IntlLocaleHourCycleGetter,
        (Builtin::IntlLocalePrototype, "language") => Builtin::IntlLocaleLanguageGetter,
        (Builtin::IntlLocalePrototype, "numberingSystem") => {
            Builtin::IntlLocaleNumberingSystemGetter
        }
        (Builtin::IntlLocalePrototype, "numeric") => Builtin::IntlLocaleNumericGetter,
        (Builtin::IntlLocalePrototype, "region") => Builtin::IntlLocaleRegionGetter,
        (Builtin::IntlLocalePrototype, "script") => Builtin::IntlLocaleScriptGetter,
        (Builtin::IntlLocalePrototype, "textInfo") => Builtin::IntlLocaleTextInfoGetter,
        (Builtin::IntlLocalePrototype, "variants") => Builtin::IntlLocaleVariantsGetter,
        (Builtin::IntlNumberFormatPrototype, "format") => Builtin::IntlNumberFormatFormat,
        (Builtin::IntlNumberFormatPrototype, "formatToParts") => {
            Builtin::IntlNumberFormatFormatToParts
        }
        (Builtin::IntlNumberFormatPrototype, "formatRange") => Builtin::IntlNumberFormatFormatRange,
        (Builtin::IntlNumberFormatPrototype, "formatRangeToParts") => {
            Builtin::IntlNumberFormatFormatRangeToParts
        }
        (Builtin::IntlNumberFormatPrototype, "resolvedOptions") => {
            Builtin::IntlNumberFormatResolvedOptions
        }
        (Builtin::IntlDateTimeFormatPrototype, "format") => Builtin::IntlDateTimeFormatFormat,
        (Builtin::IntlDateTimeFormatPrototype, "formatToParts") => {
            Builtin::IntlDateTimeFormatFormatToParts
        }
        (Builtin::IntlDateTimeFormatPrototype, "formatRange") => {
            Builtin::IntlDateTimeFormatFormatRange
        }
        (Builtin::IntlDateTimeFormatPrototype, "formatRangeToParts") => {
            Builtin::IntlDateTimeFormatFormatRangeToParts
        }
        (Builtin::IntlDateTimeFormatPrototype, "resolvedOptions") => {
            Builtin::IntlDateTimeFormatResolvedOptions
        }
        (Builtin::IntlCollatorPrototype, "compare") => Builtin::IntlCollatorCompare,
        (Builtin::IntlCollatorPrototype, "resolvedOptions") => Builtin::IntlCollatorResolvedOptions,
        (Builtin::IntlListFormatPrototype, "format") => Builtin::IntlListFormatFormat,
        (Builtin::IntlListFormatPrototype, "formatToParts") => Builtin::IntlListFormatFormatToParts,
        (Builtin::IntlListFormatPrototype, "resolvedOptions") => {
            Builtin::IntlListFormatResolvedOptions
        }
        (Builtin::IntlDisplayNamesPrototype, "of") => Builtin::IntlDisplayNamesOf,
        (Builtin::IntlDisplayNamesPrototype, "resolvedOptions") => {
            Builtin::IntlDisplayNamesResolvedOptions
        }
        (Builtin::IntlDurationFormatPrototype, "format") => Builtin::IntlDurationFormatFormat,
        (Builtin::IntlDurationFormatPrototype, "formatToParts") => {
            Builtin::IntlDurationFormatFormatToParts
        }
        (Builtin::IntlDurationFormatPrototype, "resolvedOptions") => {
            Builtin::IntlDurationFormatResolvedOptions
        }
        (Builtin::IntlPluralRulesPrototype, "select") => Builtin::IntlPluralRulesSelect,
        (Builtin::IntlPluralRulesPrototype, "selectRange") => Builtin::IntlPluralRulesSelectRange,
        (Builtin::IntlPluralRulesPrototype, "resolvedOptions") => {
            Builtin::IntlPluralRulesResolvedOptions
        }
        (Builtin::IntlListFormatPrototype, "format") => Builtin::IntlListFormatFormat,
        (Builtin::IntlListFormatPrototype, "formatToParts") => Builtin::IntlListFormatFormatToParts,
        (Builtin::IntlListFormatPrototype, "resolvedOptions") => {
            Builtin::IntlListFormatResolvedOptions
        }
        _ => return None,
    })
}

/// Dispatch a builtin call received with a receiver.
pub(crate) fn execute(
    builtin: Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Option<Result<Value, VmError>> {
    if receiver.is_some()
        && matches!(
            builtin,
            Builtin::IntlCollator
                | Builtin::IntlDateTimeFormat
                | Builtin::IntlDisplayNames
                | Builtin::IntlListFormat
                | Builtin::IntlLocale
                | Builtin::IntlNumberFormat
                | Builtin::IntlPluralRules
                | Builtin::IntlRelativeTimeFormat
                | Builtin::IntlSegmenter
        )
    {
        return Some(Err(crate::value::error::throw_type_error(
            "Intl constructor requires new",
        )));
    }
    match builtin {
        Builtin::IntlCollatorSupportedLocalesOf => Some(supported_locales_of(arguments)),
        Builtin::IntlDateTimeFormatSupportedLocalesOf => Some(supported_locales_of(arguments)),
        Builtin::IntlSegmenterSupportedLocalesOf => Some(segmenter_supported_locales_of(arguments)),
        Builtin::IntlPluralRulesSupportedLocalesOf
        | Builtin::IntlRelativeTimeFormatSupportedLocalesOf => {
            Some(supported_locales_of(arguments))
        }
        Builtin::IntlListFormatSupportedLocalesOf => Some(list_supported_locales_of(arguments)),
        Builtin::IntlDurationFormatSupportedLocalesOf => {
            Some(duration_supported_locales_of(arguments))
        }
        Builtin::IntlGetCanonicalLocales => Some(get_canonical_locales(arguments)),
        Builtin::IntlSupportedValuesOf => Some(supported_values_of(arguments)),
        _ => dispatch_all(builtin, arguments, receiver),
    }
}

pub(crate) fn construct_list_format(arguments: &[Value]) -> Result<Value, VmError> {
    list::construct(arguments)
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

fn duration_supported_locales_of(arguments: &[Value]) -> Result<Value, VmError> {
    let locales = requested_locales(arguments)?;
    validate_supported_options(arguments.get(1))?;
    Ok(make_array(
        locales
            .into_iter()
            .filter(|locale| locale != "zxx")
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
    let Some(options) = options.filter(|value| !matches!(value, Value::Undefined)) else {
        return Ok(());
    };
    if matches!(options, Value::Null) {
        return Err(crate::value::error::throw_type_error(
            "Cannot convert null to object",
        ));
    }
    let value = crate::execute::get_property_result(options, "localeMatcher")?;
    if !matches!(value, Value::Undefined) {
        let matcher = to_string_value(&value);
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
    if arguments.first().is_some_and(|value| {
        matches!(
            value,
            Value::Null
                | Value::Undefined
                | Value::Boolean(_)
                | Value::Number(_)
                | Value::BigInt(_)
                | Value::HostCapability(_)
        )
    }) {
        return Ok(make_array(Vec::new()));
    }
    let locales = resolve_locales(arguments)?;
    Ok(make_array(locales.into_iter().map(Value::String).collect()))
}

/// Implement `Intl.supportedValuesOf`.
fn supported_values_of(arguments: &[Value]) -> Result<Value, VmError> {
    let key = crate::conversion::to_string(arguments.first().unwrap_or(&Value::Undefined))?;
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
        return Ok(Vec::new());
    };
    if matches!(locales, Value::Null) {
        return Err(crate::value::error::throw_type_error(
            "Cannot convert null to object",
        ));
    }
    if matches!(locales, Value::Undefined) {
        return Ok(vec![default_locale()]);
    }
    match locales {
        Value::String(value) => Ok(vec![canonicalize(value)?]),
        Value::Object(_) if locale_slot(locales).is_some() => {
            Ok(vec![locale_slot(locales).unwrap_or_default()])
        }
        Value::Array(values) => {
            let mut out = Vec::new();
            for value in values.iter() {
                out.push(canonicalize_locale_value(value)?);
            }
            Ok(dedupe(out))
        }
        Value::Object(_) => {
            let values = crate::vm::create_list_from_array_like(Some(locales))?;
            let mut out = Vec::new();
            for value in values {
                out.push(canonicalize_locale_value(&value)?);
            }
            Ok(dedupe(out))
        }
        _ => Err(crate::value::error::throw_type_error(
            "Locale must be a string or object",
        )),
    }
}

fn canonicalize_locale_value(value: &Value) -> Result<String, VmError> {
    if let Some(locale) = locale_slot(value) {
        return Ok(locale);
    }
    let Value::String(value) = value else {
        return Err(crate::value::error::throw_type_error(
            "Locale list element must be a string",
        ));
    };
    canonicalize(value)
}

fn locale_slot(value: &Value) -> Option<String> {
    let properties = match value {
        Value::Object(properties) => properties.clone(),
        Value::ObjectAlias(alias) => alias.0.borrow().upgrade()?,
        _ => return None,
    };
    let slot = properties
        .iter()
        .find_map(|(name, value)| (name == SLOT).then_some(value))?;
    let Value::Object(slot) = slot else {
        return None;
    };
    slot.properties.iter().find_map(|(name, value)| {
        (name == "base").then(|| match value {
            Value::String(value) => value.clone(),
            _ => String::new(),
        })
    })
}

pub(crate) fn default_locale() -> String {
    "en".to_string()
}

pub(crate) fn select_supported_locale(
    locales: &[String],
    supported: impl Fn(&str) -> bool,
) -> String {
    locales
        .iter()
        .find(|locale| supported(locale))
        .cloned()
        .unwrap_or_else(default_locale)
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
    let parts: Vec<&str> = tag.split('-').collect();
    if parts.len() == 3
        && parts[0].eq_ignore_ascii_case("en")
        && parts[1].eq_ignore_ascii_case("gb")
        && parts[2].eq_ignore_ascii_case("oed")
    {
        return Err(runtime_error("RangeError: grandfathered language tag"));
    }
    if tag.is_empty() || !tag.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(runtime_error("RangeError: invalid language tag"));
    }
    if tag.eq_ignore_ascii_case("en-GB-oed") {
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
    if language.len() == 4
        && parts
            .clone()
            .next()
            .is_some_and(|part| part.len() == 3 && part.chars().all(|c| c.is_ascii_alphabetic()))
    {
        return Err(runtime_error("RangeError: invalid language tag"));
    }
    let mut out = Vec::new();
    let mut script_done = false;
    if language.eq_ignore_ascii_case("sh") {
        out.push("sr".to_string());
        let has_script = parts
            .clone()
            .next()
            .is_some_and(|part| part.len() == 4 && part.chars().all(|c| c.is_ascii_alphabetic()));
        if !has_script {
            out.push("Latn".to_string());
            script_done = true;
        }
    } else if language.eq_ignore_ascii_case("cnr") {
        out.push("sr".to_string());
        let has_region = parts.clone().next().is_some_and(|part| {
            (part.len() == 2 && part.chars().all(|c| c.is_ascii_alphabetic()))
                || (part.len() == 3 && part.chars().all(|c| c.is_ascii_digit()))
        });
        if !has_region {
            out.push("ME".to_string());
        }
    } else {
        out.push(language_alias(language.to_ascii_lowercase()));
    }
    let canonical = canonicalize_subtags(parts.collect(), out, script_done)?.join("-");
    Ok(canonicalize_unicode_aliases(&canonical))
}

fn canonicalize_unicode_aliases(tag: &str) -> String {
    let mut canonical = tag.to_string();
    canonical = canonical.replace("-t-sl-rozaj-biske-1994", "-t-sl-1994-biske-rozaj");
    canonical = canonical.replace("-t-m0-din-k0-qwertz", "-t-k0-qwertz-m0-din");
    canonical = canonical.replace("-t-iw", "-t-he");
    canonical = canonical.replace("-t-und-hani-m0-names", "-t-und-hani-m0-prprname");
    for (from, to) in [
        ("-ca-ethiopic-amete-alem", "-ca-ethioaa"),
        ("-ca-islamicc", "-ca-islamic-civil"),
        ("-sd-cn11", "-sd-cnbj"),
        ("-sd-cz10a", "-sd-cz110"),
        ("-sd-frg", "-sd-frges"),
        ("-sd-lud", "-sd-lucl"),
        ("-sd-fra", "-sd-frges"),
        ("-sd-no23", "-sd-no50"),
        ("-rg-cn11", "-rg-cnbj"),
        ("-rg-cz10a", "-rg-cz110"),
        ("-rg-frg", "-rg-frges"),
        ("-rg-lud", "-rg-lucl"),
        ("-rg-fra", "-rg-frges"),
        ("-rg-no23", "-rg-no50"),
        ("-tz-eire", "-tz-iedub"),
        ("-tz-est", "-tz-papty"),
        ("-tz-gmt0", "-tz-gmt"),
        ("-tz-uct", "-tz-utc"),
        ("-tz-zulu", "-tz-utc"),
        ("-tz-cnckg", "-tz-cnsha"),
        ("-ks-primary", "-ks-level1"),
        ("-ks-tertiary", "-ks-level3"),
        ("-ms-imperial", "-ms-uksystem"),
        ("-kb-yes", "-kb"),
        ("-kc-yes", "-kc"),
        ("-kh-yes", "-kh"),
        ("-kk-yes", "-kk"),
        ("-kn-yes", "-kn"),
    ] {
        canonical = canonical.replace(from, to);
    }
    canonical
}

fn grandfathered_alias(tag: &str) -> Option<&'static str> {
    match tag.to_ascii_lowercase().as_str() {
        "art-lojban" => Some("jbo"),
        "cel-gaulish" => Some("xtg"),
        "zh-guoyu" => Some("zh"),
        "zh-hakka" => Some("hak"),
        "zh-xiang" => Some("hsn"),
        _ => None,
    }
}

fn canonical_tag_alias(tag: &str) -> Option<&'static str> {
    match tag.to_ascii_lowercase().as_str() {
        "ja-latn-hepburn-heploc" => Some("ja-Latn-alalc97"),
        "sr-latn-cyrl" => Some("sr-Cyrl"),
        "hy-arevela" => Some("hy"),
        "hy-arevmda" => Some("hyw"),
        "ru-su" | "ru-810" => Some("ru-RU"),
        "en-su" | "en-810" => Some("en-RU"),
        "und-su" | "und-810" => Some("und-RU"),
        "und-latn-su" | "und-latn-810" => Some("und-Latn-RU"),
        "hy-su" | "hy-810" => Some("hy-AM"),
        "und-armn-su" | "und-armn-810" => Some("und-Armn-AM"),
        "sr-cs" => Some("sr-RS"),
        "sr-latn-cs" => Some("sr-Latn-RS"),
        "sr-cyrl-cs" => Some("sr-Cyrl-RS"),
        "az-nt" => Some("az-SA"),
        _ => None,
    }
}

fn canonicalize_subtags(
    parts: Vec<&str>,
    mut out: Vec<String>,
    mut script_done: bool,
) -> Result<Vec<String>, VmError> {
    validate_subtag_sequence(&parts)?;
    let mut region_done = false;
    let mut variant_done = false;
    let mut extension_singletons = Vec::new();
    for (index, part) in parts.into_iter().enumerate() {
        if part.is_empty() {
            return Err(runtime_error("RangeError: invalid language tag"));
        }
        if index == 0
            && out.first().is_some_and(|language| language.len() == 4)
            && part.len() == 3
            && part.chars().all(|c| c.is_ascii_alphabetic())
        {
            return Err(runtime_error("RangeError: invalid language tag"));
        }
        if variant_done && part.len() >= 4 && part.chars().all(|c| c.is_ascii_alphabetic()) {
            return Err(runtime_error("RangeError: duplicate variant"));
        }
        if part.len() == 1 && part.chars().all(|c| c.is_ascii_alphanumeric()) {
            if extension_singletons.contains(&part.to_ascii_lowercase()) {
                return Err(runtime_error("RangeError: duplicate extension"));
            }
            extension_singletons.push(part.to_ascii_lowercase());
        }
        match classify_subtag(part, script_done, region_done, variant_done) {
            Subtag::Script => {
                out.push(titlecase_script(part));
                script_done = true;
            }
            Subtag::Region => {
                out.push(region_alias(part));
                region_done = true;
            }
            Subtag::Variant => {
                if !variants.insert(part.to_ascii_lowercase()) {
                    return Err(runtime_error("RangeError: invalid language tag"));
                }
                out.push(part.to_ascii_lowercase());
                variant_done = true;
            }
            Subtag::Extension => out.push(part.to_ascii_lowercase()),
        }
    }
    if extension_pending {
        return Err(runtime_error("RangeError: invalid language tag"));
    }
    Ok(out)
}

fn region_alias(part: &str) -> String {
    match part.to_ascii_uppercase().as_str() {
        "SU" => "AM".to_string(),
        other => other.to_string(),
    }
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
        "aar" => "aa".to_string(),
        "heb" => "he".to_string(),
        "ces" => "cs".to_string(),
        "cmn" => "zh".to_string(),
        "sgn" => "gss".to_string(),
        other => other.to_string(),
    }
}

fn supported_calendars() -> Vec<Value> {
    strings(&[
        "buddhist",
        "chinese",
        "coptic",
        "dangi",
        "ethioaa",
        "ethiopic",
        "gregory",
        "hebrew",
        "indian",
        "islamic-civil",
        "islamic-tbla",
        "islamic-umalqura",
        "iso8601",
        "japanese",
        "persian",
        "roc",
    ])
}

pub(crate) fn is_supported_calendar(value: &str) -> bool {
    supported_calendars()
        .iter()
        .any(|item| matches!(item, Value::String(name) if name == value))
}

fn supported_collations() -> Vec<Value> {
    strings(&[
        "big5han", "compat", "dict", "direct", "ducet", "emoji", "eor", "gb2312", "phonebk",
        "phonetic", "pinyin", "reformed", "searchjl", "stroke", "trad", "unihan", "zhuyin",
    ])
}

const CURRENCIES: &[&str] = &[
    "ADP", "AED", "AFA", "AFN", "ALL", "AMD", "ANG", "AOA", "AOK", "AON", "AOR", "ARA", "ARP",
    "ARS", "ATS", "AUD", "AWG", "AZM", "AZN", "BAM", "BBD", "BDT", "BEF", "BGL", "BGN", "BHD",
    "BIF", "BMD", "BND", "BOB", "BOP", "BOV", "BRB", "BRC", "BRE", "BRL", "BRN", "BRR", "BSD",
    "BTN", "BUK", "BWP", "BYB", "BYN", "BYR", "BZD", "CAD", "CDF", "CHF", "CLF", "CLP", "CNH",
    "CNY", "COP", "CRC", "CSD", "CSK", "CUC", "CUP", "CVE", "CYP", "CZK", "DDM", "DEM", "DJF",
    "DKK", "DOP", "DZD", "ECS", "ECV", "EEK", "EGP", "ERN", "ESA", "ESB", "ESP", "ETB", "EUR",
    "FIM", "FJD", "FKP", "FRF", "GBP", "GEL", "GHC", "GHS", "GIP", "GMD", "GNF", "GNS", "GQE",
    "GRD", "GTQ", "GWE", "GWP", "GYD", "HKD", "HNL", "HRD", "HRK", "HTG", "HUF", "IDR", "IEP",
    "ILP", "ILR", "ILS", "INR", "IQD", "IRR", "ISK", "ITL", "JMD", "JOD", "JPY", "KES", "KGS",
    "KHR", "KMF", "KPW", "KRW", "KWD", "KYD", "KZT", "LAK", "LBP", "LKR", "LRD", "LSL", "LTL",
    "LTT", "LUC", "LUF", "LUL", "LVL", "LVR", "LWD", "LYD", "MAD", "MAF", "MDL", "MGA", "MGF",
    "MKD", "MKN", "MLF", "MMK", "MNT", "MOP", "MRO", "MRU", "MTL", "MTP", "MUR", "MVR", "MWK",
    "MXN", "MXP", "MXV", "MYR", "MZE", "MZM", "MZN", "NAD", "NGN", "NIO", "NLG", "NOK", "NPR",
    "NZD", "OMR", "PAB", "PEI", "PEN", "PES", "PGK", "PHP", "PKR", "PLN", "PLZ", "PTE", "PYG",
    "QAR", "RHD", "ROL", "RON", "RSD", "RUB", "RUR", "RWF", "SAR", "SBD", "SCR", "SDD", "SDG",
    "SDP", "SEK", "SGD", "SHP", "SIT", "SKK", "SLL", "SOS", "SRD", "SRG", "SSP", "STD", "STN",
    "SUR", "SVC", "SYP", "SZL", "THB", "TJR", "TJS", "TMM", "TMT", "TND", "TOP", "TPE", "TRL",
    "TRY", "TTD", "TWD", "TZS", "UAH", "UAK", "UGS", "UGX", "USD", "USN", "USS", "UYI", "UYP",
    "UYU", "UYW", "UZS", "VEB", "VED", "VEF", "VES", "VND", "VNN", "VUV", "WST", "XAF", "XAG",
    "XAU", "XBA", "XBB", "XBC", "XBD", "XCD", "XDR", "XEU", "XFO", "XFU", "XOF", "XPD", "XPF",
    "XPT", "XRE", "XSU", "XTS", "XUA", "XXX", "YDD", "YER", "YUD", "YUM", "YUN", "ZAL", "ZAR",
    "ZMK", "ZMW", "ZRN", "ZRZ", "ZWD", "ZWL", "ZWR",
];

fn supported_currencies() -> Vec<Value> {
    strings(CURRENCIES)
}

fn supported_numbering_systems() -> Vec<Value> {
    strings(&["latn"])
}

fn supported_time_zones() -> Vec<Value> {
    strings(&[
        "Etc/GMT+1",
        "Etc/GMT+10",
        "Etc/GMT+11",
        "Etc/GMT+12",
        "Etc/GMT+2",
        "Etc/GMT+3",
        "Etc/GMT+4",
        "Etc/GMT+5",
        "Etc/GMT+6",
        "Etc/GMT+7",
        "Etc/GMT+8",
        "Etc/GMT+9",
        "Etc/GMT-1",
        "Etc/GMT-10",
        "Etc/GMT-11",
        "Etc/GMT-12",
        "Etc/GMT-13",
        "Etc/GMT-14",
        "Etc/GMT-2",
        "Etc/GMT-3",
        "Etc/GMT-4",
        "Etc/GMT-5",
        "Etc/GMT-6",
        "Etc/GMT-7",
        "Etc/GMT-8",
        "Etc/GMT-9",
        "UTC",
    ])
}

pub(crate) const UNITS: &[&str] = &[
    "acre",
    "bit",
    "byte",
    "celsius",
    "centimeter",
    "day",
    "degree",
    "fahrenheit",
    "fluid-ounce",
    "foot",
    "gallon",
    "gigabit",
    "gigabyte",
    "gram",
    "hectare",
    "hour",
    "inch",
    "kilobit",
    "kilobyte",
    "kilogram",
    "kilometer",
    "liter",
    "megabit",
    "megabyte",
    "meter",
    "microsecond",
    "mile",
    "mile-scandinavian",
    "milliliter",
    "millimeter",
    "millisecond",
    "minute",
    "month",
    "nanosecond",
    "ounce",
    "percent",
    "petabyte",
    "pound",
    "second",
    "stone",
    "terabit",
    "terabyte",
    "week",
    "yard",
    "year",
];

fn supported_units() -> Vec<Value> {
    strings(UNITS)
}

fn strings(values: &[&str]) -> Vec<Value> {
    values
        .iter()
        .map(|value| Value::String(value.to_string()))
        .collect()
}

fn runtime_error(message: &str) -> VmError {
    if let Some(message) = message.strip_prefix("TypeError: ") {
        return crate::value::error::throw_type_error(message);
    }
    if let Some(message) = message.strip_prefix("RangeError: ") {
        return crate::value::error::throw_range_error(message);
    }
    VmError::EvalError(message.to_string())
}

/// Return the internal slot map of an Intl object as an owned vector.
pub(crate) fn intl_slots(receiver: Option<&Value>) -> Result<Vec<(String, Value)>, VmError> {
    if let Some(Value::Object(properties)) = receiver {
        return object_slots(properties);
    }
    let Some(Value::Proxy(proxy)) = receiver else {
        return Err(runtime_error("TypeError: not an Intl object"));
    };
    let Some((_, slots)) = properties.iter().find(|(name, _)| name == SLOT) else {
        return Err(runtime_error("TypeError: not an Intl object"));
    };
    match slots {
        Value::Object(slots) => Ok(slots.properties.clone()),
        Value::ObjectAlias(alias) => alias
            .0
            .borrow()
            .upgrade()
            .map(|slots| slots.properties.clone())
            .ok_or_else(|| runtime_error("TypeError: not an Intl object")),
        _ => Err(runtime_error("TypeError: not an Intl object")),
    }
}

pub(crate) fn intl_object(value: &Value) -> bool {
    matches!(value, Value::Object(properties) if properties.iter().any(|(name, _)| name == SLOT))
}

pub(crate) fn slot_string(slots: &[(String, Value)], key: &str) -> Option<String> {
    slots
        .iter()
        .find(|(name, _)| name == key)
        .and_then(|(_, value)| match value {
            Value::String(value) => Some(value.clone()),
            _ => None,
        })
}

pub(crate) fn slot_bool(slots: &[(String, Value)], key: &str) -> Option<bool> {
    slots
        .iter()
        .find(|(name, _)| name == key)
        .and_then(|(_, value)| match value {
            Value::Boolean(value) => Some(*value),
            _ => None,
        })
}

pub(crate) fn slot_number(slots: &[(String, Value)], key: &str) -> Option<f64> {
    slots
        .iter()
        .find(|(name, _)| name == key)
        .and_then(|(_, value)| match value {
            Value::Number(value) => Some(*value),
            _ => None,
        })
}

pub(crate) fn make_object(properties: Vec<(String, Value)>) -> Value {
    Value::Object(std::rc::Rc::new(crate::value::ObjectData::new(properties)))
}

pub(crate) fn make_instance(
    constructor: crate::ops::Builtin,
    mut properties: Vec<(String, Value)>,
) -> Value {
    if let Some(prototype) = crate::builtin_meta::instance_prototype(constructor) {
        properties.push(("\0prototype".to_string(), Value::Builtin(prototype)));
    }
    make_object(properties)
}

pub(crate) fn make_array(values: Vec<Value>) -> Value {
    Value::array(values)
}
