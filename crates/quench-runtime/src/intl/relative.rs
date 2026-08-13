//! `Intl.RelativeTimeFormat`.

use crate::{execute::VmError, value::Value};

use super::{
    default_locale, make_array, make_object, resolve_locales, runtime_error, slot_string,
    to_string_value, SLOT,
};

pub(crate) struct RelativeOptions {
    locale: String,
    style: String,
    numeric: String,
}

/// A single formatted part: a type, its text, and whether it carries the unit.
struct Part {
    ty: &'static str,
    value: String,
    unit: bool,
}

pub(crate) fn construct(arguments: &[Value]) -> Result<Value, VmError> {
    let locales = resolve_locales(arguments)?;
    let locale = locales.first().cloned().unwrap_or_else(default_locale);
    let options = RelativeOptions::from_options(locale, arguments.get(1))?;
    Ok(options.build_object())
}

impl RelativeOptions {
    fn from_options(locale: String, options: Option<&Value>) -> Result<Self, VmError> {
        let mut formatter = RelativeOptions {
            locale,
            style: "long".to_string(),
            numeric: "always".to_string(),
        };
        if let Some(Value::Object(properties)) = options {
            for (key, value) in properties.iter() {
                if matches!(value, Value::Undefined) {
                    continue;
                }
                let text = to_string_value(value);
                match key.as_str() {
                    "style" => {
                        if let Some(style) = valid_enum(&text, &["long", "short", "narrow"]) {
                            formatter.style = style;
                        } else {
                            return Err(runtime_error("RangeError: invalid style"));
                        }
                    }
                    "numeric" => {
                        if let Some(numeric) = valid_enum(&text, &["always", "auto"]) {
                            formatter.numeric = numeric;
                        } else {
                            return Err(runtime_error("RangeError: invalid numeric"));
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(formatter)
    }

    fn build_object(&self) -> Value {
        let properties = vec![
            (
                "format".to_string(),
                Value::Builtin(crate::ops::Builtin::IntlRelativeTimeFormatFormat),
            ),
            (
                "formatToParts".to_string(),
                Value::Builtin(crate::ops::Builtin::IntlRelativeTimeFormatFormatToParts),
            ),
            (
                "resolvedOptions".to_string(),
                Value::Builtin(crate::ops::Builtin::IntlRelativeTimeFormatResolvedOptions),
            ),
            (SLOT.to_string(), self.slot()),
        ];
        make_object(properties)
    }

    fn slot(&self) -> Value {
        make_object(vec![
            ("locale".to_string(), Value::String(self.locale.clone())),
            ("style".to_string(), Value::String(self.style.clone())),
            ("numeric".to_string(), Value::String(self.numeric.clone())),
            (
                "numberingSystem".to_string(),
                Value::String("latn".to_string()),
            ),
        ])
    }
}

fn valid_enum(text: &str, allowed: &[&str]) -> Option<String> {
    if allowed.contains(&text) {
        Some(text.to_string())
    } else {
        None
    }
}

fn singularize(unit: &str) -> String {
    match unit {
        "seconds" => "second".to_string(),
        "minutes" => "minute".to_string(),
        "hours" => "hour".to_string(),
        "days" => "day".to_string(),
        "weeks" => "week".to_string(),
        "months" => "month".to_string(),
        "quarters" => "quarter".to_string(),
        "years" => "year".to_string(),
        _ => unit.to_string(),
    }
}

fn valid_unit(unit: &str) -> Result<String, VmError> {
    let unit = singularize(unit);
    if matches!(
        unit.as_str(),
        "second" | "minute" | "hour" | "day" | "week" | "month" | "quarter" | "year"
    ) {
        Ok(unit)
    } else {
        Err(runtime_error("RangeError: invalid unit"))
    }
}

fn format_relative(value: f64, unit: &str, style: &str, numeric: &str) -> Result<String, VmError> {
    let unit = valid_unit(unit)?;
    let parts = phrase_parts(value, &unit, style, numeric);
    Ok(parts.iter().map(|part| part.value.clone()).collect())
}

fn parts_value(value: f64, unit: &str, style: &str, numeric: &str) -> Result<Value, VmError> {
    let unit = valid_unit(unit)?;
    let parts = phrase_parts(value, &unit, style, numeric);
    let values = parts
        .into_iter()
        .map(|part| part_object(part, &unit))
        .collect();
    Ok(make_array(values))
}

fn part_object(part: Part, unit: &str) -> Value {
    let mut properties = vec![
        ("type".to_string(), Value::String(part.ty.to_string())),
        ("value".to_string(), Value::String(part.value)),
    ];
    if part.unit {
        properties.push(("unit".to_string(), Value::String(unit.to_string())));
    }
    make_object(properties)
}

fn phrase_parts(value: f64, unit: &str, style: &str, numeric: &str) -> Vec<Part> {
    if numeric == "auto" {
        if let Some(exception) = auto_exception(value, unit) {
            return vec![Part {
                ty: "literal",
                value: exception,
                unit: false,
            }];
        }
    }
    numeric_parts(value, unit, style)
}

fn auto_exception(value: f64, unit: &str) -> Option<String> {
    match unit {
        "day" => day_word(value),
        "year" | "quarter" | "month" | "week" => period_word(value, unit),
        "hour" | "minute" | "second" => moment_word(value, unit),
        _ => None,
    }
}

fn day_word(value: f64) -> Option<String> {
    match value {
        1.0 => Some("tomorrow".to_string()),
        0.0 => Some("today".to_string()),
        -1.0 => Some("yesterday".to_string()),
        _ => None,
    }
}

fn period_word(value: f64, unit: &str) -> Option<String> {
    let word = match unit {
        "year" => "year",
        "quarter" => "quarter",
        "month" => "month",
        _ => "week",
    };
    let prefix = match value {
        1.0 => "next",
        0.0 => "this",
        -1.0 => "last",
        _ => return None,
    };
    Some(format!("{prefix} {word}"))
}

fn moment_word(value: f64, unit: &str) -> Option<String> {
    if value != 0.0 {
        return None;
    }
    let word = match unit {
        "hour" => "this hour",
        "minute" => "this minute",
        _ => "now",
    };
    Some(word.to_string())
}

fn numeric_parts(value: f64, unit: &str, style: &str) -> Vec<Part> {
    if value.is_nan() {
        return vec![Part {
            ty: "literal",
            value: "NaN".to_string(),
            unit: false,
        }];
    }
    let negative = value < 0.0;
    let magnitude = value.abs();
    let mut parts = Vec::new();
    if !negative {
        parts.push(Part {
            ty: "literal",
            value: "in ".to_string(),
            unit: false,
        });
    }
    let mut number = number_parts(&grouped_number(magnitude));
    for part in &mut number {
        part.unit = true;
    }
    parts.extend(number);
    let word = unit_word(unit, style, magnitude != 1.0);
    if negative {
        parts.push(Part {
            ty: "literal",
            value: format!(" {word} ago"),
            unit: false,
        });
    } else {
        parts.push(Part {
            ty: "literal",
            value: format!(" {word}"),
            unit: false,
        });
    }
    parts
}

fn grouped_number(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_string();
    }
    if value.is_infinite() {
        return "Infinity".to_string();
    }
    let mut text = format!("{value:.3}");
    if text.contains('.') {
        while text.ends_with('0') {
            text.pop();
        }
        if text.ends_with('.') {
            text.pop();
        }
    }
    group_integer(&text)
}

fn group_integer(text: &str) -> String {
    let (integer, fraction) = match text.split_once('.') {
        Some((integer, fraction)) => (integer, Some(fraction)),
        None => (text, None),
    };
    let mut grouped = String::new();
    for (index, character) in integer.chars().enumerate() {
        if index > 0 && (integer.len() - index) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(character);
    }
    if let Some(fraction) = fraction {
        grouped.push('.');
        grouped.push_str(fraction);
    }
    grouped
}

fn number_parts(text: &str) -> Vec<Part> {
    let mut parts = Vec::new();
    let mut digits = String::new();
    let mut fractional = false;
    for character in text.chars() {
        if character.is_ascii_digit() {
            digits.push(character);
        } else {
            flush_digits(&mut parts, &mut digits, fractional);
            match character {
                ',' => parts.push(Part {
                    ty: "group",
                    value: ",".to_string(),
                    unit: false,
                }),
                '.' => {
                    parts.push(Part {
                        ty: "decimal",
                        value: ".".to_string(),
                        unit: false,
                    });
                    fractional = true;
                }
                _ => {}
            }
        }
    }
    flush_digits(&mut parts, &mut digits, fractional);
    parts
}

fn flush_digits(parts: &mut Vec<Part>, digits: &mut String, fractional: bool) {
    if digits.is_empty() {
        return;
    }
    let ty = if fractional { "fraction" } else { "integer" };
    parts.push(Part {
        ty,
        value: std::mem::take(digits),
        unit: false,
    });
}

fn unit_word(unit: &str, style: &str, plural: bool) -> String {
    if style == "short" || style == "narrow" {
        short_word(unit, plural)
    } else {
        long_word(unit, plural)
    }
}

fn short_word(unit: &str, plural: bool) -> String {
    const WORDS: &[(&str, bool, &str)] = &[
        ("second", false, "sec."),
        ("second", true, "sec."),
        ("minute", false, "min."),
        ("minute", true, "min."),
        ("hour", false, "hr."),
        ("hour", true, "hr."),
        ("week", false, "wk."),
        ("week", true, "wk."),
        ("month", false, "mo."),
        ("month", true, "mo."),
        ("year", false, "yr."),
        ("year", true, "yr."),
        ("day", false, "day"),
        ("day", true, "days"),
        ("quarter", false, "qtr."),
        ("quarter", true, "qtrs."),
    ];
    WORDS
        .iter()
        .find(|(name, is_plural, _)| *name == unit && *is_plural == plural)
        .map_or_else(|| long_word(unit, plural), |(_, _, word)| word.to_string())
}

fn long_word(unit: &str, plural: bool) -> String {
    let (single, multi) = match unit {
        "second" => ("second", "seconds"),
        "minute" => ("minute", "minutes"),
        "hour" => ("hour", "hours"),
        "day" => ("day", "days"),
        "week" => ("week", "weeks"),
        "month" => ("month", "months"),
        "quarter" => ("quarter", "quarters"),
        "year" => ("year", "years"),
        _ => (unit, unit),
    };
    if plural {
        multi.to_string()
    } else {
        single.to_string()
    }
}

pub(crate) fn prototype_method(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Result<Value, VmError> {
    let slots = receiver_slots(receiver)?;
    let locale = slot_string(&slots, "locale").unwrap_or_else(default_locale);
    let style = slot_string(&slots, "style").unwrap_or_else(|| "long".to_string());
    let numeric = slot_string(&slots, "numeric").unwrap_or_else(|| "always".to_string());
    match builtin {
        crate::ops::Builtin::IntlRelativeTimeFormatFormat => {
            let value = super::number::to_number(arguments.first());
            let unit = to_string_value(arguments.get(1).unwrap_or(&Value::Undefined));
            Ok(Value::String(format_relative(
                value, &unit, &style, &numeric,
            )?))
        }
        crate::ops::Builtin::IntlRelativeTimeFormatFormatToParts => {
            let value = super::number::to_number(arguments.first());
            let unit = to_string_value(arguments.get(1).unwrap_or(&Value::Undefined));
            parts_value(value, &unit, &style, &numeric)
        }
        crate::ops::Builtin::IntlRelativeTimeFormatResolvedOptions => Ok(make_object(vec![
            ("locale".to_string(), Value::String(locale)),
            ("style".to_string(), Value::String(style)),
            ("numeric".to_string(), Value::String(numeric)),
            (
                "numberingSystem".to_string(),
                Value::String("latn".to_string()),
            ),
        ])),
        _ => Err(runtime_error("TypeError: method not found")),
    }
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
        crate::ops::Builtin::IntlRelativeTimeFormat => Some(construct(arguments)),
        crate::ops::Builtin::IntlRelativeTimeFormatFormat
        | crate::ops::Builtin::IntlRelativeTimeFormatFormatToParts
        | crate::ops::Builtin::IntlRelativeTimeFormatResolvedOptions => {
            Some(prototype_method(builtin, arguments, receiver))
        }
        _ => None,
    }
}
