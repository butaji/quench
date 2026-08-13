pub(crate) fn group_integer(text: &str) -> String {
    let (sign, rest) = match text.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", text),
    };
    let (integer, fraction) = rest
        .split_once('.')
        .map_or((rest, None), |value| (value.0, Some(value.1)));
    let chars: Vec<char> = integer.chars().collect();
    let mut grouped = String::new();
    for (index, character) in chars.iter().enumerate() {
        if index > 0 && (chars.len() - index) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(*character);
    }
    let mut result = format!("{sign}{grouped}");
    if let Some(fraction) = fraction {
        result.push('.');
        result.push_str(fraction);
    }
    result
}

pub(crate) fn group_integer_locale(text: &str, locale: &str) -> String {
    if locale.starts_with("en-IN") {
        return group_indian(text);
    }
    let grouped = group_integer(text);
    if locale.starts_with("de") {
        grouped.replace(',', ".")
    } else {
        grouped
    }
}

fn group_indian(text: &str) -> String {
    let (sign, rest) = text
        .strip_prefix('-')
        .map_or(("", text), |rest| ("-", rest));
    let (integer, fraction) = rest
        .split_once('.')
        .map_or((rest, None), |value| (value.0, Some(value.1)));
    if integer.len() <= 3 {
        return text.to_string();
    }
    let split = integer.len() - 3;
    let mut chunks = vec![integer[split..].to_string()];
    let prefix = &integer[..split];
    let mut end = prefix.len();
    while end > 2 {
        chunks.push(prefix[end - 2..end].to_string());
        end -= 2;
    }
    chunks.push(prefix[..end].to_string());
    chunks.reverse();
    let mut out = format!("{sign}{}", chunks.join(","));
    if let Some(fraction) = fraction {
        out.push('.');
        out.push_str(fraction);
    }
    out
}

pub(crate) fn apply_minimum_integer(text: &str, minimum: u32) -> String {
    if text
        .trim_start_matches(['-', '+'])
        .chars()
        .any(|c| !c.is_ascii_digit() && c != '.')
    {
        return text.to_string();
    }
    let (sign, rest) = text
        .strip_prefix('-')
        .map_or(("", text), |rest| ("-", rest));
    let (integer, fraction) = rest
        .split_once('.')
        .map_or((rest, None), |value| (value.0, Some(value.1)));
    let integer: String = integer.chars().filter(char::is_ascii_digit).collect();
    let mut result = String::new();
    for _ in integer.len()..minimum as usize {
        result.push('0');
    }
    result.push_str(&integer);
    let mut out = format!("{sign}{result}");
    if let Some(fraction) = fraction {
        out.push('.');
        out.push_str(fraction);
    }
    out
}

pub(crate) fn pad_fraction(text: &str, minimum: u32) -> String {
    let (sign, rest) = text
        .strip_prefix('-')
        .map_or(("", text), |rest| ("-", rest));
    let fraction_digits = rest
        .split_once('.')
        .map_or(0, |(_, fraction)| fraction.len());
    let mut out = format!("{sign}{rest}");
    if minimum > 0 {
        if !rest.contains('.') {
            out.push('.');
        }
        for _ in fraction_digits..minimum as usize {
            out.push('0');
        }
    }
    out
}

pub(crate) fn compact_scale(value: f64, locale: &str, display: &str) -> i32 {
    if !value.is_finite() {
        return 0;
    }
    let magnitude = value.abs().log10().floor() as i32;
    if locale.starts_with("ja") || locale.starts_with("zh") {
        return if magnitude >= 8 {
            8
        } else if magnitude >= 4 {
            4
        } else {
            0
        };
    }
    if locale.starts_with("ko") {
        return if magnitude >= 8 {
            8
        } else if magnitude >= 4 {
            4
        } else if magnitude >= 3 {
            3
        } else {
            0
        };
    }
    if locale.starts_with("de") {
        return if magnitude >= 6 {
            6
        } else if display == "long" && magnitude >= 3 {
            3
        } else {
            0
        };
    }
    if magnitude >= 9 {
        9
    } else if magnitude >= 6 {
        6
    } else if magnitude >= 3 {
        3
    } else {
        0
    }
}

