//! `Intl.RelativeTimeFormat`.
use crate::{execute::VmError, value::Value};

use super::{
    default_locale, make_array, make_object, resolve_locales, runtime_error, slot_string, SLOT,
};
mod polish;
pub(crate) struct RelativeOptions {
    locale: String,
    style: String,
    numeric: String,
    numbering_system: String,
}
fn apply_unicode_extension(formatter: &mut RelativeOptions) -> (Option<String>, Option<String>) {
    let mut unicode_locale = None;
    let mut unicode_numbering = None;
    if let Some((base, extension)) = formatter.locale.split_once("-u-") {
        let parts: Vec<&str> = extension.split('-').collect();
        if let Some(value) = parts
            .windows(2)
            .find(|pair| pair[0] == "nu")
            .map(|pair| pair[1])
        {
            unicode_numbering = Some(value.to_string());
            if super::number::supports_digit_system(value) {
                formatter.numbering_system = value.to_string();
                unicode_locale = Some(formatter.locale.clone());
            }
        }
        formatter.locale = base.to_string();
    }
    (unicode_locale, unicode_numbering)
}

fn read_options(options: Option<&Value>, formatter: &mut RelativeOptions) -> Result<(), VmError> {
    if matches!(options, Some(Value::Null)) {
        return Err(crate::value::error::throw_type_error(
            "Cannot convert null to object",
        ));
    }
    let Some(options) = options.filter(|value| !matches!(value, Value::Undefined)) else {
        return Ok(());
    };
    let source = crate::construct::to_object(options)?;
    for key in ["localeMatcher", "numberingSystem", "style", "numeric"] {
        let value = crate::execute::get_property_result(&source, key)?;
        if !matches!(value, Value::Undefined) {
            apply_option(formatter, key, &value)?;
        }
    }
    Ok(())
}

/// A single formatted part: a type, its text, and whether it carries the unit.
pub(super) struct Part {
    pub(super) ty: &'static str,
    pub(super) value: String,
    pub(super) unit: bool,
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
            numbering_system: "latn".to_string(),
        };
        let (unicode_locale, unicode_numbering) = apply_unicode_extension(&mut formatter);
        read_options(options, &mut formatter)?;
        if let (Some(locale), Some(numbering)) = (unicode_locale, unicode_numbering) {
            if formatter.numbering_system == numbering {
                formatter.locale = locale;
            }
        }
        Ok(formatter)
    }

    fn build_object(&self) -> Value {
        let properties = vec![
            (
                "\0prototype".to_string(),
                Value::Builtin(crate::ops::Builtin::IntlRelativeTimeFormatPrototype),
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
                Value::String(self.numbering_system.clone()),
            ),
        ])
    }
}

fn apply_option(formatter: &mut RelativeOptions, key: &str, value: &Value) -> Result<(), VmError> {
    let text = crate::conversion::to_string(value)?;
    match key {
        "localeMatcher" if matches!(text.as_str(), "lookup" | "best fit") => Ok(()),
        "localeMatcher" => Err(runtime_error("RangeError: invalid localeMatcher")),
        "style" => set_enum(
            &mut formatter.style,
            &text,
            &["long", "short", "narrow"],
            "style",
        ),
        "numeric" => set_enum(
            &mut formatter.numeric,
            &text,
            &["always", "auto"],
            "numeric",
        ),
        "numberingSystem" => set_numbering_system(formatter, text),
        _ => Ok(()),
    }
}

fn set_enum(target: &mut String, text: &str, allowed: &[&str], name: &str) -> Result<(), VmError> {
    let value = valid_enum(text, allowed).ok_or_else(|| match name {
        "style" => runtime_error("RangeError: invalid style"),
        _ => runtime_error("RangeError: invalid numeric"),
    })?;
    *target = value;
    Ok(())
}

fn set_numbering_system(formatter: &mut RelativeOptions, text: String) -> Result<(), VmError> {
    if !valid_numbering_system(&text) {
        return Err(runtime_error("RangeError: invalid numberingSystem"));
    }
    if super::number::supports_digit_system(&text) {
        formatter.numbering_system = text;
    }
    Ok(())
}

fn valid_enum(text: &str, allowed: &[&str]) -> Option<String> {
    if allowed.contains(&text) {
        Some(text.to_string())
    } else {
        None
    }
}

