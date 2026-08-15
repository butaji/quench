//! `Intl.Collator`.

use crate::{execute::VmError, value::Value};

use super::{
    default_locale, make_object, resolve_locales, runtime_error, slot_bool, slot_string,
    to_string_value, SLOT,
};

pub(crate) fn construct(arguments: &[Value]) -> Result<Value, VmError> {
    let locales = resolve_locales(arguments)?;
    let mut locale = locales.first().cloned().unwrap_or_else(default_locale);
    let mut usage = "sort".to_string();
    let mut sensitivity = "variant".to_string();
    let mut ignore_punctuation = locale.starts_with("th");
    let mut numeric = false;
    let mut case_first = "false".to_string();
    apply_options(
        arguments.get(1),
        &mut locale,
        &mut usage,
        &mut sensitivity,
        &mut ignore_punctuation,
        &mut numeric,
        &mut case_first,
    )?;
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
                ("usage".to_string(), Value::String(options.usage)),
                ("numeric".to_string(), Value::Boolean(options.numeric)),
                ("caseFirst".to_string(), Value::String(options.case_first)),
                (
                    "sensitivity".to_string(),
                    Value::String(options.sensitivity),
                ),
                (
                    "ignorePunctuation".to_string(),
                    Value::Boolean(options.ignore_punctuation),
                ),
            ]),
        ),
    ]))
}

fn apply_options(
    options: Option<&Value>,
    locale: &mut String,
    usage: &mut String,
    sensitivity: &mut String,
    ignore_punctuation: &mut bool,
    numeric: &mut bool,
    case_first: &mut String,
) -> Result<(), VmError> {
    let Some(Value::Object(properties)) = options else {
        return Ok(());
    };
    if let Some((_, value)) = properties.iter().find(|(name, _)| name == "usage") {
        *usage = to_string_value(value);
        validate_option(usage, &["sort", "search"], "usage")?;
    }
    if let Some((_, Value::Boolean(value))) = properties.iter().find(|(name, _)| name == "numeric")
    {
        *numeric = *value;
    }
    if let Some((_, value)) = properties.iter().find(|(name, _)| name == "caseFirst") {
        *case_first = to_string_value(value);
        validate_option(case_first, &["upper", "lower", "false"], "caseFirst")?;
    }
    if let Some((_, value)) = properties.iter().find(|(name, _)| name == "sensitivity") {
        *sensitivity = to_string_value(value);
        validate_option(
            sensitivity,
            &["base", "accent", "case", "variant"],
            "sensitivity",
        )?;
    }
    update_locale(locale, case_first, *numeric);
    if let Some((_, Value::Boolean(value))) = properties
        .iter()
        .find(|(name, _)| name == "ignorePunctuation")
    {
        *ignore_punctuation = *value;
    }
    Ok(())
}

fn validate_option(value: &str, allowed: &[&str], name: &str) -> Result<(), VmError> {
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(runtime_error(&format!("RangeError: invalid {name}")))
    }
}

fn update_locale(locale: &mut String, case_first: &str, numeric: bool) {
    if case_first != "false" {
        *locale = remove_conflicting_extension(locale, "kf", case_first);
    }
    if numeric {
        *locale = remove_conflicting_extension(locale, "kn", "true");
    }
}

fn remove_conflicting_extension(locale: &str, key: &str, value: &str) -> String {
    let parts: Vec<&str> = locale.split('-').collect();
    let Some(index) = parts.iter().position(|part| part.eq_ignore_ascii_case(key)) else {
        return locale.to_string();
    };
    let extension_value = parts.get(index + 1).copied().unwrap_or("true");
    if extension_value.eq_ignore_ascii_case(value) {
        return locale.to_string();
    }
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
    let Some(index) = unicode_extension_index(&parts) else {
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
                numeric = value.map_or(true, |item| {
                    item.len() == 1 || item.len() == 2 || item == "true"
                });
                if value != Some("false") {
                    retained.push("kn");
                }
            }
            "kf" if matches!(value, Some("upper" | "lower" | "false")) => {
                case_first = value.map(str::to_string);
                retained.extend(["kf", value.unwrap_or("false")]);
            }
            "co" if base.starts_with("de")
                && matches!(value, Some("phonebk" | "dict" | "ducet")) =>
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

fn unicode_extension_index(parts: &[&str]) -> Option<usize> {
    parts
        .iter()
        .position(|part| *part == "u")
        .filter(|index| !parts[..*index].contains(&"x"))
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
        .to_string();
    result
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
            Ok(Value::Number(compare(&left, &right, &locale)))
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
                Value::String("default".to_string()),
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

fn compare(left: &str, right: &str, locale: &str) -> f64 {
    let _ = locale;
    match left.cmp(right) {
        std::cmp::Ordering::Less => -1.0,
        std::cmp::Ordering::Equal => 0.0,
        std::cmp::Ordering::Greater => 1.0,
    }
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
