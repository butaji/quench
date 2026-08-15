//! `Intl.Locale` constructor and prototype methods.

use crate::{execute::VmError, value::Value};

use super::{canonicalize, make_array, make_object, runtime_error, slot_string, to_string_value};

pub(crate) struct Locale {
    pub language: String,
    pub script: Option<String>,
    pub region: Option<String>,
    pub calendar: Option<String>,
    pub collation: Option<String>,
    pub case_first: Option<String>,
    pub hour_cycle: Option<String>,
    pub numbering_system: Option<String>,
    pub numeric: bool,
}

impl Locale {
    pub fn base_name(&self) -> String {
        let mut base = self.language.clone();
        if let Some(script) = &self.script {
            base.push('-');
            base.push_str(script);
        }
        if let Some(region) = &self.region {
            base.push('-');
            base.push_str(region);
        }
        base
    }
}

pub(crate) fn construct(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(tag_arg) = arguments.first() else {
        return Err(runtime_error("RangeError: Locale requires a tag"));
    };
    let tag = locale_tag(tag_arg)?;
    let canonical = canonicalize(&tag)?;
    let locale = parse_canonical(&canonical);
    let options = arguments.get(1);
    let locale = apply_options(locale, options)?;
    Ok(build_object(locale))
}

fn construct_call(arguments: &[Value], receiver: Option<&Value>) -> Result<Value, VmError> {
    if receiver.is_some() {
        return Err(crate::value::error::throw_type_error(
            "Intl.Locale requires new",
        ));
    }
    construct(arguments)
}

fn locale_tag(value: &Value) -> Result<String, VmError> {
    match value {
        Value::String(_) | Value::Object(_) => Ok(to_string_value(value)),
        _ => Err(runtime_error("TypeError: locale tag must be a string")),
    }
}

fn parse_canonical(tag: &str) -> Locale {
    let parts: Vec<&str> = tag.split('-').collect();
    let mut locale = Locale {
        language: parts.first().map(|p| p.to_string()).unwrap_or_default(),
        script: None,
        region: None,
        calendar: None,
        collation: None,
        case_first: None,
        hour_cycle: None,
        numbering_system: None,
        numeric: false,
    };
    let mut index = 1;
    if parts
        .get(index)
        .is_some_and(|p| p.len() == 4 && p.chars().all(|c| c.is_ascii_alphabetic()))
    {
        locale.script = Some(parts[index].to_string());
        index += 1;
    }
    if parts
        .get(index)
        .is_some_and(|p| p.len() == 2 || (p.len() == 3 && p.chars().all(|c| c.is_ascii_digit())))
    {
        locale.region = Some(parts[index].to_string());
        index += 1;
    }
    parse_extensions(&mut locale, &parts[index..]);
    locale
}

fn parse_extensions(locale: &mut Locale, parts: &[&str]) {
    let mut i = 0;
    while i < parts.len() {
        if parts[i] == "u" {
            let mut j = i + 1;
            while j + 1 < parts.len() {
                let key = parts[j];
                let item = parts[j + 1];
                match key {
                    "ca" => {
                        if locale.calendar.is_none() {
                            let value = calendar_extension_value(parts, j, item);
                            locale.calendar = Some(calendar_alias(&value));
                        }
                    }
                    "co" if locale.collation.is_none() => locale.collation = Some(item.to_string()),
                    "kf" if locale.case_first.is_none() => {
                        locale.case_first = Some(item.to_string())
                    }
                    "hc" if locale.hour_cycle.is_none() => {
                        locale.hour_cycle = Some(item.to_string())
                    }
                    "nu" if locale.numbering_system.is_none() => {
                        locale.numbering_system = Some(item.to_string())
                    }
                    "kn" => locale.numeric = item == "true",
                    _ => {}
                }
                j += calendar_extension_width(parts, j, key, item);
            }
            break;
        }
        i += 1;
    }
}

