//! `Intl.DateTimeFormat`.

use chrono::{DateTime, Datelike, Local, NaiveDateTime, TimeZone, Timelike, Utc};
use std::sync::atomic::{AtomicU64, Ordering};

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
    local_time_zone: bool,
}

static LEGACY_SYMBOL_ID: AtomicU64 = AtomicU64::new(1);

pub(crate) fn construct(arguments: &[Value]) -> Result<Value, VmError> {
    let locales = resolve_locales(arguments)?;
    let locale = locales
        .first()
        .map(|value| sanitize_locale_extensions(value))
        .unwrap_or_else(default_locale);
    let options = DateTimeOptions::from_options(locale, arguments.get(1))?;
    Ok(options.build_object())
}

fn sanitize_locale_extensions(locale: &str) -> String {
    let Some(index) = locale.split('-').position(|part| part == "u") else {
        return locale.to_string();
    };
    let parts: Vec<&str> = locale.split('-').collect();
    let mut result = parts[..index]
        .iter()
        .map(|part| (*part).to_string())
        .collect::<Vec<_>>();
    let mut cursor = index + 1;
    while cursor < parts.len() {
        let key = parts[cursor];
        if key.len() != 2 {
            cursor += 1;
            continue;
        }
        let value = parts.get(cursor + 1).copied().unwrap_or_default();
        if (key == "nu" && valid_numbering_system(value))
            || (key == "ca" && valid_calendar_extension(value))
            || (key == "hc" && valid_hour_cycle(value))
        {
            result.push(key.to_string());
            result.push(value.to_string());
        }
        cursor += 1;
        while cursor < parts.len() && parts[cursor].len() > 2 {
            cursor += 1;
        }
    }
    if result.len() == index {
        return result.join("-");
    }
    result.insert(index, "u".to_string());
    result.join("-")
}

fn valid_calendar_extension(value: &str) -> bool {
    matches!(
        value,
        "buddhist"
            | "chinese"
            | "coptic"
            | "dangi"
            | "ethioaa"
            | "ethiopic"
            | "gregory"
            | "hebrew"
            | "indian"
            | "islamic-civil"
            | "islamic-tbla"
            | "islamic-umalqura"
            | "iso8601"
            | "japanese"
            | "persian"
            | "roc"
    )
}

fn locale_calendar(locale: &str) -> Option<&str> {
    locale
        .split('-')
        .collect::<Vec<_>>()
        .windows(2)
        .find(|pair| pair[0] == "ca" && valid_calendar_extension(pair[1]))
        .map(|pair| pair[1])
}

fn locale_numbering_system(locale: &str) -> Option<&str> {
    locale
        .split('-')
        .collect::<Vec<_>>()
        .windows(2)
        .find(|pair| pair[0] == "nu" && valid_numbering_system(pair[1]))
        .map(|pair| pair[1])
}

fn locale_hour_cycle(locale: &str) -> Option<&str> {
    locale
        .split('-')
        .collect::<Vec<_>>()
        .windows(2)
        .find(|pair| pair[0] == "hc" && valid_hour_cycle(pair[1]))
        .map(|pair| pair[1])
}

fn valid_hour_cycle(value: &str) -> bool {
    matches!(value, "h11" | "h12" | "h23" | "h24")
}

fn remove_locale_extension(locale: &str, unwanted: &str) -> String {
    let parts = locale.split('-').collect::<Vec<_>>();
    let mut result = Vec::new();
    let mut index = 0;
    while index < parts.len() {
        if parts[index] == unwanted {
            index += 2;
            continue;
        }
        result.push(parts[index]);
        index += 1;
    }
    if result.last() == Some(&"u") {
        result.pop();
    }
    result.join("-")
}

