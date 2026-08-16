//! `Intl.Locale` constructor and prototype methods.

use crate::{execute::VmError, value::Value};

use super::{canonicalize, make_array, make_object, runtime_error, slot_string};

mod locale_methods;
pub(crate) use locale_methods::prototype_method;

pub(crate) struct Locale {
    pub language: String,
    pub script: Option<String>,
    pub region: Option<String>,
    pub variants: Vec<String>,
    pub calendar: Option<String>,
    pub collation: Option<String>,
    pub case_first: Option<String>,
    pub hour_cycle: Option<String>,
    pub first_day_of_week: Option<String>,
    pub numbering_system: Option<String>,
    pub numeric: bool,
    pub numeric_explicit: bool,
    pub unicode_extensions: Vec<UnicodeExtension>,
    pub other_extensions: Vec<OtherExtension>,
}

#[derive(Clone)]
pub(crate) struct UnicodeExtension {
    pub attributes: Vec<String>,
    pub key: String,
    pub types: Vec<String>,
}

#[derive(Clone)]
pub(crate) struct OtherExtension {
    pub singleton: String,
    pub subtags: Vec<String>,
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
        for variant in &self.variants {
            base.push('-');
            base.push_str(variant);
        }
        base
    }
}

pub(crate) fn construct(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(tag_arg) = arguments.first() else {
        return Err(crate::value::error::throw_type_error(
            "Locale requires a tag",
        ));
    };
    let tag = locale_tag(tag_arg)?;
    let options = arguments.get(1);
    let has_options = options.is_some_and(|value| !matches!(value, Value::Undefined));
    let grandfathered = matches!(
        tag.to_ascii_lowercase().as_str(),
        "art-lojban" | "cel-gaulish"
    );
    let canonical = if grandfathered && has_options {
        tag.clone()
    } else {
        canonicalize(&tag)?
    };
    let language = canonical.split('-').next().unwrap_or_default();
    if !valid_language_subtag(language) {
        return Err(runtime_error("RangeError: invalid language tag"));
    }
    let locale = parse_canonical(&canonical);
    let mut locale = apply_options(locale, options)?;
    if grandfathered && has_options {
        locale.variants.clear();
    }
    Ok(build_object(locale))
}

fn valid_language_subtag(language: &str) -> bool {
    matches!(language.len(), 2 | 3 | 5..=8)
        && language
            .chars()
            .all(|character| character.is_ascii_alphabetic())
}

fn locale_tag(value: &Value) -> Result<String, VmError> {
    match value {
        Value::String(_) | Value::StringUnits(_) => crate::conversion::to_string(value),
        Value::Object(_) => crate::conversion::to_string(value),
        _ => Err(runtime_error("TypeError: locale tag must be a string")),
    }
}

