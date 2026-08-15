//! `Intl.Locale` constructor and prototype methods.

use crate::{execute::VmError, value::Value};

use super::{canonicalize, make_array, make_object, runtime_error, slot_string, to_string_value};

mod locale_methods;
pub(crate) use locale_methods::prototype_method;

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
                            locale.calendar = Some(calendar_alias(item));
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
                    _ => {}
                }
                j += 2;
            }
            break;
        }
        i += 1;
    }
}

fn calendar_alias(value: &str) -> String {
    match value {
        "islamicc" => "islamic-civil".to_string(),
        other => canonicalize(other).unwrap_or_else(|_| other.to_string()),
    }
}

fn apply_options(mut locale: Locale, options: Option<&Value>) -> Result<Locale, VmError> {
    if matches!(options, Some(Value::Null)) {
        return Err(crate::value::error::throw_type_error(
            "Cannot convert null or undefined to object",
        ));
    }
    let Some(Value::Object(properties)) = options else {
        return Ok(locale);
    };
    for (key, value) in properties.iter() {
        if matches!(value, Value::Undefined) {
            continue;
        }
        let text = crate::conversion::to_string(value)?;
        match key.as_str() {
            "calendar" => {
                let value = option_value(&text, "calendar")?;
                locale.calendar = Some(calendar_alias(&value));
            }
            "collation" => locale.collation = Some(option_value(&text, "collation")?),
            "caseFirst" => locale.case_first = Some(normalize_case_first(&text)?),
            "hourCycle" => locale.hour_cycle = Some(normalize_hour_cycle(&text)?),
            "numberingSystem" => {
                locale.numbering_system = Some(option_value(&text, "numberingSystem")?)
            }
            "numeric" => locale.numeric = normalize_numeric(value, &text)?,
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
            full.push('-');
            full.push_str(&item);
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
        keys.push(("kn", "true".to_string()));
    }
    keys
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
        crate::ops::Builtin::IntlLocale => Some(construct(arguments)),
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
