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
    let text = if locale.starts_with("de") {
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
    let formatted = if locale.starts_with("de") {
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
