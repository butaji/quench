//! `Intl.Collator`.

use crate::{execute::VmError, value::Value};
use unicode_normalization::UnicodeNormalization;

use super::{
    default_locale, make_object, resolve_locales, runtime_error, slot_bool, slot_string,
    to_string_value, SLOT,
};

pub(crate) fn construct(arguments: &[Value]) -> Result<Value, VmError> {
    let locales = resolve_locales(arguments)?;
    let raw_locale = locales.first().cloned().unwrap_or_else(default_locale);
    let (mut locale, extension) = normalize_locale_extensions(&raw_locale);
    let mut usage = "sort".to_string();
    let mut sensitivity = "variant".to_string();
    let mut ignore_punctuation = locale.starts_with("th");
    let mut numeric = extension.numeric;
    let mut case_first = extension.case_first.unwrap_or_else(|| "false".to_string());
    let mut collation = extension.collation.unwrap_or_else(|| "default".to_string());
    if let Some(Value::Object(properties)) = arguments.get(1) {
        if let Some((_, value)) = properties.iter().find(|(name, _)| name == "usage") {
            usage = crate::conversion::to_string(value)?;
            if !matches!(usage.as_str(), "sort" | "search") {
                return Err(runtime_error("RangeError: invalid usage"));
            }
        }
        if let Some((_, value)) = properties.iter().find(|(name, _)| name == "numeric") {
            numeric = truthy_option(value);
        }
        if let Some((_, value)) = properties.iter().find(|(name, _)| name == "caseFirst") {
            case_first = crate::conversion::to_string(value)?;
            if !matches!(case_first.as_str(), "upper" | "lower" | "false") {
                return Err(runtime_error("RangeError: invalid caseFirst"));
            }
        }
        if let Some((_, value)) = properties.iter().find(|(name, _)| name == "sensitivity") {
            sensitivity = crate::conversion::to_string(value)?;
            if !matches!(sensitivity.as_str(), "base" | "accent" | "case" | "variant") {
                return Err(runtime_error("RangeError: invalid sensitivity"));
            }
        }
        if let Some((_, value)) = properties.iter().find(|(name, _)| name == "collation") {
            collation = crate::conversion::to_string(value)?;
            if !matches!(
                collation.as_str(),
                "default" | "search" | "standard" | "phonebk" | "pinyin" | "eor"
            ) {
                return Err(runtime_error("RangeError: invalid collation"));
            }
            if collation == "pinyin" {
                collation = normalize_locale_extensions(&raw_locale)
                    .1
                    .collation
                    .unwrap_or_else(|| "default".to_string());
            }
            if collation == "eor" {
                locale = strip_unicode_key(&locale, "co");
            } else if collation == "default" {
                locale = strip_unicode_key(&locale, "co");
            }
        }
        if case_first != "false" {
            locale = remove_conflicting_extension(&locale, "kf", &case_first);
        }
        if numeric {
            locale = remove_conflicting_extension(&locale, "kn", "true");
        }
        if let Some((_, value)) = properties
            .iter()
            .find(|(name, _)| name == "ignorePunctuation")
        {
            ignore_punctuation = truthy_option(value);
        }
        if let Some((_, value)) = properties.iter().find(|(name, _)| name == "localeMatcher") {
            let matcher = crate::conversion::to_string(value)?;
            if !matches!(matcher.as_str(), "lookup" | "best fit") {
                return Err(runtime_error("RangeError: invalid localeMatcher"));
            }
        }
    }
    Ok(make_object(vec![
        (
            "compare".to_string(),
            Value::Builtin(crate::ops::Builtin::IntlCollatorCompare),
        ),
        (
            "resolvedOptions".to_string(),
            Value::Builtin(crate::ops::Builtin::IntlCollatorResolvedOptions),
        ),
        (
            SLOT.to_string(),
            make_object(vec![
                ("locale".to_string(), Value::String(locale)),
                ("usage".to_string(), Value::String(usage)),
                ("numeric".to_string(), Value::Boolean(numeric)),
                ("caseFirst".to_string(), Value::String(case_first)),
                ("sensitivity".to_string(), Value::String(sensitivity)),
                (
                    "ignorePunctuation".to_string(),
                    Value::Boolean(ignore_punctuation),
                ),
                ("collation".to_string(), Value::String(collation)),
            ]),
        ),
        (
            "\0prototype".to_string(),
            Value::Builtin(crate::ops::Builtin::IntlCollatorPrototype),
        ),
    ]))
}

fn remove_conflicting_extension(locale: &str, key: &str, value: &str) -> String {
    let parts: Vec<&str> = locale.split('-').collect();
    let Some(index) = parts.iter().position(|part| part.eq_ignore_ascii_case(key)) else {
        return locale.to_string();
    };
    let next = parts.get(index + 1).copied();
    let extension_value = next.filter(|part| part.len() != 2).unwrap_or("true");
    if extension_value.eq_ignore_ascii_case(value) {
        return locale.to_string();
    }
    let remove_count = usize::from(next.is_some_and(|part| part.len() != 2)) + 1;
    let result = parts[..index]
        .iter()
        .chain(parts.get(index + remove_count..).unwrap_or_default().iter())
        .copied()
        .collect::<Vec<_>>()
        .join("-")
        .trim_end_matches("-u")
        .to_string();
    result
}

struct LocaleExtensions {
    numeric: bool,
    case_first: Option<String>,
    collation: Option<String>,
}

