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

/// Format a Temporal value through the same DateTimeFormat path as user code.
/// The defaults are supplied by the Temporal type, while locale and options
/// remain the caller's data.
pub(crate) fn format_temporal_value(
    value: &Value,
    arguments: &[Value],
    defaults: &[&str],
) -> Result<Value, VmError> {
    let formatter = construct_with_defaults(arguments, Some(defaults))?;
    prototype_method(
        crate::ops::Builtin::IntlDateTimeFormatFormat,
        std::slice::from_ref(value),
        Some(&formatter),
    )
}

fn sanitize_locale(locale: &str) -> String {
    let Some((base, extension)) = locale.split_once("-u-") else {
        return locale.to_string();
    };
    let parts: Vec<&str> = extension.split('-').collect();
    let mut kept: Vec<String> = Vec::new();
    let mut index = 0;
    while index < parts.len() {
        let key = parts[index];
        if key == "ca" {
            let end = (index + 1..parts.len())
                .find(|candidate| parts[*candidate].len() == 2)
                .unwrap_or(parts.len());
            let value = parts[index + 1..end].join("-");
            let value = super::locale::calendar_alias(&value);
            if available_calendar(&value) {
                kept.extend([key.to_string(), value]);
            }
            index = end;
        } else if let Some(value) = parts.get(index + 1).copied() {
            if key == "hc" || (key == "nu" && NUMBERING_SYSTEMS.contains(&value)) {
                kept.extend([key.to_string(), value.to_string()]);
            }
            index += 2;
        } else {
            index += 1;
        }
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
        let calendar =
            super::locale::calendar_from_tag(&locale).unwrap_or_else(|| "gregory".to_string());
        let numbering_system = super::numbering_system(&locale)
            .unwrap_or_else(|| super::default_numbering_system(&locale))
            .to_string();
        let mut formatter = DateTimeOptions {
            locale,
            calendar,
            numbering_system,
            time_zone: "America/Lima".to_string(),
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
            self.locale = super::locale::remove_unicode_extension(&self.locale, "hc");
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
                let is_hour_cycle = key == "hourCycle";
                self.set_component(name, valid.clone());
                if is_hour_cycle {
                    if locale_hour_cycle(&self.locale) != Some(valid.as_str()) {
                        self.locale = super::locale::remove_unicode_extension(&self.locale, "hc");
                    }
                }
            } else {
                return Err(runtime_error("RangeError: invalid date/time option"));
            }
            return Ok(());
        }
        match key {
            "localeMatcher" if !matches!(text.as_str(), "lookup" | "best fit") => {
                return Err(runtime_error("RangeError: invalid localeMatcher"));
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
                if available_calendar(&value) {
                    if super::locale::calendar_from_tag(&self.locale).as_deref() != Some(&value) {
                        self.locale = super::locale::remove_unicode_extension(&self.locale, "ca");
                    }
                    self.calendar = value;
                }
            }
            "numberingSystem" => {
                let value = text.to_ascii_lowercase();
                if !super::supported_values::valid_numbering_system_syntax(&value) {
                    return Err(runtime_error("RangeError: invalid numberingSystem"));
                }
                if super::supported_values::NUMBERING_SYSTEMS.contains(&value.as_str()) {
                    let locale_numbering = super::numbering_system(&self.locale);
                    self.numbering_system = value;
                    if locale_numbering != Some(self.numbering_system.as_str()) {
                        self.locale = super::locale::remove_unicode_extension(&self.locale, "nu");
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

    fn apply_defaults(&mut self, defaults: Option<&[&str]>) {
        if self.contains("dateStyle") || self.contains("timeStyle") {
            return;
        }
        let keys = defaults.unwrap_or(&["year", "month", "day"]);
        let has_core_component = self.components.iter().any(|(key, _)| {
            matches!(
                key.as_str(),
                "weekday"
                    | "era"
                    | "year"
                    | "month"
                    | "day"
                    | "dayPeriod"
                    | "hour"
                    | "minute"
                    | "second"
            )
        });
        if (self.fractional_second_digits.is_some() || self.contains("dayPeriod"))
            && !has_core_component
        {
            for key in keys
                .iter()
                .filter(|key| matches!(**key, "hour" | "minute" | "second"))
            {
                self.set_component(key, "numeric".to_string());
            }
            return;
        }
        if self.contains("dayPeriod")
            && !self.contains("hour")
            && !self.contains("minute")
            && !self.contains("second")
        {
            for key in keys
                .iter()
                .filter(|key| matches!(**key, "hour" | "minute" | "second"))
            {
                self.set_component(key, "numeric".to_string());
            }
        }
        if has_core_component {
            if defaults.is_some_and(|keys| {
                keys.len() == 3 && keys.contains(&"year") && !keys.contains(&"hour")
            }) && !self
                .components
                .iter()
                .any(|(key, _)| matches!(key.as_str(), "year" | "month" | "day" | "weekday"))
            {
                for key in ["year", "month", "day"] {
                    self.set_component(key, "numeric".to_string());
                }
            }
            if defaults.is_some_and(|keys| keys.len() == 3 && keys.contains(&"hour"))
                && !self.components.iter().any(|(key, _)| {
                    matches!(key.as_str(), "hour" | "minute" | "second" | "dayPeriod")
                })
            {
                for key in ["hour", "minute", "second"] {
                    self.set_component(key, "numeric".to_string());
                }
            }
            return;
        }
        for key in keys {
            if !self.contains(key) {
                self.set_component(key, "numeric".to_string());
            }
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
        if !self.contains("hour") && !self.contains("timeStyle") {
            return;
        }
        if self.hour12.is_some() {
            self.components.retain(|(key, _)| key != "hourCycle");
            self.set_component(
                "hourCycle",
                if self.hour12 == Some(true) {
                    if self.locale.starts_with("ja") {
                        "h11"
                    } else {
                        "h12"
                    }
                } else {
                    "h23"
                }
                .to_string(),
            );
        } else if !self.contains("hourCycle") {
            let cycle = locale_hour_cycle(&self.locale).unwrap_or_else(|| {
                if self.locale.starts_with("ja") {
                    "h11"
                } else if self.locale.starts_with("en") {
                    "h12"
                } else {
                    "h23"
                }
            });
            self.set_component("hourCycle", cycle.to_string());
        }
    }

    fn build_object(&self) -> Value {
        let properties = vec![
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
        self.push_hour_properties(&mut props);
        for (key, value) in &self.components {
            if key == "hourCycle" {
                continue;
            }
            props.push((key.clone(), Value::String(value.clone())));
        }
        if let Some(digits) = self.fractional_second_digits {
            props.push((
                "fractionalSecondDigits".to_string(),
                Value::Number(digits as f64),
            ));
        }
        props
    }

    fn push_hour_properties(&self, props: &mut Vec<(String, Value)>) {
        if !self.contains("hour") && !self.contains("timeStyle") {
            return;
        }
        if let Some((_, value)) = self.components.iter().find(|(key, _)| key == "hourCycle") {
            props.push(("hourCycle".to_string(), Value::String(value.clone())));
        }
        if let Some(hour12) = self.hour12.or_else(|| {
            self.components
                .iter()
                .find(|(key, _)| key == "hourCycle")
                .map(|(_, value)| matches!(value.as_str(), "h11" | "h12"))
        }) {
            props.push(("hour12".to_string(), Value::Boolean(hour12)));
        }
    }
}

fn valid_component(text: &str, allowed: &[&str]) -> Option<String> {
    if allowed.contains(&text) {
        Some(text.to_string())
    } else {
        None
    }
}

fn locale_hour_cycle(locale: &str) -> Option<&str> {
    let (_, extension) = locale.split_once("-u-")?;
    let parts: Vec<_> = extension.split('-').collect();
    let index = parts.iter().position(|part| *part == "hc")? + 1;
    let value = parts.get(index).copied()?;
    COMPONENT_VALUES
        .iter()
        .find(|(name, values)| *name == "hourCycle" && values.contains(&value))
        .map(|_| value)
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
            let (start, end) = range_values(arguments, slots)?;
            if start == end {
                return Ok(Value::String(start));
            }
            if let Some(collapsed) = collapse_range(&start, &end) {
                return Ok(Value::String(collapsed));
            }
            Ok(Value::String(format!("{start} – {end}")))
        }
        crate::ops::Builtin::IntlDateTimeFormatFormatRangeToParts => {
            let parts = range_parts_result(arguments, slots)?;
            Ok(make_array(parts))
        }
        crate::ops::Builtin::IntlDateTimeFormatResolvedOptions => Ok(make_object(
            slots
                .iter()
                .filter(|(name, _)| !name.starts_with('\0'))
                .cloned()
                .collect(),
        )),
        _ => Err(runtime_error("TypeError: method not found")),
    }
}

fn collapse_range(start: &str, end: &str) -> Option<String> {
    if let (Some((start_date, start_time)), Some((end_date, end_time))) =
        (start.split_once(", "), end.split_once(", "))
    {
        if start_date == end_date && start_time.contains(':') && end_time.contains(':') {
            return Some(format!("{start_date}, {start_time} – {end_time}"));
        }
    }
    let (start_prefix, start_suffix) = start.rsplit_once(", ")?;
    let (end_prefix, end_suffix) = end.rsplit_once(", ")?;
    if start_suffix != end_suffix {
        return None;
    }
    let (start_month, start_day) = start_prefix.rsplit_once(' ')?;
    let (end_month, end_day) = end_prefix.rsplit_once(' ')?;
    if start_month == end_month {
        Some(format!(
            "{start_month} {start_day} – {end_day}, {start_suffix}"
        ))
    } else {
        Some(format!(
            "{start_month} {start_day} – {end_month} {end_day}, {start_suffix}"
        ))
    }
}

include!("datetime_format_helpers.rs");

include!("datetime_tail.rs");