pub(crate) fn compact_fraction_digits(value: f64) -> u32 {
    if value == 0.0 || !value.is_finite() {
        return 0;
    }
    (1 - value.abs().log10().floor() as i32).max(0) as u32
}

pub(crate) fn compact_suffix(magnitude: i32, locale: &str, display: &str) -> &'static str {
    if locale.starts_with("ja") || locale.starts_with("zh") {
        return match magnitude {
            8 => "億",
            4 => {
                if locale.starts_with("zh-TW") {
                    "萬"
                } else {
                    "万"
                }
            }
            _ => "",
        };
    }
    if locale.starts_with("ko") {
        return match magnitude {
            8 => "억",
            4 => "만",
            3 => "천",
            _ => "",
        };
    }
    if locale.starts_with("de") {
        return match (magnitude, display) {
            (6, "long") => " Millionen",
            (3, "long") => " Tausend",
            (6, _) => "\u{a0}Mio.",
            _ => "",
        };
    }
    match magnitude {
        3 if display == "long" => " thousand",
        6 if display == "long" => " million",
        9 if display == "long" => " billion",
        3 => "K",
        6 => "M",
        9 => "B",
        _ => "",
    }
}

pub(crate) fn format_number_rounded(value: f64, max_fraction: u32, increment: u32) -> String {
    if value.is_nan() {
        return "NaN".to_string();
    }
    if value.is_infinite() {
        return if value.is_sign_negative() {
            "-∞"
        } else {
            "∞"
        }
        .to_string();
    }
    let scale = 10_f64.powi(max_fraction as i32);
    let quantum = f64::from(increment.max(1)) / scale;
    let units = value / quantum;
    let adjusted = units + units.signum() * 1e-9;
    let rounded = adjusted.round() * quantum;
    let mut text = format!("{:.*}", max_fraction as usize, rounded);
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

pub(crate) fn scientific_parts(value: f64, engineering: bool) -> (f64, i32) {
    if value == 0.0 || !value.is_finite() {
        return (value, 0);
    }
    let mut exponent = value.abs().log10().floor() as i32;
    if engineering {
        exponent -= exponent.rem_euclid(3);
    }
    (value / 10_f64.powi(exponent), exponent)
}

pub(crate) fn format_currency(
    text: &str,
    currency: Option<&str>,
    display: &str,
    locale: &str,
    currency_sign: &str,
) -> String {
    let (sign, text) = text.strip_prefix('-').map_or_else(
        || {
            text.strip_prefix('+')
                .map_or(("", text), |rest| ("+", rest))
        },
        |rest| ("-", rest),
    );
    let text = if locale.starts_with("de") || locale.starts_with("pt") {
        text.replace('.', ",")
    } else {
        text.to_string()
    };
    let symbol = match display {
        "code" | "name" => currency.unwrap_or("USD"),
        _ => match currency {
            Some("USD") => "$",
            Some("EUR") => "€",
            Some("JPY") => "¥",
            Some("GBP") => "£",
            Some("CNY") => "¥",
            Some("INR") => "₹",
            Some("RUB") => "₽",
            Some("KRW") => "₩",
            _ => currency.unwrap_or("USD"),
        },
    };
    let symbol =
        if (locale.starts_with("ko") || locale.starts_with("zh")) && currency == Some("USD") {
            "US$"
        } else {
            symbol
        };
    let formatted = if locale.starts_with("de") || locale.starts_with("pt") {
        format!("{text}\u{a0}{symbol}")
    } else {
        format!("{symbol}{text}")
    };
    if sign == "-" && currency_sign == "accounting" && !locale.starts_with("de") {
        format!("({formatted})")
    } else {
        format!("{sign}{formatted}")
    }
}

pub(crate) fn format_unit(text: &str, unit: Option<&str>, display: &str) -> String {
    let suffix = match (unit, display) {
        (Some("percent"), _) => "%",
        (Some("meter"), "long") => "meters",
        (Some("meter"), _) => "m",
        (Some("kilometer"), "long") => "kilometers",
        (Some("kilometer"), _) => "km",
        (Some("kilometer-per-hour"), "long") => "kilometers per hour",
        (Some("kilometer-per-hour"), "narrow") => "km/h",
        (Some("kilometer-per-hour"), _) => "km/h",
        _ => "",
    };
    if suffix.is_empty() {
        text.to_string()
    } else if display == "narrow" || unit == Some("percent") {
        format!("{text}{suffix}")
    } else {
        format!("{text} {suffix}")
    }
}
use crate::{intl::make_object, value::Value};

fn part(kind: &str, value: &str) -> Value {
    make_object(vec![
        ("type".to_string(), Value::String(kind.to_string())),
        ("value".to_string(), Value::String(value.to_string())),
    ])
}

pub(crate) fn percent_part() -> Value {
    part("percentSign", "%")
}

pub(crate) fn numeric_parts(text: &str, locale: &str) -> Vec<Value> {
    let (sign, body) = match text.strip_prefix('-') {
        Some(body) => (Some(("minusSign", "-")), body),
        None => match text.strip_prefix('+') {
            Some(body) => (Some(("plusSign", "+")), body),
            None => (None, text),
        },
    };
    let mut parts = sign.map_or_else(Vec::new, |(kind, value)| vec![part(kind, value)]);
    if body == "∞" || !body.chars().any(|character| character.is_ascii_digit()) {
        parts.push(part(if body == "∞" { "infinity" } else { "nan" }, body));
        return parts;
    }
    let (mantissa, exponent) = body
        .split_once('E')
        .map_or((body, None), |value| (value.0, Some(value.1)));
    parts.extend(decimal_numeric_parts(mantissa, locale));
    if let Some(exponent) = exponent {
        parts.push(part("exponentSeparator", "E"));
        let (kind, digits) = exponent.strip_prefix('-').map_or_else(
            || ("exponentInteger", exponent),
            |digits| ("exponentMinusSign", digits),
        );
        parts.push(part(
            kind,
            if kind == "exponentMinusSign" {
                "-"
            } else {
                digits
            },
        ));
        if kind == "exponentMinusSign" {
            parts.push(part("exponentInteger", digits));
        }
    }
    parts
}

fn decimal_numeric_parts(text: &str, locale: &str) -> Vec<Value> {
    let decimal = if locale.starts_with("de") { ',' } else { '.' };
    let grouping = if decimal == ',' { '.' } else { ',' };
    let split = text.find(|character: char| {
        !character.is_ascii_digit() && character != decimal && character != grouping
    });
    let (numeric, suffix) =
        split.map_or((text, None), |index| (&text[..index], Some(&text[index..])));
    let (integer, fraction) = numeric
        .split_once(decimal)
        .map_or((numeric, None), |value| (value.0, Some(value.1)));
    let mut parts = grouped_integer_parts(integer, grouping);
    if let Some(fraction) = fraction {
        parts.push(part("decimal", &decimal.to_string()));
        parts.push(part("fraction", fraction));
    }
    if let Some(suffix) = suffix {
        let trimmed = suffix.trim_start();
        if trimmed.len() != suffix.len() {
            parts.push(part("literal", &suffix[..suffix.len() - trimmed.len()]));
        }
        parts.push(part("compact", trimmed));
    }
    parts
}

fn grouped_integer_parts(integer: &str, grouping: char) -> Vec<Value> {
    let mut parts = Vec::new();
    let mut digits = String::new();
    for character in integer.chars() {
        if character == grouping {
            if !digits.is_empty() {
                parts.push(part("integer", &digits));
                digits.clear();
            }
            parts.push(part("group", &grouping.to_string()));
        } else {
            digits.push(character);
        }
    }
    if !digits.is_empty() || parts.is_empty() {
        parts.push(part("integer", &digits));
    }
    parts
}

pub(crate) fn currency_parts(
    text: &str,
    currency: Option<&str>,
    display: &str,
    locale: &str,
) -> Vec<Value> {
    let symbol = currency_symbol(currency, display, locale);
    let accounting = text.starts_with('(');
    let body = text.trim_matches(['(', ')']);
    let stripped = body.replace(symbol.as_str(), "");
    let (prefix, number) = if locale.starts_with("de") {
        let marker = format!("\u{a0}{symbol}");
        ("", body.strip_suffix(&marker).unwrap_or(body))
    } else {
        (symbol.as_str(), stripped.as_str())
    };
    let mut parts = Vec::new();
    if accounting {
        parts.push(part("literal", "("));
    }
    if number.starts_with('-') {
        parts.push(part("minusSign", "-"));
    } else if number.starts_with('+') {
        parts.push(part("plusSign", "+"));
    }
    if !prefix.is_empty() {
        parts.push(part("currency", prefix));
    }
    let number = number.trim_start_matches(['-', '+']);
    parts.extend(numeric_parts(number, locale));
    if locale.starts_with("de") {
        parts.push(part("literal", "\u{a0}"));
        parts.push(part("currency", &symbol));
    }
    if accounting {
        parts.push(part("literal", ")"));
    }
    parts
}

fn currency_symbol(currency: Option<&str>, display: &str, locale: &str) -> String {
    let currency = currency.unwrap_or("USD");
    if display == "code" || display == "name" {
        return currency.to_string();
    }
    if (locale.starts_with("ko") || locale.starts_with("zh")) && currency == "USD" {
        return "US$".to_string();
    }
    match currency {
        "USD" => "$",
        "EUR" => "€",
        "JPY" => "¥",
        "GBP" => "£",
        "CNY" => "¥",
        "INR" => "₹",
        "RUB" => "₽",
        "KRW" => "₩",
        other => other,
    }
    .to_string()
}

pub(crate) fn unit_parts(
    text: &str,
    unit: Option<&str>,
    display: &str,
    locale: &str,
) -> Vec<Value> {
    let suffix = unit_suffix(unit, display, locale);
    let narrow = display == "narrow" || unit == Some("percent");
    let localized_text = if locale.starts_with("de") {
        text.replace('.', ",")
    } else {
        text.to_string()
    };
    let number = localized_text
        .strip_suffix('%')
        .unwrap_or(&localized_text)
        .find(|character: char| character.is_ascii_alphabetic())
        .map_or(
            localized_text.strip_suffix('%').unwrap_or(&localized_text),
            |index| localized_text[..index].trim_end(),
        );
    let mut parts = numeric_parts(number, locale);
    if locale.starts_with("ko") && display == "long" {
        parts.insert(0, part("unit", "시속"));
        parts.insert(1, part("literal", " "));
    } else if locale.starts_with("ja") && display == "long" {
        parts.insert(0, part("unit", "時速"));
        parts.insert(1, part("literal", " "));
    } else if locale.starts_with("zh-TW") && display == "long" {
        parts.insert(0, part("unit", "每小時"));
        parts.insert(1, part("literal", " "));
    }
    if !narrow && !(locale.starts_with("ko") && display != "long") {
        parts.push(part("literal", " "));
    }
    parts.push(part("unit", &suffix));
    parts
}

fn unit_suffix(unit: Option<&str>, display: &str, locale: &str) -> String {
    match (unit, display) {
        (Some("percent"), _) => "%".to_string(),
        (Some("meter"), "long") => "meters".to_string(),
        (Some("meter"), _) => "m".to_string(),
        (Some("kilometer"), "long") => "kilometers".to_string(),
        (Some("kilometer"), _) => "km".to_string(),
        (Some("kilometer-per-hour"), "long") if locale.starts_with("de") => {
            "Kilometer pro Stunde".to_string()
        }
        (Some("kilometer-per-hour"), "long") if locale.starts_with("ja") => {
            "キロメートル".to_string()
        }
        (Some("kilometer-per-hour"), "long") if locale.starts_with("ko") => "킬로미터".to_string(),
        (Some("kilometer-per-hour"), "long") if locale.starts_with("zh-TW") => "公里".to_string(),
        (Some("kilometer-per-hour"), "long") => "kilometers per hour".to_string(),
        (Some("kilometer-per-hour"), _) if locale.starts_with("zh-TW") => "公里/小時".to_string(),
        (Some("kilometer-per-hour"), _) => "km/h".to_string(),
        _ => String::new(),
    }
}