fn calendar_alias(value: &str) -> String {
    match value {
        "islamicc" => "islamic-civil".to_string(),
        "ethiopic-amete-alem" => "ethioaa".to_string(),
        other => canonicalize(other).unwrap_or_else(|_| other.to_string()),
    }
}

fn calendar_extension_value(parts: &[&str], index: usize, item: &str) -> String {
    match (
        item,
        parts.get(index + 2).copied(),
        parts.get(index + 3).copied(),
    ) {
        ("islamic", Some("civil"), _) => "islamic-civil".to_string(),
        ("ethiopic", Some("amete"), Some("alem")) => "ethiopic-amete-alem".to_string(),
        _ => item.to_string(),
    }
}

fn calendar_extension_width(parts: &[&str], index: usize, key: &str, item: &str) -> usize {
    if key != "ca" {
        return 2;
    }
    match (
        item,
        parts.get(index + 2).copied(),
        parts.get(index + 3).copied(),
    ) {
        ("islamic", Some("civil"), _) => 3,
        ("ethiopic", Some("amete"), Some("alem")) => 4,
        _ => 2,
    }
}

fn apply_options(mut locale: Locale, options: Option<&Value>) -> Result<Locale, VmError> {
    if matches!(options, Some(Value::Null)) {
        return Err(crate::value::error::throw_type_error(
            "Cannot convert null or undefined to object",
        ));
    }
    let Some(options) = options.filter(|value| !matches!(value, Value::Undefined)) else {
        return Ok(locale);
    };
    if !matches!(options, Value::Object(_) | Value::Proxy(_)) {
        return Ok(locale);
    }
    for key in [
        "calendar",
        "collation",
        "caseFirst",
        "hourCycle",
        "numberingSystem",
        "numeric",
        "firstDayOfWeek",
        "language",
        "region",
        "script",
        "variants",
    ] {
        let value = crate::execute::get_property_result(options, key)?;
        if matches!(value, Value::Undefined) {
            continue;
        }
        let text = crate::conversion::to_string(&value)?;
        match key {
            "calendar" => {
                let value = option_value(&text, "calendar")?;
                if !value.split('-').all(|part| {
                    (3..=8).contains(&part.len())
                        && part
                            .chars()
                            .all(|character| character.is_ascii_alphanumeric())
                }) {
                    return Err(runtime_error("RangeError: invalid calendar"));
                }
                locale.calendar = Some(calendar_alias(&value));
            }
            "collation" => locale.collation = Some(option_value(&text, "collation")?),
            "caseFirst" => locale.case_first = Some(normalize_case_first(&text)?),
            "hourCycle" => locale.hour_cycle = Some(normalize_hour_cycle(&text)?),
            "numberingSystem" => {
                locale.numbering_system = Some(option_value(&text, "numberingSystem")?)
            }
            "numeric" => locale.numeric = normalize_numeric(&value, &text)?,
            "firstDayOfWeek" => {
                let _ = option_value(&text, "firstDayOfWeek")?;
            }
            "language" | "region" | "script" | "variants" => {
                let _ = option_value(&text, key)?;
            }
            _ => {}
        }
    }
    Ok(locale)
}

fn option_value(value: &str, name: &str) -> Result<String, VmError> {
    if value.is_empty() || !value.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(runtime_error(&format!("RangeError: invalid {name}")));
    }
    Ok(value.to_string())
}

fn normalize_case_first(value: &str) -> Result<String, VmError> {
    match value {
        "upper" | "lower" | "false" => Ok(value.to_string()),
        _ => Err(runtime_error("RangeError: invalid caseFirst")),
    }
}

fn normalize_hour_cycle(value: &str) -> Result<String, VmError> {
    match value {
        "h11" | "h12" | "h23" | "h24" => Ok(value.to_string()),
        _ => Err(runtime_error("RangeError: invalid hourCycle")),
    }
}

