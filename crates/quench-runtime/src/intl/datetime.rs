//! `Intl.DateTimeFormat`.

use chrono::{DateTime, Datelike, Timelike, Utc};

use crate::{conversion, execute::VmError, value::Value};

use super::{
    default_locale, is_supported_calendar, locale::calendar_alias, make_array, make_object,
    resolve_locales, runtime_error, slot_number, slot_string, to_string_value, SLOT,
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

const NON_IANA_TIME_ZONES: &[&str] = &[
    "ACT", "AET", "AGT", "ART", "AST", "BET", "BST", "CAT", "CNT", "CST", "CTT", "EAT", "ECT",
    "IET", "IST", "JST", "MIT", "NET", "NST", "PLT", "PNT", "PRT", "PST", "SST", "VST",
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
    let locales = resolve_locales(arguments)?;
    let locale = locales.first().cloned().unwrap_or_else(default_locale);
    let options = DateTimeOptions::from_options(locale, arguments.get(1))?;
    Ok(options.build_object())
}

impl DateTimeOptions {
    fn from_options(locale: String, options: Option<&Value>) -> Result<Self, VmError> {
        let mut formatter = DateTimeOptions {
            locale,
            calendar: "gregory".to_string(),
            numbering_system: "latn".to_string(),
            time_zone: "UTC".to_string(),
            components: Vec::new(),
            fractional_second_digits: None,
            hour12: None,
        };
        if let Some(Value::Object(properties)) = options {
            for (key, value) in properties.iter() {
                formatter.apply(key, value)?;
            }
        }
        formatter.apply_defaults();
        formatter.validate_styles()?;
        formatter.resolve_hour();
        Ok(formatter)
    }

    fn apply(&mut self, key: &str, value: &Value) -> Result<(), VmError> {
        if matches!(value, Value::Undefined) {
            return Ok(());
        }
        let text = to_string_value(value);
        if let Some((name, allowed)) = COMPONENT_VALUES.iter().find(|(name, _)| *name == key) {
            if let Some(valid) = valid_component(&text, allowed) {
                self.set_component(name, valid);
            } else {
                return Err(runtime_error("RangeError: invalid date/time option"));
            }
            return Ok(());
        }
        match key {
            "hour12" => self.hour12 = Some(text == "true"),
            "timeZone" => {
                if text.starts_with(['+', '-']) && normalize_offset(&text).is_none() {
                    return Err(runtime_error("RangeError: invalid time zone"));
                }
                if NON_IANA_TIME_ZONES.contains(&text.as_str()) {
                    return Err(runtime_error("RangeError: invalid time zone"));
                }
                self.time_zone = canonicalize_time_zone(&text);
            }
            "calendar" => {
                let calendar = calendar_alias(&text.to_ascii_lowercase());
                if !is_supported_calendar(&calendar) {
                    return Err(runtime_error("RangeError: invalid calendar"));
                }
                self.calendar = calendar;
            }
            "numberingSystem" => self.numbering_system = text.to_ascii_lowercase(),
            "fractionalSecondDigits" => {
                let digits = conversion::to_number(value)?;
                if !digits.is_finite() || digits.fract() != 0.0 || !(1.0..=9.0).contains(&digits) {
                    return Err(runtime_error("RangeError: invalid fractionalSecondDigits"));
                }
                self.fractional_second_digits = Some(digits as u32);
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

    fn apply_defaults(&mut self) {
        if self.contains("dateStyle") || self.contains("timeStyle") {
            return;
        }
        if self.contains("year") || self.contains("month") || self.contains("day") {
            return;
        }
        for key in ["year", "month", "day"] {
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
            if let Some(hour12) = self.hour12 {
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

fn normalize_offset(time_zone: &str) -> Option<String> {
    let (sign, rest) = match time_zone.chars().next()? {
        '+' => ('+', &time_zone[1..]),
        '-' => ('-', &time_zone[1..]),
        _ => return None,
    };
    let (hours, minutes) = rest.split_once(':')?;
    if hours.len() != 2 || minutes.len() != 2 {
        return None;
    }
    let hour: u32 = hours.parse().ok()?;
    let minute: u32 = minutes.parse().ok()?;
    if hour > 23 || minute > 59 {
        return None;
    }
    Some(format!("{sign}{hour:02}:{minute:02}"))
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
    match builtin {
        crate::ops::Builtin::IntlDateTimeFormatFormat => {
            let input = arguments.first().unwrap_or(&Value::Undefined);
            let number = range_number(input)?;
            if let Some(value) = day_period_format(&slots, number) {
                return Ok(Value::String(value));
            }
            if let Some(value) = proleptic_year_format(&slots, number) {
                return Ok(Value::String(value));
            }
            if let Some(value) = fractional_format(&slots, number) {
                return Ok(Value::String(value));
            }
            let value = range_text(number);
            Ok(Value::String(value))
        }
        crate::ops::Builtin::IntlDateTimeFormatFormatToParts => {
            let number = range_number(arguments.first().unwrap_or(&Value::Undefined))?;
            if let Some(value) = day_period_parts(&slots, number) {
                return Ok(make_array(value));
            }
            let value = range_text(number);
            Ok(make_array(vec![literal_part(&value)]))
        }
        crate::ops::Builtin::IntlDateTimeFormatFormatRange => {
            let (start, end) = range_values(arguments)?;
            if start == end {
                return Ok(Value::String(start));
            }
            Ok(Value::String(format!("{start} – {end}")))
        }
        crate::ops::Builtin::IntlDateTimeFormatFormatRangeToParts => {
            let (start, end) = range_values(arguments)?;
            if start == end {
                return Ok(make_array(vec![literal_part(&start)]));
            }
            Ok(make_array(vec![
                literal_part(&start),
                literal_part(" – "),
                literal_part(&end),
            ]))
        }
        crate::ops::Builtin::IntlDateTimeFormatResolvedOptions => Ok(make_object(slots)),
        _ => Err(runtime_error("TypeError: method not found")),
    }
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
    let text = year.to_string();
    if text.len() <= 3 {
        return text;
    }
    format!("{},{}", &text[..text.len() - 3], &text[text.len() - 3..])
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

fn receiver_slots(receiver: Option<&Value>) -> Result<Vec<(String, Value)>, VmError> {
    super::intl_slots(receiver)
}

pub(crate) fn dispatch(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Option<Result<Value, VmError>> {
    match builtin {
        crate::ops::Builtin::IntlDateTimeFormat => Some(construct(arguments)),
        crate::ops::Builtin::IntlDateTimeFormatFormat
        | crate::ops::Builtin::IntlDateTimeFormatFormatToParts
        | crate::ops::Builtin::IntlDateTimeFormatFormatRange
        | crate::ops::Builtin::IntlDateTimeFormatFormatRangeToParts
        | crate::ops::Builtin::IntlDateTimeFormatResolvedOptions => {
            Some(prototype_method(builtin, arguments, receiver))
        }
        _ => None,
    }
}
