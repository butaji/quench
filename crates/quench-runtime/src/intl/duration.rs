//! `Intl.DurationFormat` core duration formatting.

use crate::{execute::VmError, ops::Builtin, value::Value};

use super::{
    default_locale, make_array, make_object, resolve_locales, runtime_error,
    supported_numbering_systems, SLOT,
};

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

pub(crate) fn format_temporal_duration(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let duration = receiver.ok_or_else(|| {
        crate::value::error::throw_type_error(
            "Temporal.Duration.prototype.toLocaleString called on incompatible receiver",
        )
    })?;
    crate::temporal::duration::validate_receiver(duration)?;
    let formatter = construct(&[
        arguments.first().cloned().unwrap_or(Value::Undefined),
        arguments.get(1).cloned().unwrap_or(Value::Undefined),
    ])?;
    method(
        Builtin::IntlDurationFormatFormat,
        &[duration.clone()],
        Some(&formatter),
    )
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
    let (style, resolved_numbering, resolved_locale) = resolve_construct_options(options, &locale)?;
    let mut resolved = vec![
        ("locale".to_string(), Value::String(resolved_locale)),
        (
            "numberingSystem".to_string(),
            Value::String(resolved_numbering),
        ),
        ("style".to_string(), Value::String(style)),
    ];
    append_unit_options(&mut resolved, options)?;
    append_fractional_digits(&mut resolved, options)?;
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

fn resolve_construct_options(
    options: Option<&Value>,
    locale: &str,
) -> Result<(String, String, String), VmError> {
    if let Some(value) = options {
        if !matches!(value, Value::Object(_) | Value::Proxy(_) | Value::Undefined) {
            return Err(crate::value::error::throw_type_error(
                "DurationFormat options must be an object",
            ));
        }
    }
    validate_option(options, "localeMatcher", &["lookup", "best fit"])?;
    validate_numbering_option(options)?;
    let style = option(options, "style")?.unwrap_or_else(|| "short".to_string());
    if !matches!(style.as_str(), "long" | "short" | "narrow" | "digital") {
        return Err(runtime_error("RangeError: invalid style"));
    }
    let numbering = resolved_numbering_system(options, locale);
    let resolved_locale = locale_for_numbering(locale, &numbering);
    Ok((style, numbering, resolved_locale))
}

fn validate_numbering_option(options: Option<&Value>) -> Result<(), VmError> {
    let Some(numbering) = option(options, "numberingSystem")? else {
        return Ok(());
    };
    if (3..=8).contains(&numbering.len()) && numbering.chars().all(|ch| ch.is_ascii_alphanumeric())
    {
        Ok(())
    } else {
        Err(runtime_error("RangeError: invalid numberingSystem"))
    }
}

fn append_unit_options(
    resolved: &mut Vec<(String, Value)>,
    options: Option<&Value>,
) -> Result<(), VmError> {
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
        append_unit_display(resolved, options, unit)?;
    }
    Ok(())
}

fn append_unit_display(
    resolved: &mut Vec<(String, Value)>,
    options: Option<&Value>,
    unit: &str,
) -> Result<(), VmError> {
    let display = option(options, &format!("{unit}Display"))?.unwrap_or_else(|| "auto".to_string());
    if !matches!(display.as_str(), "auto" | "always") {
        return Err(runtime_error("RangeError: invalid display"));
    }
    resolved.push((format!("{unit}Display"), Value::String(display)));
    Ok(())
}

fn append_fractional_digits(
    resolved: &mut Vec<(String, Value)>,
    options: Option<&Value>,
) -> Result<(), VmError> {
    let Some(value) = option(options, "fractionalDigits")? else {
        return Ok(());
    };
    let digits = value
        .parse::<i64>()
        .map_err(|_| runtime_error("RangeError: invalid fractionalDigits"))?;
    if !(0..=9).contains(&digits) {
        return Err(runtime_error("RangeError: invalid fractionalDigits"));
    }
    resolved.push(("fractionalDigits".to_string(), Value::Number(digits as f64)));
    Ok(())
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
    supported_numbering_systems()
        .iter()
        .any(|item| matches!(item, Value::String(item) if item == value))
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
    let [_, _, _, days, hours, minutes, seconds, milliseconds, microseconds, nanoseconds] =
        duration_values(properties);
    validate_duration(properties)?;
    format_duration_values(
        slots,
        days,
        hours,
        minutes,
        seconds,
        milliseconds,
        microseconds,
        nanoseconds,
    )
}

