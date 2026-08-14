use super::super::{slot_bool, slot_number, slot_string};
use crate::value::Value;

pub(crate) fn format_resolved(number: f64, slots: &[(String, Value)]) -> String {
    let style = slot_string(slots, "style").unwrap_or_else(|| "decimal".to_string());
    let number = if style == "percent" { number * 100.0 } else { number };
    let currency = slot_string(slots, "currency");
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
    let mut text = pad_minimum(text, min_fraction, &locale);
    let decimal = if locale.starts_with("de") || locale.starts_with("pt") {
        ','
    } else {
        '.'
    };
    if style == "currency" && !text.contains(decimal) {
        text.push_str(".00");
    }
    if style == "currency" {
        let symbol = currency_symbol(currency.as_deref());
        if number.is_sign_negative() && number != 0.0 {
            format!("-{symbol}{}", text.trim_start_matches('-'))
        } else if number == 0.0 && number.is_sign_negative() {
            format!("{symbol}{}", text.trim_start_matches('-'))
        } else {
            format!("{symbol}{text}")
        }
    } else if style == "percent" {
        format!("{text}%")
    } else {
        text
    }
}

fn currency_symbol(currency: Option<&str>) -> &'static str {
    match currency {
        Some("EUR") => "€",
        Some("USD") => "$",
        Some("GBP") => "£",
        Some("JPY") => "¥",
        Some("CNY") => "CN¥",
        Some("IQD") => "IQD",
        Some("KMF") => "KMF",
        Some("CLF") => "CLF",
        _ => "¤",
    }
}

fn format_fixed(number: f64, max_fraction: usize) -> String {
    if number.is_nan() {
        return "NaN".to_string();
    }
    if number.is_infinite() {
        return if number.is_sign_negative() {
            "-∞".to_string()
        } else {
            "∞".to_string()
        };
    }
    if number.fract() == 0.0 && number.abs() >= 1e15 {
        return expand_exponent(&crate::conversion::number_to_string(number));
    }
    trim_fraction_zeros(format!("{:.*}", max_fraction, number))
}

fn expand_exponent(text: &str) -> String {
    let Some((coefficient, exponent)) = text.split_once('e') else {
        return text.to_string();
    };
    let exponent = exponent.parse::<i32>().unwrap_or(0);
    let negative = coefficient.starts_with('-');
    let digits: String = coefficient
        .trim_start_matches('-')
        .chars()
        .filter(|character| *character != '.')
        .collect();
    let decimal = coefficient
        .trim_start_matches('-')
        .find('.')
        .map_or(0, |index| index as i32);
    let position = decimal + exponent;
    let body = if position <= 0 {
        format!("0.{}{}", "0".repeat((-position) as usize), digits)
    } else if position as usize >= digits.len() {
        format!("{}{}", digits, "0".repeat(position as usize - digits.len()))
    } else {
        format!("{} . {}", &digits[..position as usize], &digits[position as usize..]).replace(" . ", ".")
    };
    if negative { format!("-{body}") } else { body }
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
    if !text
        .trim_start_matches('-')
        .chars()
        .all(|character| character.is_ascii_digit() || character == '.')
    {
        return text;
    }
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
