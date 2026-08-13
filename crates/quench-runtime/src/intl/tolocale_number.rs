use super::super::{slot_bool, slot_number, slot_string};
use crate::value::Value;

pub(crate) fn format_resolved(number: f64, slots: &[(String, Value)]) -> String {
    let min_fraction = slot_number(slots, "minimumFractionDigits").unwrap_or(0.0) as usize;
    let max_fraction = slot_number(slots, "maximumFractionDigits")
        .unwrap_or(3.0)
        .max(min_fraction as f64) as usize;
    let use_grouping = slot_bool(slots, "useGrouping").unwrap_or(true);
    let locale = slot_string(slots, "locale").unwrap_or_default();
    let mut text = format_fixed(number, max_fraction);
    if use_grouping {
        text = group(text, &locale);
    }
    pad_minimum(text, min_fraction, &locale)
}

fn format_fixed(number: f64, max_fraction: usize) -> String {
    if number.is_nan() {
        return "NaN".to_string();
    }
    if number.is_infinite() {
        return if number.is_sign_negative() {
            "-Infinity".to_string()
        } else {
            "Infinity".to_string()
        };
    }
    trim_fraction_zeros(format!("{:.*}", max_fraction, number))
}

fn trim_fraction_zeros(mut text: String) -> String {
    if text.contains('.') {
        while text.ends_with('0') {
            text.pop();
        }
        if text.ends_with('.') {
            text.pop();
        }
    }
    text
}

fn group(text: String, locale: &str) -> String {
    let (sign, rest) = split_sign(text);
    let (integer, fraction) = split_fraction(rest);
    let chars: Vec<char> = integer.chars().collect();
    let mut grouped = String::new();
    for (index, character) in chars.iter().enumerate() {
        if index > 0 && (chars.len() - index) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(*character);
    }
    let decimal = if locale.starts_with("de") || locale.starts_with("pt") {
        ','
    } else {
        '.'
    };
    append_fraction(format!("{sign}{grouped}"), fraction, decimal)
}

fn split_sign(text: String) -> (&'static str, String) {
    match text.strip_prefix('-') {
        Some(rest) => ("-", rest.to_string()),
        None => ("", text),
    }
}

fn split_fraction(text: String) -> (String, Option<String>) {
    match text.split_once('.') {
        Some((integer, fraction)) => (integer.to_string(), Some(fraction.to_string())),
        None => (text, None),
    }
}

fn append_fraction(mut text: String, fraction: Option<String>, decimal: char) -> String {
    if let Some(fraction) = fraction {
        text.push(decimal);
        text.push_str(&fraction);
    }
    text
}

fn pad_minimum(text: String, min_fraction: usize, locale: &str) -> String {
    if min_fraction == 0 {
        return text;
    }
    let (sign, rest) = split_sign(text);
    let fraction_digits = rest
        .split_once(['.', ','])
        .map_or(0, |(_, fraction)| fraction.len());
    let mut result = format!("{sign}{rest}");
    if fraction_digits < min_fraction {
        if !rest.contains(['.', ',']) {
            result.push(if locale.starts_with("de") || locale.starts_with("pt") {
                ','
            } else {
                '.'
            });
        }
        result.extend(std::iter::repeat('0').take(min_fraction - fraction_digits));
    }
    result
}
