//! `Intl.DurationFormat` core duration formatting.

use crate::{execute::VmError, ops::Builtin, value::Value};

use super::{
    default_locale, make_array, make_object, resolve_locales, runtime_error, slot_string,
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
    let properties = duration_properties(value)?;
    validate_duration_fields(&properties)?;
    let units = duration_values(&properties);
    validate_duration(&properties)?;
    format_duration_values(slots, &units)
}

fn duration_properties(value: &Value) -> Result<Vec<(String, Value)>, VmError> {
    match value {
        Value::Object(object) => Ok(object.properties.clone()),
        Value::ObjectAlias(alias) => alias
            .0
            .borrow()
            .upgrade()
            .map(|object| object.properties.clone())
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

#[derive(Clone, Copy)]
struct DurationUnits {
    years: i64,
    months: i64,
    weeks: i64,
    days: i64,
    hours: i64,
    minutes: i64,
    seconds: i64,
    milliseconds: i64,
    microseconds: i64,
    nanoseconds: i64,
}

fn format_duration_values(
    slots: &[(String, Value)],
    units: &DurationUnits,
) -> Result<String, VmError> {
    let negative = [units.days, units.hours, units.minutes, units.seconds]
        .iter()
        .any(|value| *value < 0);
    let units = DurationUnits {
        days: units.days.abs(),
        hours: units.hours.abs(),
        minutes: units.minutes.abs(),
        seconds: units.seconds.abs(),
        ..*units
    };
    let style = duration_style(slots);
    format_duration_shape(slots, style, negative, &units)
}

fn format_duration_shape(
    slots: &[(String, Value)],
    style: &str,
    negative: bool,
    units: &DurationUnits,
) -> Result<String, VmError> {
    if matches!(slot_value(slots, "minutes"), Some("numeric" | "2-digit"))
        && matches!(slot_value(slots, "seconds"), Some("numeric" | "2-digit"))
    {
        return Ok(format_mixed_numeric_duration(
            slots,
            style,
            units.days,
            units.hours,
            units.minutes,
            units.seconds,
        ));
    }
    if style == "digital" {
        return Ok(format_digital_duration(slots, units, negative));
    }
    Ok(format_standard_duration(slots, style, units))
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

fn duration_values(properties: &[(String, Value)]) -> DurationUnits {
    DurationUnits {
        years: number(properties, "years"),
        months: number(properties, "months"),
        weeks: number(properties, "weeks"),
        days: number(properties, "days"),
        hours: number(properties, "hours"),
        minutes: number(properties, "minutes"),
        seconds: number(properties, "seconds"),
        milliseconds: number(properties, "milliseconds"),
        microseconds: number(properties, "microseconds"),
        nanoseconds: number(properties, "nanoseconds"),
    }
}

fn format_mixed_numeric_duration(
    slots: &[(String, Value)],
    style: &str,
    days: i64,
    hours: i64,
    minutes: i64,
    seconds: i64,
) -> String {
    let locale = slot_string(slots, "locale").unwrap_or_else(default_locale);
    let mut items = Vec::new();
    if days != 0 {
        items.push(format_duration_unit(
            slots,
            style,
            "days",
            "day",
            &days.to_string(),
        ));
    }
    if hours != 0 {
        items.push(format_duration_unit(
            slots,
            style,
            "hours",
            "hour",
            &hours.to_string(),
        ));
    }
    items.push(format!("{minutes}:{seconds:02}"));
    crate::intl::list::format_list(&items, &locale, style, "unit")
}

fn format_digital_duration(
    slots: &[(String, Value)],
    units: &DurationUnits,
    negative: bool,
) -> String {
    let (days, hours, minutes) = (units.days, units.hours, units.minutes);
    let subsecond = units.milliseconds.abs() * 1_000_000
        + units.microseconds.abs() * 1_000
        + units.nanoseconds.abs();
    let seconds = units.seconds + subsecond / 1_000_000_000;
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
    style: &str,
    units: &DurationUnits,
) -> String {
    let locale = slot_string(slots, "locale").unwrap_or_else(default_locale);
    if slot_value(slots, "microseconds") == Some("numeric")
        || slot_value(slots, "nanoseconds") == Some("numeric")
    {
        return format_fractional_standard(slots, style, units, &locale);
    }
    let units_list = [
        ("years", "year", units.years),
        ("months", "month", units.months),
        ("weeks", "week", units.weeks),
        ("days", "day", units.days),
        ("hours", "hour", units.hours),
        ("minutes", "minute", units.minutes),
        ("seconds", "second", units.seconds),
        ("milliseconds", "millisecond", units.milliseconds),
        ("microseconds", "microsecond", units.microseconds),
        ("nanoseconds", "nanosecond", units.nanoseconds),
    ];
    let items = units_list
        .iter()
        .map(|(slot, unit, value)| {
            let display = slot_value(slots, slot).unwrap_or(style);
            crate::intl::number_format::format_unit(&value.to_string(), Some(unit), display)
        })
        .collect::<Vec<_>>();
    crate::intl::list::format_list(&items, &locale, style, "unit")
}

fn format_fractional_standard(
    slots: &[(String, Value)],
    style: &str,
    units: &DurationUnits,
    locale: &str,
) -> String {
    let mut items = format_fractional_prefix(slots, style, units);
    items.extend(format_fractional_subseconds(
        slots,
        style,
        units.milliseconds,
        units.microseconds,
        units.nanoseconds,
    ));
    crate::intl::list::format_list(items.as_slice(), locale, style, "unit")
}

fn format_fractional_prefix(
    slots: &[(String, Value)],
    style: &str,
    units: &DurationUnits,
) -> Vec<String> {
    let mut items = Vec::new();
    for (slot, unit, value) in [
        ("years", "year", units.years),
        ("months", "month", units.months),
        ("weeks", "week", units.weeks),
        ("days", "day", units.days),
        ("hours", "hour", units.hours),
        ("minutes", "minute", units.minutes),
        ("seconds", "second", units.seconds),
    ] {
        if value != 0 {
            items.push(format_duration_unit(
                slots,
                style,
                slot,
                unit,
                &value.to_string(),
            ));
        }
    }
    items
}

fn format_fractional_subseconds(
    slots: &[(String, Value)],
    style: &str,
    milliseconds: i64,
    microseconds: i64,
    nanoseconds: i64,
) -> Vec<String> {
    let mut items = Vec::new();
    if slot_value(slots, "microseconds") == Some("numeric") {
        let value = fractional_number(format!(
            "{milliseconds}.{:03}{:03}",
            microseconds.abs(),
            nanoseconds.abs()
        ));
        items.push(format_duration_unit(
            slots,
            style,
            "milliseconds",
            "millisecond",
            &value,
        ));
    } else {
        if milliseconds != 0 {
            items.push(format_duration_unit(
                slots,
                style,
                "milliseconds",
                "millisecond",
                &milliseconds.to_string(),
            ));
        }
        if slot_value(slots, "nanoseconds") == Some("numeric") {
            let value = fractional_number(format!("{microseconds}.{:03}", nanoseconds.abs()));
            items.push(format_duration_unit(
                slots,
                style,
                "microseconds",
                "microsecond",
                &value,
            ));
        } else if microseconds != 0 {
            items.push(format_duration_unit(
                slots,
                style,
                "microseconds",
                "microsecond",
                &microseconds.to_string(),
            ));
        }
    }
    if slot_value(slots, "microseconds") != Some("numeric")
        && slot_value(slots, "nanoseconds") != Some("numeric")
        && nanoseconds != 0
    {
        items.push(format_duration_unit(
            slots,
            style,
            "nanoseconds",
            "nanosecond",
            &nanoseconds.to_string(),
        ));
    }
    items
}

fn fractional_number(value: String) -> String {
    value.parse::<f64>().map_or(value, |value| {
        crate::intl::number_format::format_number_rounded(value, 9, 1, "trunc")
    })
}

fn format_duration_unit(
    slots: &[(String, Value)],
    style: &str,
    slot: &str,
    unit: &str,
    value: &str,
) -> String {
    let display = slot_value(slots, slot).unwrap_or(style);
    crate::intl::number_format::format_unit(value, Some(unit), display)
}

include!("duration_tail.rs");
