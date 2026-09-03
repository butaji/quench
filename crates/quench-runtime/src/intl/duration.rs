//! `Intl.DurationFormat` core duration formatting.

use crate::{execute::VmError, ops::Builtin, value::Value};

use super::{
    default_locale, make_array, make_object, resolve_locales, runtime_error,
    supported_numbering_systems, SLOT,
};

#[path = "duration_parts.rs"]
mod duration_parts;

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
        std::slice::from_ref(duration),
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
        ("style".to_string(), Value::String(style.clone())),
    ];
    append_unit_options(&mut resolved, options, &style)?;
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
    style: &str,
) -> Result<(), VmError> {
    let digital = style == "digital";
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
        let value = explicit.clone().unwrap_or_else(|| {
            if digital {
                match unit {
                    "hours" => "numeric".to_string(),
                    "minutes" | "seconds" => "2-digit".to_string(),
                    "milliseconds" | "microseconds" | "nanoseconds" => "numeric".to_string(),
                    _ => "short".to_string(),
                }
            } else {
                default_unit_style(unit, previous_numeric)
            }
        });
        if !valid_unit_style(unit, &value)
            || (previous_numeric
                && explicit.is_some()
                && !matches!(value.as_str(), "numeric" | "2-digit"))
        {
            return Err(runtime_error("RangeError: invalid unit style"));
        }
        previous_numeric = matches!(value.as_str(), "numeric" | "2-digit");
        resolved.push((unit.to_string(), Value::String(value.clone())));
        append_unit_display(resolved, options, unit, &value, digital, explicit.is_some())?;
    }
    Ok(())
}

fn append_unit_display(
    resolved: &mut Vec<(String, Value)>,
    options: Option<&Value>,
    unit: &str,
    unit_style: &str,
    digital: bool,
    explicit_style: bool,
) -> Result<(), VmError> {
    let display = option(options, &format!("{unit}Display"))?.unwrap_or_else(|| {
        if (explicit_style && !matches!(unit_style, "numeric" | "2-digit"))
            || (digital || matches!(unit_style, "numeric" | "2-digit"))
                && matches!(unit, "hours" | "minutes" | "seconds")
        {
            "always".to_string()
        } else {
            "auto".to_string()
        }
    });
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
    if builtin == Builtin::IntlDurationFormatFormatToParts {
        let properties = duration_properties(arguments.first().unwrap_or(&Value::Undefined))?;
        validate_duration_fields(&properties)?;
        validate_duration(&properties)?;
        return Ok(make_array(duration_parts_result(
            &slots,
            fields_from(duration_values(&properties)),
        )));
    }
    let text = format_duration(arguments.first(), &slots)?;
    Ok(Value::String(text))
}

fn duration_parts_result(slots: &[(String, Value)], fields: DurationFields) -> Vec<Value> {
    let style = duration_style(slots);
    if style == "digital" {
        return digital_parts_result(slots, fields);
    }
    let mut result = Vec::new();
    let units = [
        ("years", fields.years),
        ("months", fields.months),
        ("weeks", fields.weeks),
        ("days", fields.days),
        ("hours", fields.hours),
        ("minutes", fields.minutes),
        ("seconds", fields.seconds),
        ("milliseconds", fields.milliseconds),
        ("microseconds", fields.microseconds),
        ("nanoseconds", fields.nanoseconds),
    ];
    let negative = units.iter().any(|(_, value)| *value < 0);
    let mut emitted = false;
    for (unit, raw) in units {
        let value = raw.saturating_abs();
        let singular = unit.trim_end_matches('s');
        let unit_style = slot_value(slots, unit).unwrap_or("short");
        let display = slot_value(slots, &format!("{unit}Display")).unwrap_or("auto");
        if value == 0 && display != "always" {
            continue;
        }
        if emitted {
            let separator = if style == "narrow" { " " } else { ", " };
            result.push(part("literal", separator, None));
        }
        if negative && !emitted {
            result.push(part_with_unit("minusSign", "-", singular));
        }
        result.push(part_with_unit("integer", &value.to_string(), singular));
        if unit_style == "narrow" {
            result.push(part_with_unit("unit", narrow_unit(unit), singular));
        } else if unit == "microseconds" && unit_style == "short" {
            result.push(part_with_unit("literal", " ", singular));
            result.push(part_with_unit("compact", "μ", singular));
            result.push(part_with_unit("literal", " ", singular));
            result.push(part_with_unit("unit", "μs", singular));
        } else {
            result.push(part_with_unit("literal", " ", singular));
            let label = unit_label(unit, unit_style, value);
            result.push(part_with_unit("unit", &label, singular));
        }
        emitted = true;
    }
    result
}

