//! `Intl.Collator`.

use crate::{execute::VmError, value::Value};

use super::{
    default_locale, make_object, resolve_locales, runtime_error, slot_bool, slot_string,
    to_string_value, SLOT,
};

pub(crate) fn construct(arguments: &[Value]) -> Result<Value, VmError> {
    let locales = resolve_locales(arguments)?;
    let locale = locales.first().cloned().unwrap_or_else(default_locale);
    let mut usage = "sort".to_string();
    let mut sensitivity = "variant".to_string();
    let mut ignore_punctuation = locale.starts_with("th");
    if let Some(Value::Object(properties)) = arguments.get(1) {
        if let Some((_, value)) = properties.iter().find(|(name, _)| name == "usage") {
            usage = to_string_value(value);
            if !matches!(usage.as_str(), "sort" | "search") {
                return Err(runtime_error("RangeError: invalid usage"));
            }
        }
        if let Some((_, value)) = properties.iter().find(|(name, _)| name == "sensitivity") {
            sensitivity = to_string_value(value);
            if !matches!(sensitivity.as_str(), "base" | "accent" | "case" | "variant") {
                return Err(runtime_error("RangeError: invalid sensitivity"));
            }
        }
        if let Some((_, Value::Boolean(value))) = properties
            .iter()
            .find(|(name, _)| name == "ignorePunctuation")
        {
            ignore_punctuation = *value;
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
                ("sensitivity".to_string(), Value::String(sensitivity)),
                (
                    "ignorePunctuation".to_string(),
                    Value::Boolean(ignore_punctuation),
                ),
            ]),
        ),
    ]))
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
            ("numeric".to_string(), Value::Boolean(false)),
            ("caseFirst".to_string(), Value::String("false".to_string())),
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