fn format_duration_values(
    slots: &[(String, Value)],
    days: i64,
    hours: i64,
    minutes: i64,
    seconds: i64,
    milliseconds: i64,
    microseconds: i64,
    nanoseconds: i64,
) -> Result<String, VmError> {
    let negative = [days, hours, minutes, seconds]
        .iter()
        .any(|value| *value < 0);
    let days = days.abs();
    let hours = hours.abs();
    let minutes = minutes.abs();
    let seconds = seconds.abs();
    let style = duration_style(slots);
    format_duration_shape(
        slots,
        style,
        negative,
        days,
        hours,
        minutes,
        seconds,
        milliseconds,
        microseconds,
        nanoseconds,
    )
}

fn format_duration_shape(
    slots: &[(String, Value)],
    style: &str,
    negative: bool,
    days: i64,
    hours: i64,
    minutes: i64,
    seconds: i64,
    milliseconds: i64,
    microseconds: i64,
    nanoseconds: i64,
) -> Result<String, VmError> {
    if matches!(slot_value(slots, "minutes"), Some("numeric" | "2-digit"))
        && matches!(slot_value(slots, "seconds"), Some("numeric" | "2-digit"))
    {
        return Ok(format_clock_duration(days, hours, minutes, seconds));
    }
    if style == "digital" {
        return Ok(format_digital_duration(
            slots,
            days,
            hours,
            minutes,
            seconds,
            milliseconds,
            microseconds,
            nanoseconds,
            negative,
        ));
    }
    Ok(format_standard_duration(
        slots,
        hours,
        minutes,
        seconds,
        milliseconds,
        microseconds,
        nanoseconds,
    ))
}

fn duration_style(slots: &[(String, Value)]) -> &str {
    slots
        .iter()
        .find_map(|(key, value)| (key == "style").then_some(value))
        .and_then(|value| match value {
            Value::String(value) => Some(value.as_str()),
            _ => None,
        })
        .unwrap_or("short")
}

fn duration_values(properties: &[(String, Value)]) -> [i64; 10] {
    [
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
    ]
}

fn format_clock_duration(days: i64, hours: i64, minutes: i64, seconds: i64) -> String {
    let clock = format!("{minutes}:{seconds:02}");
    let time = if hours == 0 {
        clock
    } else {
        format!("{hours} hr, {clock}")
    };
    if days == 0 {
        time
    } else {
        format!("{days} day, {time}")
    }
}

fn format_digital_duration(
    slots: &[(String, Value)],
    days: i64,
    hours: i64,
    minutes: i64,
    seconds: i64,
    milliseconds: i64,
    microseconds: i64,
    nanoseconds: i64,
    negative: bool,
) -> String {
    let subsecond = milliseconds.abs() * 1_000_000 + microseconds.abs() * 1_000 + nanoseconds.abs();
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
    let sign = if negative { "-" } else { "" };
    if days == 0 {
        format!("{sign}{clock}")
    } else {
        format!("{sign}{}, {clock}", format_days(days))
    }
}

fn format_standard_duration(
    slots: &[(String, Value)],
    hours: i64,
    minutes: i64,
    seconds: i64,
    milliseconds: i64,
    microseconds: i64,
    nanoseconds: i64,
) -> String {
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
    append_subsecond_parts(&mut parts, slots, milliseconds, microseconds, nanoseconds);
    parts.join(", ")
}

fn append_subsecond_parts(
    parts: &mut Vec<String>,
    slots: &[(String, Value)],
    milliseconds: i64,
    microseconds: i64,
    nanoseconds: i64,
) {
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
}

include!("duration_tail.rs");