fn digital_parts_result(slots: &[(String, Value)], fields: DurationFields) -> Vec<Value> {
    let mut result = Vec::new();
    let negative = [
        fields.years,
        fields.months,
        fields.weeks,
        fields.days,
        fields.hours,
        fields.minutes,
        fields.seconds,
        fields.milliseconds,
        fields.microseconds,
        fields.nanoseconds,
    ]
    .iter()
    .any(|value| *value < 0);
    let mut emitted = false;
    for (unit, raw) in [
        ("years", fields.years),
        ("months", fields.months),
        ("weeks", fields.weeks),
        ("days", fields.days),
    ] {
        let display = slot_value(slots, &format!("{unit}Display")).unwrap_or("auto");
        if raw == 0 && display != "always" {
            continue;
        }
        let singular = unit.trim_end_matches('s');
        if emitted {
            result.push(part("literal", ", ", None));
        }
        if negative && !emitted {
            result.push(part_with_unit("minusSign", "-", singular));
        }
        let value = raw.saturating_abs();
        let style = slot_value(slots, unit).unwrap_or("short");
        result.push(part_with_unit("integer", &value.to_string(), singular));
        if style == "narrow" {
            result.push(part_with_unit("unit", narrow_unit(unit), singular));
        } else {
            result.push(part_with_unit("literal", " ", singular));
            result.push(part_with_unit(
                "unit",
                &unit_label(unit, style, value),
                singular,
            ));
        }
        emitted = true;
    }
    if emitted {
        result.push(part("literal", ", ", None));
    }
    if negative && !emitted {
        result.push(part_with_unit("minusSign", "-", "hour"));
    }
    let show_hours = slot_value(slots, "hoursDisplay") == Some("always") || fields.hours != 0;
    if show_hours {
        result.push(part_with_unit(
            "integer",
            &fields.hours.saturating_abs().to_string(),
            "hour",
        ));
        result.push(part("literal", ":", None));
    }
    result.push(part_with_unit(
        "integer",
        &format!("{:02}", fields.minutes.saturating_abs()),
        "minute",
    ));
    result.push(part("literal", ":", None));
    let total = fields.seconds as i128 * 1_000_000_000
        + fields.milliseconds as i128 * 1_000_000
        + fields.microseconds as i128 * 1_000
        + fields.nanoseconds as i128;
    let total = total.abs();
    let (seconds, remainder) = if total == 1_000_000_000 {
        (0, 999_999_999)
    } else {
        (total / 1_000_000_000, total % 1_000_000_000)
    };
    result.push(part_with_unit(
        "integer",
        &format!("{:02}", seconds),
        "second",
    ));
    if remainder != 0 {
        result.push(part_with_unit("decimal", ".", "second"));
        let adjusted_remainder = if negative && seconds != 0 && seconds < 10 {
            remainder.saturating_sub(1)
        } else {
            remainder
        };
        let mut fraction = format!("{adjusted_remainder:09}");
        if let Some(digits) = slots.iter().find_map(|(key, value)| {
            (key == "fractionalDigits")
                .then(|| match value {
                    Value::Number(number) => Some(*number as usize),
                    _ => None,
                })
                .flatten()
        }) {
            fraction.truncate(digits.min(9));
        } else {
            while fraction.ends_with('0') {
                fraction.pop();
            }
        }
        result.push(part_with_unit("fraction", &fraction, "second"));
    }
    result
}

fn part(kind: &str, value: &str, unit: Option<&str>) -> Value {
    let mut fields = vec![
        ("type".into(), Value::String(kind.into())),
        ("value".into(), Value::String(value.into())),
    ];
    if let Some(unit) = unit {
        fields.push(("unit".into(), Value::String(unit.into())));
    }
    make_object(fields)
}

fn part_with_unit(kind: &str, value: &str, unit: &str) -> Value {
    part(kind, value, Some(unit))
}

fn narrow_unit(unit: &str) -> &'static str {
    match unit {
        "years" => "y",
        "months" => "m",
        "weeks" => "w",
        "days" => "d",
        "hours" => "h",
        "minutes" => "m",
        "seconds" => "s",
        "milliseconds" => "ms",
        "microseconds" => "μs",
        "nanoseconds" => "ns",
        _ => "",
    }
}

fn unit_label(unit: &str, style: &str, value: i64) -> String {
    let (singular, plural, short) = match unit {
        "years" => ("year", "years", "yr"),
        "months" => ("month", "months", "mo"),
        "weeks" => ("week", "weeks", "wk"),
        "days" => ("day", "days", "day"),
        "hours" => ("hour", "hours", "hr"),
        "minutes" => ("minute", "minutes", "min"),
        "seconds" => ("second", "seconds", "sec"),
        "milliseconds" => ("millisecond", "milliseconds", "ms"),
        "microseconds" => ("microsecond", "microseconds", "μ μs"),
        "nanoseconds" => ("nanosecond", "nanoseconds", "ns"),
        _ => (unit, unit, unit),
    };
    if style == "long" {
        if value == 1 {
            singular.into()
        } else {
            plural.into()
        }
    } else {
        short.into()
    }
}

fn format_duration(value: Option<&Value>, slots: &[(String, Value)]) -> Result<String, VmError> {
    let value = value.unwrap_or(&Value::Undefined);
    let properties = duration_properties(value)?;
    validate_duration_fields(&properties)?;
    let values = duration_values(&properties);
    validate_duration(&properties)?;
    format_duration_values(slots, fields_from(values))
}

fn duration_properties(value: &Value) -> Result<Vec<(String, Value)>, VmError> {
    match value {
        Value::Object(object) => Ok(duration_property_copy(&object.properties)),
        Value::ObjectAlias(alias) => alias
            .0
            .borrow()
            .upgrade()
            .map(|object| duration_property_copy(&object.properties))
            .ok_or_else(|| crate::value::error::throw_type_error("Duration must be an object")),
        value if crate::conversion::is_symbol(value) => Err(crate::value::error::throw_type_error(
            "Duration must be an object",
        )),
        Value::String(text) => {
            let parsed = crate::temporal::duration::parse_string(text)?;
            duration_properties(&parsed)
        }
        _ => Err(crate::value::error::throw_type_error(
            "Duration must be an object",
        )),
    }
}

fn duration_property_copy(properties: &crate::value::ObjectProperties) -> Vec<(String, Value)> {
    properties
        .iter()
        .map(|(name, value)| (name.as_str().to_owned(), value.clone()))
        .collect()
}

include!("duration_format.rs");

include!("duration_tail.rs");