fn parse_canonical(tag: &str) -> Locale {
    let parts: Vec<&str> = tag.split('-').collect();
    let mut locale = Locale {
        language: parts.first().map(|p| p.to_string()).unwrap_or_default(),
        script: None,
        region: None,
        variants: Vec::new(),
        calendar: None,
        collation: None,
        case_first: None,
        hour_cycle: None,
        first_day_of_week: None,
        numbering_system: None,
        numeric: false,
        numeric_explicit: false,
        unicode_extensions: Vec::new(),
        other_extensions: Vec::new(),
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
    while let Some(part) = parts.get(index) {
        if part.len() < 4 || part.len() > 8 || !part.chars().all(|c| c.is_ascii_alphanumeric()) {
            break;
        }
        locale.variants.push((*part).to_string());
        index += 1;
    }
    locale.variants.sort_by(|left, right| {
        let left_numeric = left.chars().next().is_some_and(|c| c.is_ascii_digit());
        let right_numeric = right.chars().next().is_some_and(|c| c.is_ascii_digit());
        right_numeric
            .cmp(&left_numeric)
            .then_with(|| left.cmp(right))
    });
    locale.unicode_extensions = parse_unicode_extensions(&parts[index..]);
    locale.other_extensions = parse_other_extensions(&parts[index..]);
    parse_extensions(&mut locale, &parts[index..]);
    locale
}

fn parse_other_extensions(parts: &[&str]) -> Vec<OtherExtension> {
    let mut extensions = Vec::new();
    let mut index = 0;
    while index < parts.len() {
        if parts[index] == "u" {
            index += 1;
            while index < parts.len() && parts[index].len() != 1 {
                index += 1;
            }
            continue;
        }
        if parts[index].len() != 1 {
            index += 1;
            continue;
        }
        let singleton = parts[index].to_string();
        index += 1;
        let start = index;
        if singleton == "x" {
            extensions.push(OtherExtension {
                singleton,
                subtags: parts[start..]
                    .iter()
                    .map(|part| (*part).to_string())
                    .collect(),
            });
            break;
        }
        while index < parts.len() && parts[index].len() != 1 {
            index += 1;
        }
        extensions.push(OtherExtension {
            singleton,
            subtags: parts[start..index]
                .iter()
                .map(|part| (*part).to_string())
                .collect(),
        });
    }
    extensions
}

fn parse_unicode_extensions(parts: &[&str]) -> Vec<UnicodeExtension> {
    let Some(start) = parts
        .iter()
        .position(|part| *part == "u")
        .filter(|start| !parts[..*start].contains(&"x"))
    else {
        return Vec::new();
    };
    let mut extensions = Vec::new();
    let mut index = start + 1;
    let mut attributes = Vec::new();
    while index < parts.len() && parts[index].len() != 2 && parts[index].len() != 1 {
        attributes.push(parts[index].to_string());
        index += 1;
    }
    attributes.sort();
    if !attributes.is_empty() {
        extensions.push(UnicodeExtension {
            attributes: attributes.clone(),
            key: String::new(),
            types: Vec::new(),
        });
    }
    while index < parts.len() && parts[index].len() != 1 {
        if parts[index].len() != 2 {
            index += 1;
            continue;
        }
        let key = parts[index].to_string();
        index += 1;
        let value_start = index;
        while index < parts.len() && parts[index].len() != 2 && parts[index].len() != 1 {
            index += 1;
        }
        if extensions
            .iter()
            .any(|extension: &UnicodeExtension| extension.key == key)
        {
            continue;
        }
        extensions.push(UnicodeExtension {
            attributes: Vec::new(),
            key,
            types: parts[value_start..index]
                .iter()
                .map(|part| (*part).to_string())
                .collect(),
        });
        attributes.clear();
    }
    extensions.sort_by(|left, right| left.key.cmp(&right.key));
    extensions
}

pub(crate) fn case_first_extension(tag: &str) -> Option<String> {
    parse_canonical(tag).case_first
}

fn parse_extensions(locale: &mut Locale, parts: &[&str]) {
    let mut i = 0;
    while i < parts.len() {
        if parts[i] == "u" {
            let mut j = i + 1;
            while j + 1 < parts.len() {
                let key = parts[j];
                let item = parts[j + 1];
                if key == "kn" {
                    locale.numeric = item != "false";
                    locale.numeric_explicit = true;
                    j += if matches!(item, "true" | "false") {
                        2
                    } else {
                        1
                    };
                    continue;
                }
                match key {
                    "ca" => {
                        if locale.calendar.is_none() {
                            let calendar = if item == "ethiopic"
                                && parts.get(j + 2) == Some(&"amete")
                                && parts.get(j + 3) == Some(&"alem")
                            {
                                "ethiopic-amete-alem"
                            } else {
                                item
                            };
                            locale.calendar = Some(calendar_alias(calendar));
                        }
                    }
                    "co" if locale.collation.is_none() => locale.collation = Some(item.to_string()),
                    "kf" if locale.case_first.is_none() => {
                        locale.case_first = Some(match item {
                            "upper" | "lower" | "false" => item.to_string(),
                            _ => String::new(),
                        })
                    }
                    "hc" if locale.hour_cycle.is_none() => {
                        locale.hour_cycle = Some(item.to_string())
                    }
                    "fw" if locale.first_day_of_week.is_none() => {
                        locale.first_day_of_week = Some(item.to_string())
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

pub(crate) fn calendar_alias(value: &str) -> String {
    let value = value.to_ascii_lowercase();
    match value.as_str() {
        "islamicc" => "islamic-civil".to_string(),
        "islamic" | "islamic-rgsa" => "islamic-civil".to_string(),
        "ethiopic-amete-alem" => "ethioaa".to_string(),
        other => other.to_string(),
    }
}

include!("locale_options.rs");

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

include!("locale_slots.rs");

pub(crate) fn dispatch(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Option<Result<Value, VmError>> {
    match builtin {
        crate::ops::Builtin::IntlLocale => Some(match receiver {
            None => construct(arguments),
            Some(_) => Err(runtime_error("TypeError: Intl.Locale requires new")),
        }),
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