fn normalize_locale_extensions(locale: &str) -> (String, LocaleExtensions) {
    let parts = locale.split('-').collect::<Vec<_>>();
    let Some(index) = parts.iter().position(|part| *part == "u") else {
        return (
            locale.to_string(),
            LocaleExtensions {
                numeric: false,
                case_first: None,
                collation: None,
            },
        );
    };
    let base = parts[..index].join("-");
    let extension = &parts[index + 1..];
    let mut numeric = false;
    let mut case_first = None;
    let mut collation = None;
    let mut retained = Vec::new();
    let mut cursor = 0;
    while cursor < extension.len() {
        let key = extension[cursor];
        if key.len() != 2 {
            cursor += 1;
            continue;
        }
        let value = extension.get(cursor + 1).copied();
        match key {
            "kn" => {
                numeric =
                    value.is_none_or(|item| item.len() == 1 || item.len() == 2 || item == "true");
                if value != Some("false") {
                    retained.push("kn");
                }
            }
            "kf" if matches!(value, Some("upper" | "lower" | "false")) => {
                case_first = value.map(str::to_string);
                retained.extend(["kf", value.unwrap_or("false")]);
            }
            "co" if base.starts_with("de")
                && matches!(value, Some("phonebk" | "dict" | "ducet" | "pinyin")) =>
            {
                collation = value.map(str::to_string);
                retained.extend(["co", value.unwrap_or("")]);
            }
            _ => {}
        }
        cursor += if value.is_some_and(|item| item.len() > 2) {
            2
        } else {
            1
        };
    }
    let normalized = if retained.is_empty() {
        base
    } else {
        format!("{base}-u-{}", retained.join("-"))
    };
    (
        normalized,
        LocaleExtensions {
            numeric,
            case_first,
            collation,
        },
    )
}

fn strip_unicode_key(locale: &str, key: &str) -> String {
    let parts = locale.split('-').collect::<Vec<_>>();
    let Some(index) = parts.iter().position(|part| *part == key) else {
        return locale.to_string();
    };
    parts[..index]
        .iter()
        .chain(parts.get(index + 2..).unwrap_or_default().iter())
        .copied()
        .collect::<Vec<_>>()
        .join("-")
        .trim_end_matches("-u")
        .to_string()
}

fn truthy_option(value: &Value) -> bool {
    match value {
        Value::Undefined | Value::Null => false,
        Value::Boolean(value) => *value,
        Value::Number(value) => *value != 0.0 && !value.is_nan(),
        Value::String(value) => !value.is_empty(),
        _ => true,
    }
}

pub(crate) fn prototype_method(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Result<Value, VmError> {
    let slots = receiver_slots(receiver)?;
    let locale = slot_string(&slots, "locale").unwrap_or_else(default_locale);
    match builtin {
        crate::ops::Builtin::IntlCollatorCompare => {
            let left = to_string_value(arguments.first().unwrap_or(&Value::Undefined));
            let right = to_string_value(arguments.get(1).unwrap_or(&Value::Undefined));
            Ok(Value::Number(compare(&left, &right, &locale, &slots)))
        }
        crate::ops::Builtin::IntlCollatorResolvedOptions => Ok(make_object(vec![
            ("locale".to_string(), Value::String(locale)),
            (
                "usage".to_string(),
                Value::String(slot_string(&slots, "usage").unwrap_or_else(|| "sort".to_string())),
            ),
            (
                "sensitivity".to_string(),
                Value::String(
                    slot_string(&slots, "sensitivity").unwrap_or_else(|| "variant".to_string()),
                ),
            ),
            (
                "ignorePunctuation".to_string(),
                Value::Boolean(slot_bool(&slots, "ignorePunctuation").unwrap_or(false)),
            ),
            (
                "collation".to_string(),
                Value::String(
                    slot_string(&slots, "collation").unwrap_or_else(|| "default".to_string()),
                ),
            ),
            (
                "numeric".to_string(),
                Value::Boolean(slot_bool(&slots, "numeric").unwrap_or(false)),
            ),
            (
                "caseFirst".to_string(),
                Value::String(
                    slot_string(&slots, "caseFirst").unwrap_or_else(|| "false".to_string()),
                ),
            ),
        ])),
        _ => Err(runtime_error("TypeError: method not found")),
    }
}

fn receiver_slots(receiver: Option<&Value>) -> Result<Vec<(String, Value)>, VmError> {
    super::intl_slots(receiver)
}

fn compare(left: &str, right: &str, locale: &str, slots: &[(String, Value)]) -> f64 {
    let _ = locale;
    let left = comparison_key(left, slots);
    let right = comparison_key(right, slots);
    match left.cmp(&right) {
        std::cmp::Ordering::Less => -1.0,
        std::cmp::Ordering::Equal => 0.0,
        std::cmp::Ordering::Greater => 1.0,
    }
}

fn comparison_key(value: &str, slots: &[(String, Value)]) -> String {
    let ignore_punctuation = slot_bool(slots, "ignorePunctuation").unwrap_or(false);
    let sensitivity = slot_string(slots, "sensitivity").unwrap_or_else(|| "variant".to_string());
    let normalized = value.nfd();
    let mut key = String::new();
    for ch in normalized {
        if ignore_punctuation && !ch.is_alphanumeric() {
            continue;
        }
        if matches!(ch, '\u{0300}'..='\u{036f}') {
            continue;
        }
        key.push(if matches!(sensitivity.as_str(), "base" | "accent") {
            ch.to_ascii_lowercase()
        } else {
            ch
        });
    }
    key
}

pub(crate) fn dispatch(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Option<Result<Value, VmError>> {
    match builtin {
        crate::ops::Builtin::IntlCollator => Some(construct(arguments)),
        crate::ops::Builtin::IntlCollatorCompare
        | crate::ops::Builtin::IntlCollatorResolvedOptions => {
            Some(prototype_method(builtin, arguments, receiver))
        }
        _ => None,
    }
}
