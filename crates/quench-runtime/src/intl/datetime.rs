//! `Intl.DateTimeFormat`.

use crate::{conversion, execute::VmError, value::Value};

use super::{
    default_locale, make_array, make_object, resolve_locales, runtime_error, to_string_value, SLOT,
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
            }
            return Ok(());
        }
        match key {
            "hour12" => self.hour12 = Some(text == "true"),
            "timeZone" => self.time_zone = canonicalize_time_zone(&text),
            "calendar" => self.calendar = text.to_ascii_lowercase(),
            "numberingSystem" => self.numbering_system = text.to_ascii_lowercase(),
            "fractionalSecondDigits" => {
                if let Ok(digits) = text.parse::<u32>() {
                    if (1..=9).contains(&digits) {
                        self.fractional_second_digits = Some(digits);
                    }
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
    let (hours, minutes) = match rest.split_once(':') {
        Some((hours, minutes)) => (hours, minutes),
        None => (rest, "00"),
    };
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
            let value = range_value(arguments.first().unwrap_or(&Value::Undefined))?;
            Ok(Value::String(value))
        }
        crate::ops::Builtin::IntlDateTimeFormatFormatToParts => {
            let value = range_value(arguments.first().unwrap_or(&Value::Undefined))?;
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

fn range_values(arguments: &[Value]) -> Result<(String, String), VmError> {
    let start = range_value(arguments.first().unwrap_or(&Value::Undefined))?;
    let end = range_value(arguments.get(1).unwrap_or(&Value::Undefined))?;
    Ok((start, end))
}

fn range_value(value: &Value) -> Result<String, VmError> {
    let number = conversion::to_number(value)?;
    if !number.is_finite() || number.abs() > 8_640_000_000_000_000.0 {
        return Err(runtime_error("RangeError: date value is not finite"));
    }
    Ok(conversion::number_to_string(number.trunc()))
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
