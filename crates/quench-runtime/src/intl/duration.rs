//! `Intl.DurationFormat` core duration formatting.

use crate::{execute::VmError, ops::Builtin, value::Value};

use super::{default_locale, make_array, make_object, resolve_locales, runtime_error, SLOT};

pub(crate) fn dispatch(
    builtin: Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Option<Result<Value, VmError>> {
    match builtin {
        Builtin::IntlDurationFormat => Some(construct(arguments)),
        Builtin::IntlDurationFormatFormat
        | Builtin::IntlDurationFormatFormatToParts
        | Builtin::IntlDurationFormatResolvedOptions => Some(method(builtin, arguments, receiver)),
        _ => None,
    }
}

fn construct(arguments: &[Value]) -> Result<Value, VmError> {
    let locale = resolve_locales(arguments)?
        .first()
        .cloned()
        .unwrap_or_else(default_locale);
    let options = arguments.get(1);
    if let Some(value) = options {
        if !matches!(value, Value::Object(_) | Value::Proxy(_) | Value::Undefined) {
            return Err(crate::value::error::throw_type_error(
                "DurationFormat options must be an object",
            ));
        }
    }
    validate_option(options, "localeMatcher", &["lookup", "best fit"])?;
    if let Some(numbering) = option(options, "numberingSystem")? {
        if !(3..=8).contains(&numbering.len())
            || !numbering.chars().all(|ch| ch.is_ascii_alphanumeric())
        {
            return Err(runtime_error("RangeError: invalid numberingSystem"));
        }
    }
    if let Some(fractional) = option(options, "fractionalDigits")? {
        let digits = fractional
            .parse::<i64>()
            .map_err(|_| runtime_error("RangeError: invalid fractionalDigits"))?;
        if !(0..=9).contains(&digits) {
            return Err(runtime_error("RangeError: invalid fractionalDigits"));
        }
    }
    let style = option(options, "style")?.unwrap_or_else(|| "short".to_string());
    if !matches!(style.as_str(), "long" | "short" | "narrow" | "digital") {
        return Err(runtime_error("RangeError: invalid style"));
    }
    let mut resolved = vec![
        ("locale".to_string(), Value::String(locale)),
        (
            "numberingSystem".to_string(),
            Value::String(numbering_system(options)),
        ),
        ("style".to_string(), Value::String(style)),
    ];
    for unit in [
        "years",
        "months",
        "weeks",
        "days",
        "hours",
        "minutes",
        "seconds",
        "milliseconds",
        "microseconds",
        "nanoseconds",
    ] {
        let value = option(options, unit)?.unwrap_or_else(|| "short".to_string());
        if !valid_unit_style(unit, &value) {
            return Err(runtime_error("RangeError: invalid unit style"));
        }
        resolved.push((unit.to_string(), Value::String(value)));
        resolved.push((format!("{unit}Display"), Value::String("auto".to_string())));
    }
    if let Some(value) = option(options, "fractionalDigits")? {
        resolved.push((
            "fractionalDigits".to_string(),
            Value::Number(value.parse::<f64>().unwrap_or(0.0)),
        ));
    }
    Ok(make_object(vec![
        (
            "format".to_string(),
            Value::Builtin(Builtin::IntlDurationFormatFormat),
        ),
        (
            "formatToParts".to_string(),
            Value::Builtin(Builtin::IntlDurationFormatFormatToParts),
        ),
        (
            "resolvedOptions".to_string(),
            Value::Builtin(Builtin::IntlDurationFormatResolvedOptions),
        ),
        (SLOT.to_string(), make_object(resolved)),
        (
            "\0prototype".to_string(),
            Value::Builtin(Builtin::IntlDurationFormatPrototype),
        ),
    ]))
}

fn validate_option(options: Option<&Value>, key: &str, allowed: &[&str]) -> Result<(), VmError> {
    let Some(value) = option(options, key)? else {
        return Ok(());
    };
    if allowed.contains(&value.as_str()) {
        Ok(())
    } else {
        Err(runtime_error("RangeError: invalid option"))
    }
}

fn numbering_system(options: Option<&Value>) -> String {
    let Some(Value::Object(properties)) = options else {
        return "latn".to_string();
    };
    properties
        .iter()
        .find_map(|(name, value)| {
            (name == "numberingSystem").then(|| match value {
                Value::String(value) if !value.is_empty() => value.clone(),
                _ => "latn".to_string(),
            })
        })
        .unwrap_or_else(|| "latn".to_string())
}

fn valid_unit_style(unit: &str, style: &str) -> bool {
    if matches!(unit, "years" | "months" | "weeks" | "days") {
        return matches!(style, "long" | "short" | "narrow");
    }
    matches!(style, "long" | "short" | "narrow" | "numeric" | "2-digit")
}

fn method(
    builtin: Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Result<Value, VmError> {
    let slots = super::intl_slots(receiver)?;
    if builtin == Builtin::IntlDurationFormatResolvedOptions {
        return Ok(make_object(slots));
    }
    let text = format_duration(arguments.first(), &slots)?;
    if builtin == Builtin::IntlDurationFormatFormatToParts {
        return Ok(make_array(vec![make_object(vec![
            ("type".to_string(), Value::String("literal".to_string())),
            ("value".to_string(), Value::String(text)),
        ])]));
    }
    Ok(Value::String(text))
}

fn format_duration(value: Option<&Value>, slots: &[(String, Value)]) -> Result<String, VmError> {
    let Value::Object(properties) = value.unwrap_or(&Value::Undefined) else {
        return Err(crate::value::error::throw_type_error(
            "Duration must be an object",
        ));
    };
    let hours = number(properties, "hours");
    let minutes = number(properties, "minutes");
    let seconds = number(properties, "seconds");
    let days = number(properties, "days");
    let negative = [days, hours, minutes, seconds]
        .iter()
        .any(|value| *value < 0);
    let days = days.abs();
    let hours = hours.abs();
    let minutes = minutes.abs();
    let seconds = seconds.abs();
    let style = slots
        .iter()
        .find_map(|(key, value)| (key == "style").then_some(value))
        .and_then(|value| match value {
            Value::String(value) => Some(value.as_str()),
            _ => None,
        })
        .unwrap_or("short");
    if style == "digital" {
        let clock = if hours == 0 {
            format!("{minutes:02}:{seconds:02}")
        } else {
            format!("{hours}:{minutes:02}:{seconds:02}")
        };
        return if days == 0 {
            Ok(if negative { format!("-{clock}") } else { clock })
        } else {
            Ok(if negative {
                format!("-{days} day, {clock}")
            } else {
                format!("{days} day, {clock}")
            })
        };
    }
    Ok(format!("{hours} hr, {minutes} min, {seconds} sec"))
}

fn number(properties: &[(String, Value)], key: &str) -> i64 {
    properties
        .iter()
        .find_map(|(name, value)| (name == key).then_some(value))
        .and_then(|value| match value {
            Value::Number(value) => Some(*value as i64),
            _ => None,
        })
        .unwrap_or(0)
}

fn option(options: Option<&Value>, key: &str) -> Result<Option<String>, VmError> {
    let Some(Value::Object(properties)) = options else {
        return Ok(None);
    };
    let Some((_, value)) = properties.iter().find(|(name, _)| name == key) else {
        return Ok(None);
    };
    if matches!(value, Value::Undefined) {
        return Ok(None);
    }
    Ok(Some(crate::conversion::to_string(value)?))
}
