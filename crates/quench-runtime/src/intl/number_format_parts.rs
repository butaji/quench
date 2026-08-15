use crate::{intl::make_object, value::Value};

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
            |index| localized_text[..index].trim(),
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
    if (!narrow || locale.starts_with("de")) && !locale.starts_with("ko") {
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
