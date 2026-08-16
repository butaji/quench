//! `Intl.DateTimeFormat`.

use chrono::{DateTime, Datelike, Timelike, Utc};

use crate::{conversion, execute::VmError, value::Value};

use super::{
    default_locale, make_array, make_object, resolve_locales, runtime_error, slot_number,
    slot_string, supported_values::NUMBERING_SYSTEMS, SLOT,
};

/// Allowed values for each string-valued date/time component option.
const COMPONENT_VALUES: &[(&str, &[&str])] = &[
    ("dateStyle", &["full", "long", "medium", "short"]),
    ("timeStyle", &["full", "long", "medium", "short"]),
    ("weekday", &["narrow", "short", "long"]),
    ("era", &["narrow", "short", "long"]),
    ("year", &["2-digit", "numeric"]),
    ("month", &["2-digit", "numeric", "narrow", "short", "long"]),
    ("day", &["2-digit", "numeric"]),
    ("dayPeriod", &["narrow", "short", "long"]),
    ("hour", &["2-digit", "numeric"]),
    ("minute", &["2-digit", "numeric"]),
    ("second", &["2-digit", "numeric"]),
    (
        "timeZoneName",
        &[
            "short",
            "long",
            "shortOffset",
            "longOffset",
            "shortGeneric",
            "longGeneric",
        ],
    ),
    ("hourCycle", &["h11", "h12", "h23", "h24"]),
];

/// Explicit date/time components that conflict with `dateStyle`/`timeStyle`.
const EXPLICIT_COMPONENTS: &[&str] = &[
    "weekday",
    "era",
    "year",
    "month",
    "day",
    "dayPeriod",
    "hour",
    "minute",
    "second",
    "timeZoneName",
];

const OPTION_KEYS: &[&str] = &[
    "localeMatcher",
    "calendar",
    "numberingSystem",
    "hour12",
    "hourCycle",
    "timeZone",
    "weekday",
    "era",
    "year",
    "month",
    "day",
    "dayPeriod",
    "hour",
    "minute",
    "second",
    "fractionalSecondDigits",
    "timeZoneName",
    "formatMatcher",
    "dateStyle",
    "timeStyle",
];

pub(crate) struct DateTimeOptions {
    locale: String,
    calendar: String,
    numbering_system: String,
    time_zone: String,
    components: Vec<(String, String)>,
    fractional_second_digits: Option<u32>,
    hour12: Option<bool>,
}

pub(crate) fn construct(arguments: &[Value]) -> Result<Value, VmError> {
    construct_with_defaults(arguments, None)
}

pub(crate) fn construct_with_defaults(
    arguments: &[Value],
    defaults: Option<&[&str]>,
) -> Result<Value, VmError> {
    let locales = resolve_locales(arguments)?;
    let locale = sanitize_locale(&locales.first().cloned().unwrap_or_else(default_locale));
    let options = DateTimeOptions::from_options(locale, arguments.get(1), defaults)?;
    Ok(options.build_object())
}

fn sanitize_locale(locale: &str) -> String {
    let Some((base, extension)) = locale.split_once("-u-") else {
        return locale.to_string();
    };
    let parts: Vec<&str> = extension.split('-').collect();
    let mut kept = Vec::new();
    let mut index = 0;
    while index + 1 < parts.len() {
        let key = parts[index];
        let value = parts[index + 1];
        if (key == "ca" && available_calendar(value))
            || key == "hc"
            || (key == "nu" && NUMBERING_SYSTEMS.contains(&value))
        {
            kept.extend([key, value]);
        }
        index += 2;
    }
    if kept.is_empty() {
        base.to_string()
    } else {
        format!("{base}-u-{}", kept.join("-"))
    }
}

fn available_calendar(calendar: &str) -> bool {
    super::supported_values::supported_calendars()
        .iter()
        .any(|value| matches!(value, Value::String(value) if value == calendar))
}

