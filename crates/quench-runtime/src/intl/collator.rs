//! `Intl.Collator`.

use crate::{execute::VmError, value::Value};

use super::{
    default_locale, make_object, resolve_locales, runtime_error, slot_bool, slot_string,
    to_string_value, SLOT,
};

pub(crate) fn construct(arguments: &[Value]) -> Result<Value, VmError> {
    let locales = resolve_locales(arguments)?;
    let mut locale = locales.first().cloned().unwrap_or_else(default_locale);
    let options = CollatorOptions::read(arguments.get(1), &mut locale)?;
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

struct CollatorOptions {
    usage: String,
    sensitivity: String,
    ignore_punctuation: bool,
    numeric: bool,
    case_first: String,
}

impl CollatorOptions {
    fn read(value: Option<&Value>, locale: &mut String) -> Result<Self, VmError> {
        let mut options = Self {
            usage: "sort".to_string(),
            sensitivity: "variant".to_string(),
            ignore_punctuation: locale.starts_with("th"),
            numeric: false,
            case_first: "false".to_string(),
        };
        let Some(Value::Object(properties)) = value else {
            return Ok(options);
        };
        if let Some((_, value)) = properties.iter().find(|(name, _)| name == "usage") {
            options.usage = to_string_value(value);
            if !matches!(options.usage.as_str(), "sort" | "search") {
                return Err(runtime_error("RangeError: invalid usage"));
            }
        }
        if let Some((_, Value::Boolean(value))) =
            properties.iter().find(|(name, _)| name == "numeric")
        {
            options.numeric = *value;
        }
        if let Some((_, value)) = properties.iter().find(|(name, _)| name == "caseFirst") {
            options.case_first = to_string_value(value);
            if !matches!(options.case_first.as_str(), "upper" | "lower" | "false") {
                return Err(runtime_error("RangeError: invalid caseFirst"));
            }
        }
        if let Some((_, value)) = properties.iter().find(|(name, _)| name == "sensitivity") {
            options.sensitivity = to_string_value(value);
            if !matches!(
                options.sensitivity.as_str(),
                "base" | "accent" | "case" | "variant"
            ) {
                return Err(runtime_error("RangeError: invalid sensitivity"));
            }
        }
        if options.case_first != "false" {
            *locale = remove_conflicting_extension(locale, "kf", &options.case_first);
        }
        if options.numeric {
            *locale = remove_conflicting_extension(locale, "kn", "true");
        }
        if let Some((_, Value::Boolean(value))) = properties
            .iter()
            .find(|(name, _)| name == "ignorePunctuation")
        {
            options.ignore_punctuation = *value;
        }
        Ok(options)
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