fn normalize_numeric(value: &Value, text: &str) -> Result<bool, VmError> {
    let truthy = match value {
        Value::Undefined => return Ok(false),
        Value::Null => false,
        Value::Boolean(value) => *value,
        Value::Number(value) => *value != 0.0 && !value.is_nan(),
        _ => !text.is_empty(),
    };
    Ok(truthy)
}

fn build_object(locale: Locale) -> Value {
    let mut properties = base_properties(&locale);
    properties.extend(method_properties());
    properties.push(("__intl".to_string(), locale_slot(&locale)));
    properties.push((
        "\0prototype".to_string(),
        Value::Builtin(crate::ops::Builtin::IntlLocalePrototype),
    ));
    make_object(properties)
}

fn base_properties(locale: &Locale) -> Vec<(String, Value)> {
    let mut properties = vec![
        (
            "language".to_string(),
            Value::String(locale.language.clone()),
        ),
        ("baseName".to_string(), Value::String(locale.base_name())),
        ("numeric".to_string(), Value::Boolean(locale.numeric)),
    ];
    if let Some(script) = &locale.script {
        properties.push(("script".to_string(), Value::String(script.clone())));
    }
    if let Some(region) = &locale.region {
        properties.push(("region".to_string(), Value::String(region.clone())));
    }
    if let Some(calendar) = &locale.calendar {
        properties.push(("calendar".to_string(), Value::String(calendar.clone())));
    }
    if let Some(collation) = &locale.collation {
        properties.push(("collation".to_string(), Value::String(collation.clone())));
    }
    if let Some(case_first) = &locale.case_first {
        properties.push(("caseFirst".to_string(), Value::String(case_first.clone())));
    }
    if let Some(hour_cycle) = &locale.hour_cycle {
        properties.push(("hourCycle".to_string(), Value::String(hour_cycle.clone())));
    }
    if let Some(numbering) = &locale.numbering_system {
        properties.push((
            "numberingSystem".to_string(),
            Value::String(numbering.clone()),
        ));
    }
    properties
}

fn method_properties() -> Vec<(String, Value)> {
    [
        ("toString", crate::ops::Builtin::IntlLocaleToString),
        ("maximize", crate::ops::Builtin::IntlLocaleMaximize),
        ("minimize", crate::ops::Builtin::IntlLocaleMinimize),
        ("getCalendars", crate::ops::Builtin::IntlLocaleGetCalendars),
        (
            "getCollations",
            crate::ops::Builtin::IntlLocaleGetCollations,
        ),
        (
            "getHourCycles",
            crate::ops::Builtin::IntlLocaleGetHourCycles,
        ),
        (
            "getNumberingSystems",
            crate::ops::Builtin::IntlLocaleGetNumberingSystems,
        ),
        ("getTimeZones", crate::ops::Builtin::IntlLocaleGetTimeZones),
        ("getTextInfo", crate::ops::Builtin::IntlLocaleGetTextInfo),
        ("getWeekInfo", crate::ops::Builtin::IntlLocaleGetWeekInfo),
    ]
    .iter()
    .map(|(name, builtin)| (name.to_string(), Value::Builtin(*builtin)))
    .collect()
}

fn locale_slot(locale: &Locale) -> Value {
    let mut properties = vec![
        (
            "language".to_string(),
            Value::String(locale.language.clone()),
        ),
        ("base".to_string(), Value::String(locale.base_name())),
    ];
    if let Some(script) = &locale.script {
        properties.push(("script".to_string(), Value::String(script.clone())));
    }
    if let Some(region) = &locale.region {
        properties.push(("region".to_string(), Value::String(region.clone())));
    }
    if let Some(calendar) = &locale.calendar {
        properties.push(("calendar".to_string(), Value::String(calendar.clone())));
    }
    if let Some(collation) = &locale.collation {
        properties.push(("collation".to_string(), Value::String(collation.clone())));
    }
    if let Some(hour_cycle) = &locale.hour_cycle {
        properties.push(("hourCycle".to_string(), Value::String(hour_cycle.clone())));
    }
    if let Some(case_first) = &locale.case_first {
        properties.push(("caseFirst".to_string(), Value::String(case_first.clone())));
    }
    if let Some(numbering_system) = &locale.numbering_system {
        properties.push((
            "numberingSystem".to_string(),
            Value::String(numbering_system.clone()),
        ));
    }
    properties.push(("numeric".to_string(), Value::Boolean(locale.numeric)));
    properties.push(("full".to_string(), Value::String(slot_full(locale))));
    make_object(properties)
}