fn valid_numbering_system(text: &str) -> bool {
    !text.is_empty()
        && text.split('-').all(|part| {
            (3..=8).contains(&part.len())
                && part
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric())
        })
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

fn format_relative(
    value: f64,
    unit: &str,
    style: &str,
    numeric: &str,
    locale: &str,
    numbering_system: &str,
) -> Result<String, VmError> {
    let unit = valid_unit(unit)?;
    let parts = phrase_parts(value, &unit, style, numeric, locale, numbering_system);
    Ok(parts.iter().map(|part| part.value.clone()).collect())
}

fn parts_value(
    value: f64,
    unit: &str,
    style: &str,
    numeric: &str,
    locale: &str,
    numbering_system: &str,
) -> Result<Value, VmError> {
    let unit = valid_unit(unit)?;
    let parts = phrase_parts(value, &unit, style, numeric, locale, numbering_system);
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

fn phrase_parts(
    value: f64,
    unit: &str,
    style: &str,
    numeric: &str,
    locale: &str,
    numbering_system: &str,
) -> Vec<Part> {
    if numeric == "auto" {
        if let Some(exception) = auto_exception(value, unit) {
            return vec![Part {
                ty: "literal",
                value: exception,
                unit: false,
            }];
        }
    }
    let mut parts = if locale.starts_with("pl") {
        polish::parts(value, unit, style)
    } else {
        numeric_parts(value, unit, style)
    };
    for part in &mut parts {
        if matches!(part.ty, "integer" | "fraction") {
            part.value = super::number::localize_digits(part.value.clone(), numbering_system);
        }
    }
    parts
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
    let negative = value < 0.0 || (value == 0.0 && value.is_sign_negative());
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
    let word = polish::unit_word(unit, style, magnitude != 1.0);
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

pub(super) fn grouped_number(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_string();
    }
    if value.is_infinite() {
        return "∞".to_string();
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

pub(super) fn number_parts(text: &str) -> Vec<Part> {
    let mut parts = Vec::new();
    let mut digits = String::new();
    let mut fractional = false;
    for character in text.chars() {
        if character.is_ascii_digit() {
            digits.push(character);
        } else {
            flush_digits(&mut parts, &mut digits, fractional);
            match character {
                ',' | '\u{a0}' => parts.push(Part {
                    ty: "group",
                    value: character.to_string(),
                    unit: false,
                }),
                '.' | '|' => {
                    parts.push(Part {
                        ty: "decimal",
                        value: if character == '|' { "," } else { "." }.to_string(),
                        unit: false,
                    });
                    fractional = true;
                }
                '∞' => parts.push(Part {
                    ty: "integer",
                    value: "∞".to_string(),
                    unit: false,
                }),
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

pub(crate) fn prototype_method(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Result<Value, VmError> {
    let slots = receiver_slots(receiver)?;
    let locale = slot_string(&slots, "locale").unwrap_or_else(default_locale);
    let style = slot_string(&slots, "style").unwrap_or_else(|| "long".to_string());
    let numeric = slot_string(&slots, "numeric").unwrap_or_else(|| "always".to_string());
    let numbering_system =
        slot_string(&slots, "numberingSystem").unwrap_or_else(|| "latn".to_string());
    match builtin {
        crate::ops::Builtin::IntlRelativeTimeFormatFormat => {
            let value = super::tolocale::value::to_number_result(arguments.first())?;
            let unit = crate::conversion::to_string(
                arguments.get(1).map_or(&Value::Undefined, |value| value),
            )?;
            if !value.is_finite() {
                return Err(runtime_error("RangeError: value must be finite"));
            }
            Ok(Value::String(format_relative(
                value,
                &unit,
                &style,
                &numeric,
                &locale,
                &numbering_system,
            )?))
        }
        crate::ops::Builtin::IntlRelativeTimeFormatFormatToParts => {
            let value = super::tolocale::value::to_number_result(arguments.first())?;
            if !value.is_finite() {
                return Err(runtime_error("RangeError: value must be finite"));
            }
            let unit = crate::conversion::to_string(
                arguments.get(1).map_or(&Value::Undefined, |value| value),
            )?;
            relative_parts(value, &unit, &style, &numeric, &locale, &numbering_system)
        }
        crate::ops::Builtin::IntlRelativeTimeFormatResolvedOptions => {
            relative_resolved_options(&slots, locale, style, numeric)
        }
        _ => Err(runtime_error("TypeError: method not found")),
    }
}

include!("relative_dispatch.rs");