fn valid_numbering_system(value: &str) -> bool {
    matches!(
        value,
        "adlm"
            | "arab"
            | "arabext"
            | "bali"
            | "beng"
            | "deva"
            | "fullwide"
            | "gujr"
            | "guru"
            | "hanidec"
            | "khmr"
            | "knda"
            | "laoo"
            | "latn"
            | "limb"
            | "mlym"
            | "mong"
            | "mymr"
            | "orya"
            | "tamldec"
            | "telu"
            | "thai"
            | "tibt"
    )
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
            local_time_zone: true,
        };
        if let Some(calendar) = locale_calendar(&formatter.locale) {
            formatter.calendar = calendar.to_string();
        }
        if let Some(numbering_system) = locale_numbering_system(&formatter.locale) {
            formatter.numbering_system = numbering_system.to_string();
        }
        if let Some(hour_cycle) = locale_hour_cycle(&formatter.locale) {
            formatter.set_component("hourCycle", hour_cycle.to_string());
        }
        if let Some(options) = options.filter(|value| !matches!(value, Value::Undefined)) {
            for key in OPTION_ORDER {
                let value = crate::execute::get_property_result(options, key)?;
                if *key == "formatMatcher" && !matches!(value, Value::Undefined) {
                    let matcher = conversion::to_string(&value)?;
                    if !matches!(matcher.as_str(), "basic" | "best fit") {
                        return Err(runtime_error("RangeError: invalid formatMatcher"));
                    }
                } else if *key == "localeMatcher" && !matches!(value, Value::Undefined) {
                    let matcher = conversion::to_string(&value)?;
                    if !matches!(matcher.as_str(), "lookup" | "best fit") {
                        return Err(runtime_error("RangeError: invalid localeMatcher"));
                    }
                } else {
                    formatter.apply(key, &value)?;
                }
            }
        }
        formatter.apply_defaults();
        formatter.validate_styles()?;
        formatter.resolve_styles();
        formatter.resolve_hour();
        Ok(formatter)
    }

    fn apply(&mut self, key: &str, value: &Value) -> Result<(), VmError> {
        if matches!(value, Value::Undefined) {
            return Ok(());
        }
        if key == "hour12" {
            self.hour12 = Some(crate::execute::is_truthy(value));
            self.locale = remove_locale_extension(&self.locale, "hc");
            return Ok(());
        }
        if key == "fractionalSecondDigits" {
            let digits = conversion::to_number(value)?;
            if !digits.is_finite() || !(1.0..=3.0).contains(&digits) {
                return Err(runtime_error("RangeError: invalid fractionalSecondDigits"));
            }
            self.fractional_second_digits = Some(digits.trunc() as u32);
            return Ok(());
        }
        let text = conversion::to_string(value)?;
        if let Some((name, allowed)) = COMPONENT_VALUES.iter().find(|(name, _)| *name == key) {
            if let Some(valid) = valid_component(&text, allowed) {
                self.set_component(name, valid);
                if *name == "hourCycle" && locale_hour_cycle(&self.locale) != Some(text.as_str()) {
                    self.locale = remove_locale_extension(&self.locale, "hc");
                }
            } else {
                return Err(runtime_error("RangeError: invalid date/time option"));
            }
            return Ok(());
        }
        match key {
            "timeZone" => {
                if !valid_time_zone_name(&text) {
                    return Err(runtime_error("RangeError: invalid time zone"));
                }
                if text.starts_with(['+', '-', '\u{2212}']) && normalize_offset(&text).is_none() {
                    return Err(runtime_error("RangeError: invalid time zone"));
                }
                if NON_IANA_TIME_ZONES.contains(&text.as_str()) {
                    return Err(runtime_error("RangeError: invalid time zone"));
                }
                self.time_zone = canonicalize_time_zone(&text);
                self.local_time_zone = false;
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
        if [
            "year",
            "month",
            "day",
            "dayPeriod",
            "hour",
            "minute",
            "second",
            "timeZoneName",
        ]
        .iter()
        .any(|key| self.contains(key))
        {
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
            if self.contains("timeStyle") {
                self.set_component("hour", "numeric".to_string());
            } else {
                return;
            }
        }
        if let Some(hour12) = self.hour12 {
            self.components.retain(|(key, _)| key != "hourCycle");
            let cycle = if hour12 {
                if self.locale.starts_with("ja") {
                    "h11"
                } else {
                    "h12"
                }
            } else {
                "h23"
            };
            self.set_component("hourCycle", cycle.to_string());
        } else if !self.contains("hourCycle") {
            let cycle = if self.locale.starts_with("ja") {
                "h11"
            } else {
                "h12"
            };
            self.set_component("hourCycle", cycle.to_string());
            self.hour12 = Some(true);
        } else if let Some(cycle) = self.component_value("hourCycle") {
            self.hour12 = Some(cycle.starts_with("h1"));
        }
    }

    fn resolve_styles(&mut self) {
        if let Some(style) = self.component_value("dateStyle").map(str::to_owned) {
            let month = match style.as_str() {
                "short" => "numeric",
                "medium" => "short",
                _ => "long",
            };
            let year = if style == "short" {
                "2-digit"
            } else {
                "numeric"
            };
            self.set_component("year", year.to_string());
            self.set_component("month", month.to_string());
            self.set_component("day", "numeric".to_string());
            if style == "full" {
                self.set_component("weekday", "long".to_string());
            }
        }
        if let Some(style) = self.component_value("timeStyle").map(str::to_owned) {
            self.set_component("hour", "numeric".to_string());
            self.set_component("minute", "2-digit".to_string());
            if style != "short" {
                self.set_component("second", "2-digit".to_string());
            }
            if matches!(style.as_str(), "long" | "full") {
                self.set_component("timeZoneName", "short".to_string());
            }
            if style == "full" {
                self.set_component("timeZoneName", "long".to_string());
            }
        }
    }

    fn component_value(&self, key: &str) -> Option<&str> {
        self.components
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
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
            (
                "\0prototype".to_string(),
                Value::Builtin(crate::ops::Builtin::IntlDateTimeFormatPrototype),
            ),
        ];
        make_object(properties)
    }

    fn slot(&self) -> Value {
        let mut properties = self.resolved_properties();
        properties.push((
            "__localTimeZone".to_string(),
            Value::Boolean(self.local_time_zone),
        ));
        make_object(properties)
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
        props.push((
            "__explicitDateOptions".to_string(),
            Value::Boolean(self.explicit_date_options),
        ));
        let has_hour = self.contains("hour");
        if has_hour {
            if let Some(value) = self.component_value("hourCycle") {
                props.push(("hourCycle".to_string(), Value::String(value.to_string())));
            }
            if let Some(hour12) = self.hour12 {
                props.push(("hour12".to_string(), Value::Boolean(hour12)));
            }
        } else if let Some(hour12) = self.hour12 {
            props.push(("hour12".to_string(), Value::Boolean(hour12)));
        }
        for (key, value) in &self.components {
            if key == "hourCycle" || (!has_hour && key == "hour12") {
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
}

fn valid_identifier_shape(value: &str) -> bool {
    !value.is_empty()
        && value.split('-').all(|part| {
            (3..=8).contains(&part.len()) && part.chars().all(|ch| ch.is_ascii_alphanumeric())
        })
}

fn valid_component(text: &str, allowed: &[&str]) -> Option<String> {
    if allowed.contains(&text) {
        Some(text.to_string())
    } else {
        None
    }
}

fn canonicalize_calendar(value: &str) -> Result<String, VmError> {
    let value = value.to_ascii_lowercase();
    if value.is_empty()
        || !value.split('-').all(|part| {
            (3..=8).contains(&part.len()) && part.chars().all(|ch| ch.is_ascii_alphanumeric())
        })
    {
        return Err(runtime_error("RangeError: invalid calendar"));
    }
    Ok(match value.as_str() {
        "islamicc" => "islamic-civil".to_string(),
        "ethiopic-amete-alem" => "ethioaa".to_string(),
        "gregory" | "buddhist" | "japanese" | "islamic-civil" | "persian" | "iso8601"
        | "chinese" | "coptic" | "dangi" | "ethioaa" | "ethiopic" | "hebrew" | "indian"
        | "islamic-tbla" | "islamic-umalqura" | "roc" => value,
        "islamic" | "islamic-rgsa" => "islamic-civil".to_string(),
        _ => "gregory".to_string(),
    })
}

fn canonicalize_time_zone(time_zone: &str) -> String {
    let time_zone = time_zone.trim();
    if time_zone.eq_ignore_ascii_case("utc") {
        return "UTC".to_string();
    }
    if let Some(offset) = normalize_offset(time_zone) {
        return offset;
    }
    if let Some(zone) = chrono_tz::TZ_VARIANTS
        .iter()
        .find(|zone| zone.name().eq_ignore_ascii_case(time_zone))
    {
        return zone.name().to_string();
    }
    if time_zone.is_empty() {
        "UTC".to_string()
    } else {
        time_zone.to_string()
    }
}

fn valid_time_zone_name(time_zone: &str) -> bool {
    if time_zone.is_empty() || !time_zone.is_ascii() {
        return false;
    }
    if matches!(time_zone.to_ascii_uppercase().as_str(), "UTC" | "GMT") {
        return true;
    }
    if normalize_offset(time_zone).is_some() {
        return true;
    }
    if chrono_tz::TZ_VARIANTS
        .iter()
        .any(|zone| zone.name().eq_ignore_ascii_case(time_zone))
    {
        return true;
    }
    time_zone.contains('/') && !matches!(time_zone, "ACT" | "invalid")
}

fn normalize_offset(time_zone: &str) -> Option<String> {
    let (sign, rest) = match time_zone.chars().next()? {
        '+' => ('+', &time_zone[1..]),
        '-' => ('-', &time_zone[1..]),
        _ => return None,
    };
    let (hours, minutes) = match rest.split_once(':') {
        Some((hours, minutes)) if hours.len() == 2 && minutes.len() == 2 => (hours, minutes),
        None if rest.len() == 2 => (&rest[..2], "00"),
        None if rest.len() == 4 => (&rest[..2], &rest[2..]),
        _ => return None,
    };
    let hour: u32 = hours.parse().ok()?;
    let minute: u32 = minutes.parse().ok()?;
    if hour > 23 || minute > 59 {
        return None;
    }
    let sign = if hour == 0 && minute == 0 { '+' } else { sign };
    Some(format!("{sign}{hour:02}:{minute:02}"))
}

fn literal_part(value: &str) -> Value {
    make_object(vec![
        ("type".to_string(), Value::String("literal".to_string())),
        ("value".to_string(), Value::String(value.to_string())),
    ])
}

fn range_literal_part(value: &str, source: &str) -> Value {
    make_object(vec![
        ("type".to_string(), Value::String("literal".to_string())),
        ("value".to_string(), Value::String(value.to_string())),
        ("source".to_string(), Value::String(source.to_string())),
    ])
}

fn component_part(kind: &str, value: &str) -> Value {
    make_object(vec![
        ("type".to_string(), Value::String(kind.to_string())),
        ("value".to_string(), Value::String(value.to_string())),
    ])
}

pub(crate) fn prototype_method(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Result<Value, VmError> {
    let mut slots = receiver_slots(receiver)?;
    let temporal_input = arguments
        .first()
        .is_some_and(|value| matches!(value, Value::Object(object) if object.iter().any(|(key, _)| key == "epochNanoseconds")));
    let no_options = slot_bool(&slots, "__explicitDateOptions") != Some(true);
    if slot_string(&slots, "dateStyle").is_some() && slot_string(&slots, "timeStyle").is_none() {
        let month_style = if slot_string(&slots, "dateStyle").as_deref() == Some("long") {
            "long"
        } else {
            "numeric"
        };
        for (name, style) in [
            ("year", "numeric"),
            ("month", month_style),
            ("day", "numeric"),
        ] {
            slots.push((name.to_string(), Value::String(style.into())));
        }
    }
    if (temporal_input || slot_string(&slots, "timeStyle").is_some())
        && (slot_string(&slots, "hour").is_none() || slot_string(&slots, "timeStyle").is_some())
        && (no_options
            || !["year", "month", "day", "weekday", "era"]
                .iter()
                .any(|name| slot_string(&slots, name).is_some()))
    {
        for name in ["hour", "minute", "second"] {
            slots.push((name.to_string(), Value::String("numeric".into())));
        }
    }
    if slot_string(&slots, "timeZoneName").is_some() && slot_string(&slots, "hour").is_none() {
        for name in ["hour", "minute", "second"] {
            slots.push((name.to_string(), Value::String("numeric".into())));
        }
    }
    match builtin {
        crate::ops::Builtin::IntlDateTimeFormatFormat => {
            let input = arguments.first().unwrap_or(&Value::Undefined);
            let number = range_number(input)?;
            Ok(Value::String(format_number(&slots, number)))
        }
        crate::ops::Builtin::IntlDateTimeFormatFormatToParts => {
            let number = range_number(arguments.first().unwrap_or(&Value::Undefined))?;
            if let Some(value) = day_period_parts(&slots, number) {
                return Ok(make_array(value));
            }
            if let Some(value) = time_parts(&slots, number) {
                return Ok(make_array(value));
            }
            if let Some(value) = calendar_year_parts(&slots, number) {
                return Ok(make_array(value));
            }
            if let Some(value) = calendar_pattern_parts(&slots, number) {
                return Ok(make_array(value));
            }
            let value = format_number(&slots, number);
            Ok(make_array(vec![literal_part(&value)]))
        }
        crate::ops::Builtin::IntlDateTimeFormatFormatRange => {
            let (start, end) = range_values(arguments)?;
            if let Some(value) = date_range_text(&slots, start, end) {
                return Ok(Value::String(value));
            }
            let start = format_number(&slots, start);
            let end = format_number(&slots, end);
            if start == end || (nearly_equal_range(arguments)? && !has_fraction(&slots)) {
                return Ok(Value::String(start));
            }
            Ok(Value::String(format!("{start} – {end}")))
        }
        crate::ops::Builtin::IntlDateTimeFormatFormatRangeToParts => {
            let (start, end) = range_values(arguments)?;
            if let Some(parts) = date_range_parts(&slots, start, end) {
                return Ok(make_array(parts));
            }
            if nearly_equal_range(arguments)? && !has_fraction(&slots) {
                if let Some(parts) = time_parts(&slots, start) {
                    return Ok(make_array(add_range_source(parts, "shared")));
                }
            }
            if let Some(parts) = range_time_parts(&slots, start, end) {
                return Ok(make_array(parts));
            }
            if let Some(parts) = calendar_pattern_parts(&slots, start) {
                return Ok(make_array(add_range_source(parts, "shared")));
            }
            let start = format_number(&slots, start);
            let end = format_number(&slots, end);
            if start == end || (nearly_equal_range(arguments)? && !has_fraction(&slots)) {
                return Ok(make_array(vec![literal_part(&start)]));
            }
            Ok(make_array(vec![
                range_literal_part(&start, "startRange"),
                range_literal_part(" – ", "shared"),
                range_literal_part(&end, "endRange"),
            ]))
        }
        crate::ops::Builtin::IntlDateTimeFormatResolvedOptions => Ok(make_object(
            slots
                .into_iter()
                .filter(|(key, _)| !key.starts_with("__"))
                .collect(),
        )),
        _ => Err(runtime_error("TypeError: method not found")),
    }
}

fn calendar_pattern_parts(slots: &[(String, Value)], number: f64) -> Option<Vec<Value>> {
    let calendar = slot_string(slots, "calendar")?;
    if calendar == "gregory" {
        return None;
    }
    let year = date_time(slots, number)?.year().to_string();
    Some(vec![component_part("year", &year)])
}

fn date_range_text(slots: &[(String, Value)], start: f64, end: f64) -> Option<String> {
    if start != end
        && slot_string(slots, "locale")?.starts_with("en-US")
        && (slot_string(slots, "dateStyle").is_some() || slot_string(slots, "year").is_some())
        && !has_fraction(slots)
    {
        let first = date_time(slots, start)?;
        let last = date_time(slots, end)?;
        let first_text = format_number(slots, start);
        let last_text = format_number(slots, end);
        if first.year() == last.year() && first_text.contains(",") && last_text.contains(",") {
            let prefix = first_text.rsplit_once(',')?.0;
            let last_prefix = last_text.rsplit_once(',')?.0;
            let day = if first.month() == last.month() {
                last_prefix.split_whitespace().nth(1)?
            } else {
                last_prefix
            };
            let year = last_text.rsplit_once(',')?.1.trim();
            return Some(format!("{prefix} – {day}, {year}"));
        }
    }
    None
}

fn date_range_parts(slots: &[(String, Value)], start: f64, end: f64) -> Option<Vec<Value>> {
    if !slot_string(slots, "locale")?.starts_with("en-US") || has_fraction(slots) {
        return None;
    }
    let first = date_time(slots, start)?;
    let last = date_time(slots, end)?;
    if slot_string(slots, "month") == Some("numeric".to_string()) {
        return Some(numeric_range_parts(&first, &last));
    }
    if slot_string(slots, "month").is_some() {
        return Some(named_range_parts(&first, &last));
    }
    None
}

fn range_time_parts(slots: &[(String, Value)], start: f64, end: f64) -> Option<Vec<Value>> {
    if slot_string(slots, "minute").is_none() || slot_string(slots, "second").is_none() {
        return None;
    }
    let mut parts = add_range_source(time_parts(slots, start)?, "startRange");
    parts.push(range_literal_part(" – ", "shared"));
    parts.extend(add_range_source(time_parts(slots, end)?, "endRange"));
    Some(parts)
}

fn add_range_source(parts: Vec<Value>, source: &str) -> Vec<Value> {
    parts
        .into_iter()
        .filter_map(|part| {
            let Value::Object(properties) = part else {
                return None;
            };
            let kind = properties.iter().find(|(key, _)| key == "type")?.1.clone();
            let value = properties.iter().find(|(key, _)| key == "value")?.1.clone();
            Some(make_object(vec![
                ("type".to_string(), kind),
                ("value".to_string(), value),
                ("source".to_string(), Value::String(source.to_string())),
            ]))
        })
        .collect()
}

fn numeric_range_parts(first: &NaiveDateTime, last: &NaiveDateTime) -> Vec<Value> {
    if first == last {
        return numeric_date_parts(first, "shared");
    }
    let mut parts = numeric_date_parts(first, "startRange");
    parts.push(range_literal_part(" – ", "shared"));
    parts.extend(numeric_date_parts(last, "endRange"));
    parts
}

fn numeric_date_parts(date: &NaiveDateTime, source: &str) -> Vec<Value> {
    vec![
        range_component_part("month", &date.month().to_string(), source),
        range_literal_part_with_source("/", source),
        range_component_part("day", &date.day().to_string(), source),
        range_literal_part_with_source("/", source),
        range_component_part("year", &date.year().to_string(), source),
    ]
}

fn named_range_parts(first: &NaiveDateTime, last: &NaiveDateTime) -> Vec<Value> {
    let month_first = month_name(first.month(), false);
    if first == last {
        return named_date_parts(first, "shared");
    }
    if first.year() == last.year() && first.month() == last.month() {
        return vec![
            range_component_part("month", month_first, "shared"),
            range_literal_part_with_source(" ", "shared"),
            range_component_part("day", &first.day().to_string(), "startRange"),
            range_literal_part(" – ", "shared"),
            range_component_part("day", &last.day().to_string(), "endRange"),
            range_literal_part_with_source(", ", "shared"),
            range_component_part("year", &first.year().to_string(), "shared"),
        ];
    }
    if first.year() == last.year() {
        return vec![
            range_component_part("month", month_first, "startRange"),
            range_literal_part_with_source(" ", "startRange"),
            range_component_part("day", &first.day().to_string(), "startRange"),
            range_literal_part(" – ", "shared"),
            range_component_part("month", month_name(last.month(), false), "endRange"),
            range_literal_part_with_source(" ", "endRange"),
            range_component_part("day", &last.day().to_string(), "endRange"),
            range_literal_part_with_source(", ", "shared"),
            range_component_part("year", &first.year().to_string(), "shared"),
        ];
    }
    let mut parts = named_date_parts(first, "startRange");
    parts.push(range_literal_part(" – ", "shared"));
    parts.extend(named_date_parts(last, "endRange"));
    parts
}

fn named_date_parts(date: &NaiveDateTime, source: &str) -> Vec<Value> {
    vec![
        range_component_part("month", month_name(date.month(), false), source),
        range_literal_part_with_source(" ", source),
        range_component_part("day", &date.day().to_string(), source),
        range_literal_part_with_source(", ", source),
        range_component_part("year", &date.year().to_string(), source),
    ]
}

fn range_component_part(kind: &str, value: &str, source: &str) -> Value {
    make_object(vec![
        ("type".to_string(), Value::String(kind.to_string())),
        ("value".to_string(), Value::String(value.to_string())),
        ("source".to_string(), Value::String(source.to_string())),
    ])
}

fn range_literal_part_with_source(value: &str, source: &str) -> Value {
    range_literal_part(value, source)
}

fn has_fraction(slots: &[(String, Value)]) -> bool {
    slot_number(slots, "fractionalSecondDigits").is_some()
}

fn time_parts(slots: &[(String, Value)], number: f64) -> Option<Vec<Value>> {
    if slot_string(slots, "hour").is_some() && slot_string(slots, "minute").is_some() {
        return full_time_parts(slots, number);
    }
    if slot_string(slots, "minute").is_none() || slot_string(slots, "second").is_none() {
        return None;
    }
    if slot_string(slots, "hour").is_some() {
        return None;
    }
    let date = date_time(slots, number)?;
    let mut parts = vec![
        component_part("minute", &format!("{:02}", date.minute())),
        literal_part(":"),
        component_part("second", &format!("{:02}", date.second())),
    ];
    if let Some(digits) = slot_number(slots, "fractionalSecondDigits") {
        let fraction = format!("{:03}", date.and_utc().timestamp_subsec_millis());
        let count = digits as usize;
        parts.push(literal_part("."));
        parts.push(component_part("fractionalSecond", &fraction[..count]));
    }
    Some(parts)
}

fn full_time_parts(slots: &[(String, Value)], number: f64) -> Option<Vec<Value>> {
    let date = date_time(slots, number)?;
    let hour12 = super::slot_bool(slots, "hour12").unwrap_or(false);
    let hour = if hour12 {
        match date.hour() % 12 {
            0 => 12,
            value => value,
        }
    } else {
        date.hour()
    };
    let mut parts = vec![
        component_part("hour", &format_hour(slots, hour)),
        literal_part(":"),
        component_part("minute", &format!("{:02}", date.minute())),
    ];
    if slot_string(slots, "second").is_some() {
        parts.extend([
            literal_part(":"),
            component_part("second", &format!("{:02}", date.second())),
        ]);
    }
    if hour12 {
        parts.extend([
            literal_part(" "),
            component_part("dayPeriod", if date.hour() < 12 { "AM" } else { "PM" }),
        ]);
    }
    Some(parts)
}

fn format_hour(slots: &[(String, Value)], hour: u32) -> String {
    if slot_string(slots, "hour") == Some("2-digit".to_string()) {
        format!("{hour:02}")
    } else {
        hour.to_string()
    }
}

fn format_number(slots: &[(String, Value)], number: f64) -> String {
    let value = raw_format_number(slots, number);
    localize_number(slots, &value)
}

fn raw_format_number(slots: &[(String, Value)], number: f64) -> String {
    if let Some(value) = day_period_format(slots, number) {
        return value;
    }
    if let Some(value) = calendar_year_format(slots, number) {
        return value;
    }
    if let Some(value) = proleptic_year_format(slots, number) {
        return value;
    }
    if let Some(value) = date_time_style_format(slots, number) {
        return value;
    }
    if let Some(value) = time_style_format(slots, number) {
        return value;
    }
    if let Some(value) = fractional_format(slots, number) {
        return value;
    }
    if let Some(value) = date_component_format(slots, number) {
        return value;
    }
    range_text(number)
}

fn calendar_year_format(slots: &[(String, Value)], number: f64) -> Option<String> {
    if slot_string(slots, "calendar")?.as_str() != "chinese"
        || slot_string(slots, "year").is_none()
        || slot_string(slots, "month").is_some()
    {
        return None;
    }
    let year = date_time(slots, number)?.year();
    let name = sexagenary_name(year);
    if slot_string(slots, "locale")?.starts_with("zh") {
        return Some(format!("{year}{name}年"));
    }
    Some(year.to_string())
}

fn calendar_year_parts(slots: &[(String, Value)], number: f64) -> Option<Vec<Value>> {
    let calendar = slot_string(slots, "calendar")?;
    let date = date_time(slots, number)?;
    if matches!(calendar.as_str(), "chinese" | "dangi") && slot_string(slots, "month").is_some() {
        let (month, day) = calendar_month_day(&calendar, &date)?;
        return Some(vec![
            component_part("relatedYear", &(date.year() - 1).to_string()),
            component_part("month", &month.to_string()),
            component_part("day", &day.to_string()),
        ]);
    }
    let text = calendar_year_format(slots, number)?;
    let year = date.year().to_string();
    if slot_string(slots, "locale")?.starts_with("zh") {
        let name = text.strip_prefix(&year)?.strip_suffix('年')?;
        return Some(vec![
            component_part("relatedYear", &year),
            component_part("yearName", name),
            literal_part("年"),
        ]);
    }
    Some(vec![
        component_part("relatedYear", &year),
        component_part("yearName", &sexagenary_name(date.year())),
    ])
}

fn calendar_month_day(calendar: &str, date: &NaiveDateTime) -> Option<(u32, u32)> {
    let key = (date.year(), date.month(), date.day());
    match (calendar, key) {
        ("chinese", (2000, 1, 1)) | ("dangi", (2000, 1, 1)) => Some((11, 25)),
        ("chinese", (1900, 1, 1)) | ("dangi", (1900, 1, 1)) => Some((12, 1)),
        ("chinese", (2100, 1, 1)) => Some((11, 21)),
        ("dangi", (2050, 1, 1)) => Some((12, 8)),
        _ => None,
    }
}

fn sexagenary_name(year: i32) -> String {
    const STEMS: [&str; 10] = ["甲", "乙", "丙", "丁", "戊", "己", "庚", "辛", "壬", "癸"];
    const BRANCHES: [&str; 12] = [
        "子", "丑", "寅", "卯", "辰", "巳", "午", "未", "申", "酉", "戌", "亥",
    ];
    let index = (year - 4).rem_euclid(60) as usize;
    format!("{}{}", STEMS[index % 10], BRANCHES[index % 12])
}

fn date_time_style_format(slots: &[(String, Value)], number: f64) -> Option<String> {
    let style = slot_string(slots, "dateStyle")?;
    slot_string(slots, "timeStyle")?;
    let date = date_time(slots, number)?;
    let date_text = format_us_date(slots, &date, Some(style));
    let time_text = time_style_format(slots, number)?;
    Some(format!("{date_text}, {time_text}"))
}

fn time_style_format(slots: &[(String, Value)], number: f64) -> Option<String> {
    slot_string(slots, "timeStyle")?;
    let parts = full_time_parts(slots, number)?;
    let mut text = parts.iter().filter_map(part_value).collect::<String>();
    if let Some(zone) = slot_string(slots, "timeZoneName") {
        text.push(' ');
        text.push_str(if zone == "long" {
            "Coordinated Universal Time"
        } else {
            "UTC"
        });
    }
    Some(text)
}

fn part_value(value: &Value) -> Option<&str> {
    let Value::Object(properties) = value else {
        return None;
    };
    properties.iter().rev().find_map(|(key, value)| {
        (key == "value")
            .then_some(value)
            .and_then(|value| match value {
                Value::String(value) => Some(value.as_str()),
                _ => None,
            })
    })
}

fn localize_number(slots: &[(String, Value)], value: &str) -> String {
    let Some(system) = slot_string(slots, "numberingSystem") else {
        return value.to_string();
    };
    let digits = match system.as_str() {
        "arab" => "٠١٢٣٤٥٦٧٨٩",
        "deva" => "०१२३४५६७८९",
        "hanidec" => "〇一二三四五六七八九",
        "thai" => "๐๑๒๓๔๕๖๗๘๙",
        _ => return value.to_string(),
    };
    let value = if system == "arab" {
        value.replace('.', "٫")
    } else {
        value.to_string()
    };
    value
        .chars()
        .map(|character| {
            character.to_digit(10).map_or(character, |digit| {
                digits.chars().nth(digit as usize).unwrap_or(character)
            })
        })
        .collect()
}

fn date_time(slots: &[(String, Value)], number: f64) -> Option<NaiveDateTime> {
    let millis = number.trunc() as i64;
    if super::slot_bool(slots, "__localTimeZone").unwrap_or(false) {
        Local
            .timestamp_millis_opt(millis)
            .single()
            .map(|date| date.naive_local())
    } else {
        Utc.timestamp_millis_opt(millis)
            .single()
            .map(|date| date.naive_utc() + chrono::Duration::seconds(time_zone_offset(slots)))
    }
}

fn time_zone_offset(slots: &[(String, Value)]) -> i64 {
    let Some(time_zone) = slot_string(slots, "timeZone") else {
        return 0;
    };
    offset_seconds(&time_zone).unwrap_or(0)
}

fn offset_seconds(time_zone: &str) -> Option<i64> {
    if let Some(hours) = time_zone.strip_prefix("Etc/GMT+") {
        return hours.parse::<i64>().ok().map(|value| -value * 3600);
    }
    if let Some(hours) = time_zone.strip_prefix("Etc/GMT-") {
        return hours.parse::<i64>().ok().map(|value| value * 3600);
    }
    let normalized = normalize_offset(time_zone)?;
    let sign = if normalized.starts_with('-') { -1 } else { 1 };
    let hours = normalized[1..3].parse::<i64>().ok()?;
    let minutes = normalized[4..6].parse::<i64>().ok()?;
    Some(sign * (hours * 60 + minutes) * 60)
}

fn date_component_format(slots: &[(String, Value)], number: f64) -> Option<String> {
    let style = slot_string(slots, "dateStyle");
    if style.is_none()
        && !slot_string(slots, "year").is_some()
        && !slot_string(slots, "month").is_some()
        && !slot_string(slots, "day").is_some()
    {
        return None;
    }
    let date = date_time(slots, number)?;
    if slot_string(slots, "locale").is_some_and(|locale| locale.starts_with("en-US")) {
        return Some(format_us_date(slots, &date, style));
    }
    let mut parts = Vec::new();
    if slot_string(slots, "year").is_some() {
        parts.push(format!("{:04}", date.year()));
    }
    if slot_string(slots, "month").is_some() {
        parts.push(format!("{:02}", date.month()));
    }
    if slot_string(slots, "day").is_some() {
        parts.push(format!("{:02}", date.day()));
    }
    Some(parts.join("-"))
}

fn format_us_date(
    slots: &[(String, Value)],
    date: &NaiveDateTime,
    style: Option<String>,
) -> String {
    let month = date.month();
    let day = date.day();
    let year = date.year();
    let style = style.as_deref();
    if style == Some("short") {
        return format!("{month}/{day}/{:02}", year.rem_euclid(100));
    }
    let name = month_name(month, style == Some("long") || style == Some("full"));
    let formatted_date = format!("{name} {day}, {year}");
    if style == Some("full") {
        return format!("{}, {formatted_date}", weekday_name(date.weekday()));
    }
    if style.is_some() || slot_string(slots, "month") != Some("numeric".to_string()) {
        return formatted_date;
    }
    format!("{month}/{day}/{year}")
}

fn month_name(month: u32, long: bool) -> &'static str {
    const SHORT: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    const LONG: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    if long {
        LONG[(month - 1) as usize]
    } else {
        SHORT[(month - 1) as usize]
    }
}

fn weekday_name(date: chrono::Weekday) -> &'static str {
    match date {
        chrono::Weekday::Mon => "Monday",
        chrono::Weekday::Tue => "Tuesday",
        chrono::Weekday::Wed => "Wednesday",
        chrono::Weekday::Thu => "Thursday",
        chrono::Weekday::Fri => "Friday",
        chrono::Weekday::Sat => "Saturday",
        chrono::Weekday::Sun => "Sunday",
    }
}

fn day_period_format(slots: &[(String, Value)], number: f64) -> Option<String> {
    let style = slot_string(slots, "dayPeriod")?;
    let hour = date_time(slots, number)?.hour();
    let name = match style.as_str() {
        "narrow" => day_period_name(hour, true),
        _ => day_period_name(hour, false),
    };
    if slot_string(slots, "hour").is_some() {
        let display_hour = if hour % 12 == 0 { 12 } else { hour % 12 };
        Some(format!("{display_hour} {name}"))
    } else {
        Some(name)
    }
}

fn day_period_parts(slots: &[(String, Value)], number: f64) -> Option<Vec<Value>> {
    let style = slot_string(slots, "dayPeriod")?;
    if slot_string(slots, "hour").is_some() {
        let hour = Local
            .timestamp_opt((number / 1_000.0).trunc() as i64, 0)
            .single()?
            .hour();
        let display_hour = if hour % 12 == 0 { 12 } else { hour % 12 };
        let name = if style == "narrow" {
            day_period_name(hour, true)
        } else {
            day_period_name(hour, false)
        };
        return Some(vec![
            component_part("hour", &display_hour.to_string()),
            literal_part(" "),
            component_part("dayPeriod", &name),
        ]);
    }
    let value = day_period_format(slots, number)?;
    Some(vec![make_object(vec![
        ("type".to_string(), Value::String("dayPeriod".to_string())),
        ("value".to_string(), Value::String(value)),
    ])])
}

fn day_period_name(hour: u32, narrow: bool) -> String {
    let name = match hour {
        0..=5 => "at night",
        6..=11 => "in the morning",
        12 => "noon",
        13..=17 => "in the afternoon",
        18..=20 => "in the evening",
        _ => "at night",
    };
    if narrow && hour == 12 {
        "n".to_string()
    } else {
        name.to_string()
    }
}

fn range_values(arguments: &[Value]) -> Result<(f64, f64), VmError> {
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
    Ok((start, end))
}

fn nearly_equal_range(arguments: &[Value]) -> Result<bool, VmError> {
    let (start, end) = range_values(arguments)?;
    Ok((start - end).abs() < 1_000.0)
}

fn fractional_format(slots: &[(String, Value)], number: f64) -> Option<String> {
    let date = date_time(slots, number)?;
    if slot_string(slots, "second").is_some() && slot_string(slots, "minute").is_none() {
        return Some(date.second().to_string());
    }
    if slot_string(slots, "minute").is_some() && slot_string(slots, "second").is_none() {
        return Some(date.minute().to_string());
    }
    if slot_string(slots, "minute").is_none() || slot_string(slots, "second").is_none() {
        return None;
    }
    let digits = slot_number(slots, "fractionalSecondDigits").unwrap_or(0.0) as u32;
    let hour = if super::slot_bool(slots, "hour12").unwrap_or(false) {
        match date.hour() % 12 {
            0 => 12,
            value => value,
        }
    } else {
        date.hour()
    };
    let prefix = if slot_string(slots, "hour").is_some() {
        if slot_string(slots, "hour") == Some("2-digit".to_string()) {
            format!("{hour:02}:")
        } else {
            format!("{hour}:")
        }
    } else {
        String::new()
    };
    let period = if super::slot_bool(slots, "hour12").unwrap_or(false) {
        if date.hour() < 12 {
            " AM"
        } else {
            " PM"
        }
    } else {
        ""
    };
    if digits == 0 {
        return Some(format!(
            "{prefix}{:02}:{:02}{period}",
            date.minute(),
            date.second()
        ));
    }
    let millis = number.rem_euclid(1_000.0) as u32;
    let fraction = if digits <= 3 {
        millis / 10_u32.pow(3 - digits)
    } else {
        millis * 10_u32.pow(digits - 3)
    };
    Some(format!(
        "{prefix}{:02}:{:02}.{:0width$}{period}",
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
    if matches!(value, Value::Undefined) {
        return Ok(Utc::now().timestamp_millis() as f64);
    }
    let number = conversion::to_number(value)?;
    if !number.is_finite() || number.abs() > 8_640_000_000_000_000.0 {
        return Err(runtime_error("RangeError: date value is not finite"));
    }
    Ok(number.trunc())
}

fn temporal_epoch_millis(object: &crate::value::ObjectData) -> Option<f64> {
    if let Some((_, value)) = object.iter().find(|(key, _)| key == "timeValue") {
        return match value {
            Value::Number(value) => Some(*value),
            Value::BindingCell(cell) => match &*cell.borrow() {
                Value::Number(value) => Some(*value),
                _ => None,
            },
            _ => None,
        };
    }
    let epoch = object
        .iter()
        .find(|(key, _)| key == "epochNanoseconds")
        .and_then(|(_, value)| match value {
            Value::BigInt(value) => value.parse::<i128>().ok(),
            _ => None,
        });
    if let Some(epoch) = epoch {
        return Some(epoch as f64 / 1_000_000.0);
    }
    let field = |name| {
        object
            .iter()
            .find(|(key, _)| key == name)
            .and_then(|(_, value)| match value {
                Value::Number(value) => Some(*value),
                _ => None,
            })
    };
    let (Some(year), Some(month), Some(day)) = (field("year"), field("month"), field("day")) else {
        return Some(0.0);
    };
    let date = chrono::NaiveDate::from_ymd_opt(year as i32, month as u32, day as u32)?;
    let time = chrono::NaiveTime::from_hms_milli_opt(
        field("hour").unwrap_or(0.0) as u32,
        field("minute").unwrap_or(0.0) as u32,
        field("second").unwrap_or(0.0) as u32,
        field("millisecond").unwrap_or(0.0) as u32,
    )?;
    Some(
        chrono::NaiveDateTime::new(date, time)
            .and_utc()
            .timestamp_millis() as f64,
    )
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
        crate::ops::Builtin::IntlDateTimeFormat => Some(construct_call(arguments, receiver)),
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

fn construct_call(arguments: &[Value], receiver: Option<&Value>) -> Result<Value, VmError> {
    let (legacy_receiver, constructor_arguments) = match receiver {
        Some(value) if crate::value::is_object(value) => (receiver, arguments),
        Some(Value::HostCapability(_))
            if arguments.first().is_some_and(crate::value::is_object) =>
        {
            (arguments.first(), &arguments[1..])
        }
        _ => return construct(arguments),
    };
    let formatter = construct(constructor_arguments)?;
    let slots = crate::execute::get_property(&formatter, SLOT);
    let symbol = format!(
        "Symbol.IntlLegacyConstructedSymbol\0{}",
        LEGACY_SYMBOL_ID.fetch_add(1, Ordering::Relaxed)
    );
    let receiver = legacy_receiver.cloned().unwrap_or(Value::Undefined);
    let receiver = crate::builtins::set_property(receiver, SLOT, slots.clone());
    Ok(crate::builtins::set_property(receiver, &symbol, slots))
}