fn slot_full(locale: &Locale) -> String {
    let mut full = locale.base_name();
    let keys = slot_keys(locale);
    if !keys.is_empty() {
        full.push_str("-u");
        for (key, item) in keys {
            full.push('-');
            full.push_str(key);
            if !item.is_empty() {
                full.push('-');
                full.push_str(&item);
            }
        }
    }
    full
}

fn slot_keys(locale: &Locale) -> Vec<(&'static str, String)> {
    let mut keys: Vec<(&'static str, String)> = Vec::new();
    if let Some(calendar) = &locale.calendar {
        keys.push(("ca", calendar.clone()));
    }
    if let Some(collation) = &locale.collation {
        keys.push(("co", collation.clone()));
    }
    if let Some(case_first) = &locale.case_first {
        keys.push(("kf", case_first.clone()));
    }
    if let Some(hour_cycle) = &locale.hour_cycle {
        keys.push(("hc", hour_cycle.clone()));
    }
    if let Some(numbering) = &locale.numbering_system {
        keys.push(("nu", numbering.clone()));
    }
    if locale.numeric {
        keys.push(("kn", String::new()));
    }
    keys
}

pub(crate) fn prototype_method(
    builtin: crate::ops::Builtin,
    receiver: Option<&Value>,
) -> Result<Value, VmError> {
    let slot = slot_string(&super::intl_slots(receiver)?, "full").unwrap_or_default();
    match builtin {
        crate::ops::Builtin::IntlLocaleToString => Ok(Value::String(slot)),
        crate::ops::Builtin::IntlLocaleMaximize => Ok(Value::String(maximize(&slot))),
        crate::ops::Builtin::IntlLocaleMinimize => Ok(Value::String(minimize(&slot))),
        crate::ops::Builtin::IntlLocaleGetCalendars => {
            let calendar = slot_string(&super::intl_slots(receiver)?, "calendar")
                .unwrap_or_else(|| "gregory".to_string());
            Ok(make_array(vec![Value::String(calendar)]))
        }
        crate::ops::Builtin::IntlLocaleGetCollations => {
            let collation = slot_string(&super::intl_slots(receiver)?, "collation")
                .unwrap_or_else(|| "default".to_string());
            Ok(make_array(vec![Value::String(collation)]))
        }
        crate::ops::Builtin::IntlLocaleGetHourCycles => {
            Ok(make_array(vec![Value::String("h12".to_string())]))
        }
        crate::ops::Builtin::IntlLocaleGetNumberingSystems => {
            Ok(make_array(vec![Value::String("latn".to_string())]))
        }
        crate::ops::Builtin::IntlLocaleGetTimeZones => {
            if slot_string(&super::intl_slots(receiver)?, "region").is_some() {
                Ok(make_array(vec![Value::String("UTC".to_string())]))
            } else {
                Ok(Value::Undefined)
            }
        }
        crate::ops::Builtin::IntlLocaleGetTextInfo => Ok(make_object(vec![(
            "direction".to_string(),
            Value::String("ltr".to_string()),
        )])),
        crate::ops::Builtin::IntlLocaleGetWeekInfo => Ok(make_object(vec![
            ("firstDay".to_string(), Value::Number(1.0)),
            (
                "weekend".to_string(),
                make_array(vec![Value::Number(6.0), Value::Number(7.0)]),
            ),
        ])),
        crate::ops::Builtin::IntlLocaleBaseNameGetter => Ok(Value::String(
            slot_string(&super::intl_slots(receiver)?, "base").unwrap_or_default(),
        )),
        crate::ops::Builtin::IntlLocaleCalendarGetter
        | crate::ops::Builtin::IntlLocaleCaseFirstGetter
        | crate::ops::Builtin::IntlLocaleCollationGetter
        | crate::ops::Builtin::IntlLocaleFirstDayOfWeekGetter
        | crate::ops::Builtin::IntlLocaleHourCycleGetter
        | crate::ops::Builtin::IntlLocaleLanguageGetter
        | crate::ops::Builtin::IntlLocaleNumberingSystemGetter
        | crate::ops::Builtin::IntlLocaleNumericGetter
        | crate::ops::Builtin::IntlLocaleRegionGetter
        | crate::ops::Builtin::IntlLocaleScriptGetter
        | crate::ops::Builtin::IntlLocaleTextInfoGetter
        | crate::ops::Builtin::IntlLocaleVariantsGetter => {
            let key = match builtin {
                crate::ops::Builtin::IntlLocaleCalendarGetter => "calendar",
                crate::ops::Builtin::IntlLocaleCaseFirstGetter => "caseFirst",
                crate::ops::Builtin::IntlLocaleCollationGetter => "collation",
                crate::ops::Builtin::IntlLocaleFirstDayOfWeekGetter => "firstDayOfWeek",
                crate::ops::Builtin::IntlLocaleHourCycleGetter => "hourCycle",
                crate::ops::Builtin::IntlLocaleLanguageGetter => "language",
                crate::ops::Builtin::IntlLocaleNumberingSystemGetter => "numberingSystem",
                crate::ops::Builtin::IntlLocaleNumericGetter => "numeric",
                crate::ops::Builtin::IntlLocaleRegionGetter => "region",
                crate::ops::Builtin::IntlLocaleScriptGetter => "script",
                crate::ops::Builtin::IntlLocaleTextInfoGetter => "textInfo",
                _ => "variants",
            };
            Ok(locale_slot_value(&super::intl_slots(receiver)?, key))
        }
        _ => Err(runtime_error("TypeError: method not found")),
    }
}

fn maximize(tag: &str) -> String {
    let (base, extension) = tag
        .split_once("-u")
        .map_or((tag, String::new()), |(a, b)| (a, format!("-u{b}")));
    let parts: Vec<&str> = base.split('-').collect();
    let script = parts.iter().skip(1).find(|part| part.len() == 4).copied();
    let region = parts
        .iter()
        .skip(1)
        .find(|part| {
            part.len() == 2 || (part.len() == 3 && part.chars().all(|c| c.is_ascii_digit()))
        })
        .copied();
    let language = if parts.first().copied() == Some("und") {
        undefined_language(script, region)
    } else {
        parts.first().copied().unwrap_or("und")
    };
    let (default_script, default_region) = likely_subtags(language);
    let default_region = if language == "zh" && script == Some("Hant") {
        "TW"
    } else {
        default_region
    };
    let mut result = format!(
        "{}-{}-{}",
        language,
        script.unwrap_or(default_script),
        region.unwrap_or(default_region)
    );
    for part in parts.iter().skip(1) {
        if part.len() != 4
            && !(part.len() == 2 || (part.len() == 3 && part.chars().all(|c| c.is_ascii_digit())))
        {
            result.push('-');
            result.push_str(part);
        }
    }
    result.push_str(&extension);
    result
}

fn undefined_language(script: Option<&str>, region: Option<&str>) -> &'static str {
    match (script, region) {
        (Some("Thai"), _) => "th",
        (_, Some("419")) => "es",
        (_, Some("150")) => "en",
        (_, Some("AT")) => "de",
        (_, Some("CW")) => "pap",
        _ => "en",
    }
}

