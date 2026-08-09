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
    let tag = to_string_value(tag_arg);
    let canonical = canonicalize(&tag)?;
    let locale = parse_canonical(&canonical);
    let options = arguments.get(1);
    let locale = apply_options(locale, options)?;
    Ok(build_object(locale))
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
                        locale.calendar =
                            Some(canonicalize(item).unwrap_or_else(|_| item.to_string()))
                    }
                    "co" => locale.collation = Some(item.to_string()),
                    "kf" => locale.case_first = Some(item.to_string()),
                    "hc" => locale.hour_cycle = Some(item.to_string()),
                    "nu" => locale.numbering_system = Some(item.to_string()),
                    _ => {}
                }
                j += 2;
            }
            break;
        }
        i += 1;
    }
}

fn apply_options(mut locale: Locale, options: Option<&Value>) -> Result<Locale, VmError> {
    let Some(Value::Object(properties)) = options else {
        return Ok(locale);
    };
    for (key, value) in properties.iter() {
        let value = to_string_value(value);
        match key.as_str() {
            "calendar" => locale.calendar = Some(value),
            "collation" => locale.collation = Some(value),
            "caseFirst" => locale.case_first = Some(value),
            "hourCycle" => locale.hour_cycle = normalize_hour_cycle(&value),
            "numberingSystem" => locale.numbering_system = Some(value),
            "numeric" => locale.numeric = value == "true",
            _ => {}
        }
    }
    Ok(locale)
}

fn normalize_hour_cycle(value: &str) -> Option<String> {
    match value {
        "h11" | "h12" | "h23" | "h24" => Some(value.to_string()),
        _ => None,
    }
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
    let mut properties = vec![(
        "language".to_string(),
        Value::String(locale.language.clone()),
    )];
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

pub(crate) fn prototype_method(
    builtin: crate::ops::Builtin,
    receiver: Option<&Value>,
) -> Result<Value, VmError> {
    let slot = slot_string(&super::intl_slots(receiver)?, "full").unwrap_or_default();
    match builtin {
        crate::ops::Builtin::IntlLocaleToString => Ok(Value::String(slot)),
        crate::ops::Builtin::IntlLocaleMaximize => Ok(Value::String(slot)),
        crate::ops::Builtin::IntlLocaleMinimize => Ok(Value::String(slot)),
        crate::ops::Builtin::IntlLocaleGetCalendars => {
            Ok(make_array(vec![Value::String("gregory".to_string())]))
        }
        crate::ops::Builtin::IntlLocaleGetCollations => {
            Ok(make_array(vec![Value::String("default".to_string())]))
        }
        crate::ops::Builtin::IntlLocaleGetHourCycles => Ok(make_array(vec![])),
        crate::ops::Builtin::IntlLocaleGetNumberingSystems => {
            Ok(make_array(vec![Value::String("latn".to_string())]))
        }
        crate::ops::Builtin::IntlLocaleGetTimeZones => Ok(make_array(vec![])),
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
            ("minimalDays".to_string(), Value::Number(1.0)),
        ])),
        _ => Err(runtime_error("TypeError: method not found")),
    }
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
        | crate::ops::Builtin::IntlLocaleGetWeekInfo => Some(prototype_method(builtin, receiver)),
        _ => None,
    }
}
