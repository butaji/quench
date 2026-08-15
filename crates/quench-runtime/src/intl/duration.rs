//! `Intl.DurationFormat` core duration formatting.

use crate::{execute::VmError, ops::Builtin, value::Value};

use super::{default_locale, make_array, make_object, resolve_locales, runtime_error, SLOT};

pub(crate) fn dispatch(
    builtin: Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Option<Result<Value, VmError>> {
    match builtin {
        Builtin::IntlDurationFormat => Some(construct_call(arguments, receiver)),
        Builtin::IntlDurationFormatFormat
        | Builtin::IntlDurationFormatFormatToParts
        | Builtin::IntlDurationFormatResolvedOptions => Some(method(builtin, arguments, receiver)),
        _ => None,
    }
}

fn construct_call(arguments: &[Value], receiver: Option<&Value>) -> Result<Value, VmError> {
    if receiver.is_some() {
        return Err(crate::value::error::throw_type_error(
            "Intl.DurationFormat requires new",
        ));
    }
    construct(arguments)
}

fn construct(arguments: &[Value]) -> Result<Value, VmError> {
    let locale = resolve_locales(arguments)?
        .first()
        .cloned()
        .unwrap_or_else(default_locale);
    let locale = sanitize_locale(&locale);
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
    let style = option(options, "style")?.unwrap_or_else(|| "short".to_string());
    if !matches!(style.as_str(), "long" | "short" | "narrow" | "digital") {
        return Err(runtime_error("RangeError: invalid style"));
    }
    let resolved_numbering = resolved_numbering_system(options, &locale);
    let resolved_locale = locale_for_numbering(&locale, &resolved_numbering);
    let mut resolved = vec![
        ("locale".to_string(), Value::String(resolved_locale)),
        (
            "numberingSystem".to_string(),
            Value::String(resolved_numbering),
        ),
        ("style".to_string(), Value::String(style)),
    ];
    let mut previous_numeric = false;
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
        let explicit = option(options, unit)?;
        let value = explicit
            .clone()
            .unwrap_or_else(|| default_unit_style(unit, previous_numeric));
        if !valid_unit_style(unit, &value)
            || (previous_numeric
                && explicit.is_some()
                && !matches!(value.as_str(), "numeric" | "2-digit"))
        {
            return Err(runtime_error("RangeError: invalid unit style"));
        }
        previous_numeric = matches!(value.as_str(), "numeric" | "2-digit");
        resolved.push((unit.to_string(), Value::String(value)));
        let display =
            option(options, &format!("{unit}Display"))?.unwrap_or_else(|| "auto".to_string());
        if !matches!(display.as_str(), "auto" | "always") {
            return Err(runtime_error("RangeError: invalid display"));
        }
        resolved.push((format!("{unit}Display"), Value::String(display)));
    }
    if let Some(value) = option(options, "fractionalDigits")? {
        let digits = value
            .parse::<i64>()
            .map_err(|_| runtime_error("RangeError: invalid fractionalDigits"))?;
        if !(0..=9).contains(&digits) {
            return Err(runtime_error("RangeError: invalid fractionalDigits"));
        }
        resolved.push(("fractionalDigits".to_string(), Value::Number(digits as f64)));
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

fn resolved_numbering_system(options: Option<&Value>, locale: &str) -> String {
    let requested = numbering_system(options);
    if valid_numbering_system(&requested) {
        return requested;
    }
    locale
        .split_once("-u-nu-")
        .and_then(|(_, value)| value.split('-').next())
        .filter(|value| valid_numbering_system(value))
        .unwrap_or("latn")
        .to_string()
}

fn sanitize_locale(locale: &str) -> String {
    let Some((prefix, extension)) = locale.split_once("-u-nu-") else {
        return locale.to_string();
    };
    let value = extension.split('-').next().unwrap_or_default();
    if valid_numbering_system(value) {
        locale.to_string()
    } else {
        prefix.to_string()
    }
}

fn locale_for_numbering(locale: &str, numbering: &str) -> String {
    let Some((prefix, extension)) = locale.split_once("-u-nu-") else {
        return locale.to_string();
    };
    if extension.split('-').next() == Some(numbering) {
        locale.to_string()
    } else {
        prefix.to_string()
    }
}

fn valid_numbering_system(value: &str) -> bool {
    matches!(
        value,
        "arab" | "arabext" | "deva" | "latn" | "thai" | "jpanfin"
    )
}

fn valid_unit_style(unit: &str, style: &str) -> bool {
    if matches!(unit, "years" | "months" | "weeks" | "days") {
        return matches!(style, "long" | "short" | "narrow");
    }
    matches!(style, "long" | "short" | "narrow" | "numeric" | "2-digit")
}

fn default_unit_style(unit: &str, previous_numeric: bool) -> String {
    if !previous_numeric {
        return "short".to_string();
    }
    match unit {
        "minutes" | "seconds" => "2-digit".to_string(),
        "milliseconds" | "microseconds" | "nanoseconds" => "numeric".to_string(),
        _ => "numeric".to_string(),
    }
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
    let value = value.unwrap_or(&Value::Undefined);
    let Value::Object(properties) = value else {
        return Err(match value {
            value if crate::conversion::is_symbol(value) => {
                crate::value::error::throw_type_error("Duration must be an object")
            }
            Value::String(_) => runtime_error("RangeError: invalid duration string"),
            _ => crate::value::error::throw_type_error("Duration must be an object"),
        });
    };
    validate_duration_fields(properties)?;
    let hours = number(properties, "hours");
    let minutes = number(properties, "minutes");
    let seconds = number(properties, "seconds");
    let milliseconds = number(properties, "milliseconds");
    let microseconds = number(properties, "microseconds");
    let nanoseconds = number(properties, "nanoseconds");
    validate_duration(properties)?;
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
    if slot_value(slots, "minutes") == Some("numeric")
        && slot_value(slots, "seconds") == Some("numeric")
    {
        let clock = format!("{minutes}:{seconds:02}");
        let time = if hours == 0 {
            clock
        } else {
            format!("{hours} hr, {clock}")
        };
        return if days == 0 {
            Ok(time)
        } else {
            Ok(format!("{days} day, {time}"))
        };
    }
    if style == "digital" {
        let subsecond =
            milliseconds.abs() * 1_000_000 + microseconds.abs() * 1_000 + nanoseconds.abs();
        let seconds = seconds + subsecond / 1_000_000_000;
        let remainder = subsecond % 1_000_000_000;
        let mut clock = if hours == 0 {
            format!("{minutes:02}:{seconds:02}")
        } else {
            format!("{hours}:{minutes:02}:{seconds:02}")
        };
        if let Some(fraction) = digital_fraction(slots, remainder) {
            clock.push_str(&fraction);
        }
        return if days == 0 {
            Ok(if negative { format!("-{clock}") } else { clock })
        } else {
            let day_text = format_days(days);
            Ok(if negative {
                format!("-{day_text}, {clock}")
            } else {
                format!("{day_text}, {clock}")
            })
        };
    }
    let mut parts = Vec::new();
    if hours != 0 {
        parts.push(format!("{hours} hr"));
    }
    if minutes != 0 {
        parts.push(format!("{minutes} min"));
    }
    if seconds != 0 {
        parts.push(format!("{seconds} sec"));
    }
    if slot_value(slots, "microseconds") == Some("numeric")
        && (milliseconds != 0 || microseconds != 0 || nanoseconds != 0)
    {
        parts.push(format!(
            "{:03}.{:03}{:03} ms",
            milliseconds.abs(),
            microseconds.abs(),
            nanoseconds.abs()
        ));
    } else if slot_value(slots, "nanoseconds") == Some("numeric") {
        if milliseconds != 0 {
            parts.push(format!("{milliseconds} ms"));
        }
        if microseconds != 0 || nanoseconds != 0 {
            parts.push(format!(
                "{:02}.{:03} μs",
                microseconds.abs(),
                nanoseconds.abs()
            ));
        }
    }
    Ok(parts.join(", "))
}

fn validate_duration_fields(properties: &[(String, Value)]) -> Result<(), VmError> {
    const UNITS: [&str; 10] = [
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
    ];
    let fields = properties
        .iter()
        .filter(|(name, _)| UNITS.contains(&name.as_str()))
        .collect::<Vec<_>>();
    if fields.is_empty()
        || fields
            .iter()
            .any(|(_, value)| matches!(value, Value::Undefined))
    {
        return Err(crate::value::error::throw_type_error(
            "invalid duration record",
        ));
    }
    Ok(())
}

fn format_days(days: i64) -> String {
    let text = days.to_string();
    let grouped = text
        .as_bytes()
        .rchunks(3)
        .rev()
        .map(|chunk| String::from_utf8_lossy(chunk).into_owned())
        .collect::<Vec<_>>()
        .join(",");
    format!("{grouped} {}", if days == 1 { "day" } else { "days" })
}

fn digital_fraction(slots: &[(String, Value)], nanoseconds: i64) -> Option<String> {
    let digits = slot_number(slots, "fractionalDigits");
    let raw = format!("{nanoseconds:09}");
    let count = digits.map_or_else(|| raw.trim_end_matches('0').len(), |value| value as usize);
    (count > 0).then(|| format!(".{}", &raw[..count.min(9)]))
}

fn slot_number(slots: &[(String, Value)], key: &str) -> Option<f64> {
    slots.iter().find_map(|(name, value)| {
        (name == key)
            .then_some(value)
            .and_then(|value| match value {
                Value::Number(value) => Some(*value),
                _ => None,
            })
    })
}

fn validate_duration(properties: &[(String, Value)]) -> Result<(), VmError> {
    if raw_duration_out_of_range(properties) {
        return Err(runtime_error("RangeError: invalid duration"));
    }
    let values = [
        number(properties, "years"),
        number(properties, "months"),
        number(properties, "weeks"),
        number(properties, "days"),
        number(properties, "hours"),
        number(properties, "minutes"),
        number(properties, "seconds"),
        number(properties, "milliseconds"),
        number(properties, "microseconds"),
        number(properties, "nanoseconds"),
    ];
    if values[..3]
        .iter()
        .any(|value| value.unsigned_abs() >= 1_u64 << 32)
        || values[3].unsigned_abs() >= 104_249_991_375
        || normalized_nanoseconds(&values).abs() >= 9_007_199_254_740_992_i128 * 1_000_000_000
    {
        return Err(runtime_error("RangeError: invalid duration"));
    }
    let positive = values.iter().any(|value| *value > 0);
    let negative = values.iter().any(|value| *value < 0);
    if positive && negative {
        return Err(runtime_error("RangeError: mixed-sign duration"));
    }
    Ok(())
}

fn raw_duration_out_of_range(properties: &[(String, Value)]) -> bool {
    let factors = [
        ("days", 86_400_000_000_000_i128),
        ("hours", 3_600_000_000_000),
        ("minutes", 60_000_000_000),
        ("seconds", 1_000_000_000),
        ("milliseconds", 1_000_000),
        ("microseconds", 1_000),
        ("nanoseconds", 1),
    ];
    let total = factors.iter().fold(0_i128, |total, (unit, factor)| {
        total
            + properties.iter().fold(0_i128, |subtotal, (name, value)| {
                subtotal
                    + if name == unit {
                        match value {
                            Value::Number(value) => *value as i128 * factor,
                            _ => 0,
                        }
                    } else {
                        0
                    }
            })
    });
    total.abs() >= 9_007_199_254_740_992_i128 * 1_000_000_000
}

fn normalized_nanoseconds(values: &[i64; 10]) -> i128 {
    i128::from(values[3]) * 86_400_000_000_000
        + i128::from(values[4]) * 3_600_000_000_000
        + i128::from(values[5]) * 60_000_000_000
        + i128::from(values[6]) * 1_000_000_000
        + i128::from(values[7]) * 1_000_000
        + i128::from(values[8]) * 1_000
        + i128::from(values[9])
}

fn slot_value<'a>(slots: &'a [(String, Value)], key: &str) -> Option<&'a str> {
    slots.iter().find_map(|(name, value)| {
        (name == key)
            .then_some(value)
            .and_then(|value| match value {
                Value::String(value) => Some(value.as_str()),
                _ => None,
            })
    })
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
    let Some(options) = options else {
        return Ok(None);
    };
    let value = crate::execute::get_property_result(options, key)?;
    if matches!(value, Value::Undefined) {
        return Ok(None);
    }
    Ok(Some(crate::conversion::to_string(&value)?))
}