fn minimize(tag: &str) -> String {
    let maximized = maximize(tag);
    let (base, extension) = maximized
        .split_once("-u")
        .map_or((maximized.as_str(), String::new()), |(a, b)| {
            (a, format!("-u{b}"))
        });
    let parts: Vec<&str> = base.split('-').collect();
    let language = parts.first().copied().unwrap_or("und");
    let (default_script, default_region) = likely_subtags(language);
    let script = parts.get(1).copied().unwrap_or(default_script);
    let region = parts.get(2).copied().unwrap_or(default_region);
    if language == "zh" && script == "Hant" && region == "TW" {
        return format!("{language}-{region}") + &extension;
    }
    let short = if script == default_script && region == default_region {
        language.to_string()
    } else if script == default_script {
        format!("{language}-{region}")
    } else if region == default_region {
        format!("{language}-{script}")
    } else {
        format!("{language}-{script}-{region}")
    };
    short + &extension
}

fn likely_subtags(language: &str) -> (&'static str, &'static str) {
    match language {
        "aae" => ("Latn", "IT"),
        "ar" => ("Arab", "EG"),
        "de" => ("Latn", "DE"),
        "en" => ("Latn", "US"),
        "es" => ("Latn", "ES"),
        "fr" => ("Latn", "FR"),
        "ja" => ("Jpan", "JP"),
        "ko" => ("Kore", "KR"),
        "ru" => ("Cyrl", "RU"),
        "sr" => ("Cyrl", "RS"),
        "th" => ("Thai", "TH"),
        "pap" => ("Latn", "CW"),
        "zh" => ("Hans", "CN"),
        "und" => ("Latn", "US"),
        _ => ("Latn", "US"),
    }
}