impl DateTimeOptions {
    fn from_options(
        locale: String,
        options: Option<&Value>,
        defaults: Option<&[&str]>,
    ) -> Result<Self, VmError> {
        if matches!(options, Some(Value::Null)) {
            return Err(runtime_error("TypeError: Cannot convert null to object"));
        }
        let mut formatter = DateTimeOptions {
            locale,
            calendar: "gregory".to_string(),
            numbering_system: "latn".to_string(),
            time_zone: "UTC".to_string(),
            components: Vec::new(),
            fractional_second_digits: None,
            hour12: None,
        };
        if let Some(options) = options.filter(|value| !matches!(value, Value::Undefined)) {
            for key in OPTION_KEYS {
                let value = crate::execute::get_property_result(options, key)?;
                formatter.apply(key, &value)?;
            }
        }
        formatter.apply_defaults(defaults);
        formatter.validate_styles()?;
        formatter.resolve_hour();
        Ok(formatter)
    }

    fn apply(&mut self, key: &str, value: &Value) -> Result<(), VmError> {
        if matches!(value, Value::Undefined) {
            return Ok(());
        }
        let recognized = COMPONENT_VALUES.iter().any(|(name, _)| *name == key)
            || matches!(
                key,
                "localeMatcher"
                    | "formatMatcher"
                    | "hour12"
                    | "timeZone"
                    | "calendar"
                    | "numberingSystem"
                    | "fractionalSecondDigits"
            );
        if !recognized {
            return Ok(());
        }
        if key == "hour12" {
            self.hour12 = Some(crate::execute::is_truthy(value));
            return Ok(());
        }
        if key == "fractionalSecondDigits" {
            let digits = conversion::to_number(value)?;
            if !digits.is_finite() || !(1.0..=3.0).contains(&digits) {
                return Err(runtime_error("RangeError: invalid fractionalSecondDigits"));
            }
            self.fractional_second_digits = Some(digits.floor() as u32);
            return Ok(());
        }
        let text = conversion::to_string(value)?;
        if let Some((name, allowed)) = COMPONENT_VALUES.iter().find(|(name, _)| *name == key) {
            if let Some(valid) = valid_component(&text, allowed) {
                self.set_component(name, valid);
            } else {
                return Err(runtime_error("RangeError: invalid date/time option"));
            }
            return Ok(());
        }
        match key {
            "localeMatcher" if matches!(value, Value::Null) => {
                return Err(runtime_error("TypeError: localeMatcher"));
            }
            "formatMatcher" if !matches!(text.as_str(), "basic" | "best fit") => {
                return Err(runtime_error("RangeError: invalid formatMatcher"));
            }
            "timeZone" => {
                let named_time_zone = canonical_named_time_zone(&text);
                if text.starts_with(['+', '-']) && normalize_offset(&text).is_none() {
                    return Err(runtime_error("RangeError: invalid time zone"));
                }
                if !text.eq_ignore_ascii_case("utc")
                    && normalize_offset(&text).is_none()
                    && named_time_zone.is_none()
                {
                    return Err(runtime_error("RangeError: invalid time zone"));
                }
                self.time_zone = named_time_zone.unwrap_or_else(|| canonicalize_time_zone(&text));
            }
            "calendar" => {
                let value = super::locale::calendar_option(&text)?;
                let value = super::locale::calendar_alias(&value);
                self.calendar = if available_calendar(&value) {
                    value
                } else {
                    "gregory".to_string()
                };
            }
            "numberingSystem" => {
                let value = text.to_ascii_lowercase();
                if !super::supported_values::valid_numbering_system_syntax(&value) {
                    return Err(runtime_error("RangeError: invalid numberingSystem"));
                }
                if super::supported_values::NUMBERING_SYSTEMS.contains(&value.as_str()) {
                    self.numbering_system = value;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn set_component(&mut self, key: &str, value: String) {
        if let Some((_, existing)) = self
            .components
            .iter_mut()
            .find(|(existing, _)| existing == key)
        {
            *existing = value;
        } else {
            self.components.push((key.to_string(), value));
        }
    }

    fn contains(&self, key: &str) -> bool {
        self.components.iter().any(|(name, _)| name == key)
    }

    fn apply_defaults(&mut self, defaults: Option<&[&str]>) {
        if self.contains("dateStyle") || self.contains("timeStyle") {
            return;
        }
        let keys = defaults.unwrap_or(&["year", "month", "day"]);
        if !self.components.is_empty() || keys.iter().any(|key| self.contains(key)) {
            return;
        }
        for key in keys {
            self.set_component(key, "numeric".to_string());
        }
    }

    fn validate_styles(&self) -> Result<(), VmError> {
        if !self.contains("dateStyle") && !self.contains("timeStyle") {
            return Ok(());
        }
        if EXPLICIT_COMPONENTS.iter().any(|name| self.contains(name))
            || self.fractional_second_digits.is_some()
        {
            return Err(runtime_error(
                "TypeError: dateStyle/timeStyle with explicit components",
            ));
        }
        Ok(())
    }

    fn resolve_hour(&mut self) {
        if !self.contains("hour") {
            return;
        }
        if self.hour12.is_some() {
            self.components.retain(|(key, _)| key != "hourCycle");
        } else if !self.contains("hourCycle") {
            self.set_component("hourCycle", "h23".to_string());
        }
    }

    fn build_object(&self) -> Value {
        let properties = vec![
            (
                "format".to_string(),
                Value::Builtin(crate::ops::Builtin::IntlDateTimeFormatFormat),
            ),
            (
                "formatToParts".to_string(),
                Value::Builtin(crate::ops::Builtin::IntlDateTimeFormatFormatToParts),
            ),
            (
                "formatRange".to_string(),
                Value::Builtin(crate::ops::Builtin::IntlDateTimeFormatFormatRange),
            ),
            (
                "formatRangeToParts".to_string(),
                Value::Builtin(crate::ops::Builtin::IntlDateTimeFormatFormatRangeToParts),
            ),
            (
                "resolvedOptions".to_string(),
                Value::Builtin(crate::ops::Builtin::IntlDateTimeFormatResolvedOptions),
            ),
            (
                "\0prototype".to_string(),
                crate::vm::realm_intrinsic(crate::ops::Builtin::IntlDateTimeFormatPrototype),
            ),
            (SLOT.to_string(), self.slot()),
        ];
        make_object(properties)
    }

    fn slot(&self) -> Value {
        make_object(self.resolved_properties())
    }

    fn resolved_properties(&self) -> Vec<(String, Value)> {
        let mut props = vec![
            ("locale".to_string(), Value::String(self.locale.clone())),
            ("calendar".to_string(), Value::String(self.calendar.clone())),
            (
                "numberingSystem".to_string(),
                Value::String(self.numbering_system.clone()),
            ),
            (
                "timeZone".to_string(),
                Value::String(self.time_zone.clone()),
            ),
        ];
        let has_hour = self.contains("hour");
        for (key, value) in &self.components {
            if key == "hourCycle" && !has_hour {
                continue;
            }
            props.push((key.clone(), Value::String(value.clone())));
        }
        if has_hour {
            let hour12 = self.hour12.or_else(|| {
                self.components
                    .iter()
                    .find(|(key, _)| key == "hourCycle")
                    .map(|(_, value)| matches!(value.as_str(), "h11" | "h12"))
            });
            if let Some(hour12) = hour12 {
                props.push(("hour12".to_string(), Value::Boolean(hour12)));
            }
        }
        if let Some(digits) = self.fractional_second_digits {
            props.push((
                "fractionalSecondDigits".to_string(),
                Value::Number(digits as f64),
            ));
        }
        props
    }
}

fn valid_component(text: &str, allowed: &[&str]) -> Option<String> {
    if allowed.contains(&text) {
        Some(text.to_string())
    } else {
        None
    }
}

fn canonicalize_time_zone(time_zone: &str) -> String {
    let time_zone = time_zone.trim();
    if time_zone.eq_ignore_ascii_case("utc") {
        return "UTC".to_string();
    }
    if let Some(offset) = normalize_offset(time_zone) {
        return offset;
    }
    if time_zone.is_empty() {
        "UTC".to_string()
    } else {
        time_zone.to_string()
    }
}

fn canonical_named_time_zone(time_zone: &str) -> Option<String> {
    chrono_tz::TZ_VARIANTS
        .iter()
        .find(|zone| zone.name().eq_ignore_ascii_case(time_zone))
        .map(|zone| zone.name().to_string())
}

fn normalize_offset(time_zone: &str) -> Option<String> {
    let (sign, rest) = match time_zone.chars().next()? {
        '+' => ('+', &time_zone[1..]),
        '-' => ('-', &time_zone[1..]),
        _ => return None,
    };
    let (hours, minutes) = offset_parts(rest)?;
    let hour: u32 = hours.parse().ok()?;
    let minute: u32 = minutes.parse().ok()?;
    if hour > 23 || minute > 59 {
        return None;
    }
    if hour == 0 && minute == 0 {
        return Some("+00:00".to_string());
    }
    Some(format!("{sign}{hour:02}:{minute:02}"))
}

fn offset_parts(rest: &str) -> Option<(&str, &str)> {
    if let Some((hours, minutes)) = rest.split_once(':') {
        return (hours.len() == 2 && minutes.len() == 2).then_some((hours, minutes));
    }
    if !rest.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    match rest.len() {
        2 => Some((rest, "00")),
        4 => Some((&rest[..2], &rest[2..])),
        _ => None,
    }
}

fn literal_part(value: &str) -> Value {
    make_object(vec![
        ("type".to_string(), Value::String("literal".to_string())),
        ("value".to_string(), Value::String(value.to_string())),
    ])
}

pub(crate) fn prototype_method(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Result<Value, VmError> {
    let slots = receiver_slots(receiver)?;
    prototype_result(builtin, arguments, &slots)
}

fn prototype_result(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
    slots: &[(String, Value)],
) -> Result<Value, VmError> {
    match builtin {
        crate::ops::Builtin::IntlDateTimeFormatFormat => format_result(arguments, slots),
        crate::ops::Builtin::IntlDateTimeFormatFormatToParts => parts_result(arguments, slots),
        crate::ops::Builtin::IntlDateTimeFormatFormatRange => {
            let (start, end) = range_values(arguments)?;
            if start == end {
                return Ok(Value::String(start));
            }
            Ok(Value::String(format!("{start} – {end}")))
        }
        crate::ops::Builtin::IntlDateTimeFormatFormatRangeToParts => {
            let (start, end) = range_values(arguments)?;
            Ok(make_array(range_parts(&start, &end)))
        }
        crate::ops::Builtin::IntlDateTimeFormatResolvedOptions => Ok(make_object(slots.to_vec())),
        _ => Err(runtime_error("TypeError: method not found")),
    }
}

fn format_result(arguments: &[Value], slots: &[(String, Value)]) -> Result<Value, VmError> {
    let number = range_number(arguments.first().unwrap_or(&Value::Undefined))?;
    if let Some(value) = day_period_format(slots, number) {
        return Ok(Value::String(value));
    }
    if let Some(value) = proleptic_year_format(slots, number) {
        return Ok(Value::String(value));
    }
    if let Some(value) = fractional_format(slots, number) {
        return Ok(Value::String(value));
    }
    Ok(Value::String(range_text(number)))
}

fn parts_result(arguments: &[Value], slots: &[(String, Value)]) -> Result<Value, VmError> {
    let number = range_number(arguments.first().unwrap_or(&Value::Undefined))?;
    if let Some(value) = day_period_parts(slots, number) {
        return Ok(make_array(value));
    }
    if let Some(value) = fractional_parts(slots, number) {
        return Ok(make_array(value));
    }
    let value = range_text(number);
    Ok(make_array(vec![literal_part(&value)]))
}

fn fractional_parts(slots: &[(String, Value)], number: f64) -> Option<Vec<Value>> {
    let digits = slot_number(slots, "fractionalSecondDigits").unwrap_or(0.0) as u32;
    slot_string(slots, "minute")?;
    slot_string(slots, "second")?;
    let date = DateTime::<Utc>::from_timestamp((number / 1_000.0).trunc() as i64, 0)?;
    let millis = number.rem_euclid(1_000.0) as u32;
    let fraction = millis / 10_u32.pow(3 - digits);
    let mut parts = vec![
        typed_part("minute", format!("{:02}", date.minute())),
        literal_part(":"),
        typed_part("second", format!("{:02}", date.second())),
    ];
    if digits > 0 {
        parts.push(literal_part("."));
        parts.push(typed_part(
            "fractionalSecond",
            format!("{fraction:0width$}", width = digits as usize),
        ));
    }
    Some(parts)
}

fn typed_part(kind: &str, value: String) -> Value {
    make_object(vec![
        ("type".to_string(), Value::String(kind.to_string())),
        ("value".to_string(), Value::String(value)),
    ])
}

fn range_parts(start: &str, end: &str) -> Vec<Value> {
    if start == end {
        return vec![literal_part(start)];
    }
    vec![literal_part(start), literal_part(" – "), literal_part(end)]
}

fn day_period_format(slots: &[(String, Value)], number: f64) -> Option<String> {
    let style = slot_string(slots, "dayPeriod")?;
    let hour = DateTime::<Utc>::from_timestamp((number / 1_000.0).trunc() as i64, 0)?.hour();
    Some(match style.as_str() {
        "narrow" => day_period_name(hour, false),
        "short" => day_period_name(hour, true),
        _ => day_period_name(hour, true),
    })
}

fn day_period_parts(slots: &[(String, Value)], number: f64) -> Option<Vec<Value>> {
    let value = day_period_format(slots, number)?;
    Some(vec![make_object(vec![
        ("type".to_string(), Value::String("dayPeriod".to_string())),
        ("value".to_string(), Value::String(value)),
    ])])
}

fn day_period_name(hour: u32, with_prefix: bool) -> String {
    let name = match hour {
        0..=5 => "night",
        6..=11 => "in the morning",
        12 => "noon",
        13..=17 => "in the afternoon",
        18..=20 => "in the evening",
        _ => "at night",
    };
    if with_prefix {
        name.to_string()
    } else {
        name.replace("in the ", "").replace("at ", "")
    }
}

fn range_values(arguments: &[Value]) -> Result<(String, String), VmError> {
    let Some(start_value) = arguments.first() else {
        return Err(crate::value::error::throw_type_error(
            "date value is undefined",
        ));
    };
    let Some(end_value) = arguments.get(1) else {
        return Err(crate::value::error::throw_type_error(
            "date value is undefined",
        ));
    };
    if matches!(start_value, Value::Undefined) || matches!(end_value, Value::Undefined) {
        return Err(crate::value::error::throw_type_error(
            "date value is undefined",
        ));
    }
    let start = range_number(start_value)?;
    let end = range_number(end_value)?;
    Ok((range_text(start), range_text(end)))
}

fn fractional_format(slots: &[(String, Value)], number: f64) -> Option<String> {
    if slot_string(slots, "minute").is_none() || slot_string(slots, "second").is_none() {
        return None;
    }
    let digits = slot_number(slots, "fractionalSecondDigits").unwrap_or(0.0) as u32;
    let seconds = (number / 1_000.0).trunc() as i64;
    let date = DateTime::<Utc>::from_timestamp(seconds, 0)?;
    if digits == 0 {
        return Some(format!("{:02}:{:02}", date.minute(), date.second()));
    }
    let millis = number.rem_euclid(1_000.0) as u32;
    let fraction = if digits <= 3 {
        millis / 10_u32.pow(3 - digits)
    } else {
        millis * 10_u32.pow(digits - 3)
    };
    Some(format!(
        "{:02}:{:02}.{:0width$}",
        date.minute(),
        date.second(),
        fraction,
        width = digits as usize
    ))
}

fn proleptic_year_format(slots: &[(String, Value)], number: f64) -> Option<String> {
    if slot_string(slots, "year").is_none() || slot_string(slots, "era").is_none() {
        return None;
    }
    let seconds = (number / 1_000.0).trunc() as i64;
    let year = DateTime::<Utc>::from_timestamp(seconds, 0)
        .map_or_else(|| civil_year(number), |date| i64::from(date.year()));
    if year <= 0 {
        Some(format!("{} BC", grouped_year(1 - year)))
    } else {
        Some(format!("{} AD", grouped_year(year)))
    }
}

fn civil_year(number: f64) -> i64 {
    let days = (number / 86_400_000.0).floor() as i64;
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted / 146_097
    } else {
        (shifted - 146_096) / 146_097
    };
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month = (5 * day_of_year + 2) / 153;
    year + if month < 10 { 0 } else { 1 }
}

fn grouped_year(year: i64) -> String {
    year.to_string()
}

fn range_number(value: &Value) -> Result<f64, VmError> {
    let number = conversion::to_number(value)?;
    if !number.is_finite() || number.abs() > 8_640_000_000_000_000.0 {
        return Err(runtime_error("RangeError: date value is not finite"));
    }
    Ok(number.trunc())
}

fn range_text(number: f64) -> String {
    conversion::number_to_string(number)
}

include!("datetime_tail.rs");