fn locale_slot_value(slots: &[(String, Value)], key: &str) -> Value {
    slots
        .iter()
        .find(|(name, _)| name == key)
        .map_or(Value::Undefined, |(_, value)| value.clone())
}

pub(crate) fn dispatch(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Option<Result<Value, VmError>> {
    match builtin {
        crate::ops::Builtin::IntlLocale => Some(construct_call(arguments, receiver)),
        crate::ops::Builtin::IntlLocaleToString
        | crate::ops::Builtin::IntlLocaleMaximize
        | crate::ops::Builtin::IntlLocaleMinimize
        | crate::ops::Builtin::IntlLocaleGetCalendars
        | crate::ops::Builtin::IntlLocaleGetCollations
        | crate::ops::Builtin::IntlLocaleGetHourCycles
        | crate::ops::Builtin::IntlLocaleGetNumberingSystems
        | crate::ops::Builtin::IntlLocaleGetTimeZones
        | crate::ops::Builtin::IntlLocaleGetTextInfo
        | crate::ops::Builtin::IntlLocaleGetWeekInfo
        | crate::ops::Builtin::IntlLocaleBaseNameGetter
        | crate::ops::Builtin::IntlLocaleCalendarGetter
        | crate::ops::Builtin::IntlLocaleCaseFirstGetter
        | crate::ops::Builtin::IntlLocaleCollationGetter
        | crate::ops::Builtin::IntlLocaleFirstDayOfWeekGetter
        | crate::ops::Builtin::IntlLocaleHourCycleGetter
        | crate::ops::Builtin::IntlLocaleLanguageGetter
        | crate::ops::Builtin::IntlLocaleNumberingSystemGetter
        | crate::ops::Builtin::IntlLocaleNumericGetter
        | crate::ops::Builtin::IntlLocaleRegionGetter
        | crate::ops::Builtin::IntlLocaleScriptGetter
        | crate::ops::Builtin::IntlLocaleTextInfoGetter
        | crate::ops::Builtin::IntlLocaleVariantsGetter => {
            Some(prototype_method(builtin, receiver))
        }
        _ => None,
    }
}
