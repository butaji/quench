mod number_format_parts;

pub(crate) use number_format_parts::{
    currency_parts, format_unit, numeric_parts, percent_part, unit_parts,
};

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
    let (sign, body) = text
        .strip_prefix(['-', '+'])
        .map_or(("", text), |rest| (&text[..1], rest));
    let (integer, fraction) = body
        .split_once('.')
        .map_or((body, None), |value| (value.0, Some(value.1)));
    let grouped = group_integer(integer);
    let (grouping, decimal) = if locale.starts_with("de") {
        ('.', ',')
    } else if locale.starts_with("pt") {
        ('\u{a0}', ',')
    } else {
        (',', '.')
    };
    let grouped = grouped.replace(',', &grouping.to_string());
    fraction.map_or_else(
        || format!("{sign}{grouped}"),
        |fraction| format!("{sign}{grouped}{decimal}{fraction}"),
    )
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
    if locale.starts_with("en-IN") {
        return indian_compact_scale(magnitude);
    }
    if locale.starts_with("ja") || locale.starts_with("zh") {
        return east_asian_compact_scale(magnitude);
    }
    if locale.starts_with("ko") {
        return korean_compact_scale(magnitude);
    }
    if locale.starts_with("de") {
        return german_compact_scale(magnitude, display);
    }
    western_compact_scale(magnitude)
}

fn indian_compact_scale(magnitude: i32) -> i32 {
    if magnitude >= 5 {
        5
    } else if magnitude >= 3 {
        3
    } else {
        0
    }
}

fn east_asian_compact_scale(magnitude: i32) -> i32 {
    if magnitude >= 8 {
        8
    } else if magnitude >= 4 {
        4
    } else {
        0
    }
}

fn korean_compact_scale(magnitude: i32) -> i32 {
    if magnitude >= 8 {
        8
    } else if magnitude >= 4 {
        4
    } else if magnitude >= 3 {
        3
    } else {
        0
    }
}

fn german_compact_scale(magnitude: i32, display: &str) -> i32 {
    if magnitude >= 6 {
        6
    } else if display == "long" && magnitude >= 3 {
        3
    } else {
        0
    }
}

fn western_compact_scale(magnitude: i32) -> i32 {
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
        return east_asian_compact_suffix(magnitude, locale);
    }
    if locale.starts_with("ko") {
        return korean_compact_suffix(magnitude);
    }
    if locale.starts_with("de") {
        return german_compact_suffix(magnitude, display);
    }
    if locale.starts_with("en-IN") && magnitude == 5 {
        return "L";
    }
    western_compact_suffix(magnitude, display)
}

fn east_asian_compact_suffix(magnitude: i32, locale: &str) -> &'static str {
    match magnitude {
        8 => "億",
        4 if locale.starts_with("zh-TW") => "萬",
        4 => "万",
        _ => "",
    }
}

fn korean_compact_suffix(magnitude: i32) -> &'static str {
    match magnitude {
        8 => "억",
        4 => "만",
        3 => "천",
        _ => "",
    }
}

fn german_compact_suffix(magnitude: i32, display: &str) -> &'static str {
    match (magnitude, display) {
        (6, "long") => " Millionen",
        (3, "long") => " Tausend",
        (6, _) => "\u{a0}Mio.",
        _ => "",
    }
}

fn western_compact_suffix(magnitude: i32, display: &str) -> &'static str {
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

pub(crate) fn format_number_rounded(
    value: f64,
    max_fraction: u32,
    increment: u32,
    mode: &str,
) -> String {
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
    if value.abs() >= 1e21 && increment == 1 {
        return value.to_string();
    }
    let scale = 10_f64.powi(max_fraction as i32);
    let quantum = f64::from(increment.max(1)) / scale;
    let units = value / quantum;
    let adjusted = units + units.signum() * 1e-9;
    let rounded = round_units(adjusted, mode) * quantum;
    let mut text = format!("{:.*}", max_fraction as usize, rounded);
    if text.contains('.') {
        while text.ends_with('0') {
            text.pop();
        }
        if text.ends_with('.') {
            text.pop();
        }
    }
    if rounded == 0.0 && value.is_sign_negative() {
        text.insert(0, '-');
    }
    text
}

pub(crate) fn format_significant(value: f64, minimum: u32, maximum: u32, mode: &str) -> String {
    if value == 0.0 {
        return format_zero_significant(value, minimum);
    }
    if let Some(text) = format_large_significant(value, minimum, maximum, mode) {
        return text;
    }
    let exponent = value.abs().log10().floor() as i32;
    let decimal_places = maximum as i32 - exponent - 1;
    let scale = 10f64.powi(decimal_places);
    let rounded = if decimal_places >= 0 {
        round_units(value * scale, mode) / scale
    } else {
        let quantum = 10f64.powi(-decimal_places);
        round_units(value / quantum, mode) * quantum
    };
    let decimals = decimal_places.max(0) as usize;
    let text = format!("{:.*}", decimals, rounded);
    finish_significant(text, minimum)
}

fn format_zero_significant(value: f64, minimum: u32) -> String {
    let zero = if minimum > 1 {
        format!("{:.*}", (minimum - 1) as usize, 0.0)
    } else {
        "0".to_string()
    };
    if value.is_sign_negative() {
        format!("-{zero}")
    } else {
        zero
    }
}

fn finish_significant(mut text: String, minimum: u32) -> String {
    if let Some((whole, fraction)) = text.split_once('.') {
        let mut fraction = fraction.trim_end_matches('0').to_string();
        let required = minimum.saturating_sub(whole.trim_start_matches('-').len() as u32);
        while fraction.len() < required as usize {
            fraction.push('0');
        }
        text = if fraction.is_empty() {
            whole.to_string()
        } else {
            format!("{whole}.{fraction}")
        };
    }
    text
}

fn format_large_significant(value: f64, minimum: u32, maximum: u32, mode: &str) -> Option<String> {
    if !value.is_finite() || value.abs() < 1e21 {
        return None;
    }
    let (sign, digits) = value
        .to_string()
        .strip_prefix('-')
        .map_or(("", value.to_string()), |text| ("-", text.to_string()));
    let mut digits: Vec<char> = digits.chars().filter(|c| c.is_ascii_digit()).collect();
    let keep = maximum as usize;
    if digits.len() > keep {
        let round_up = should_round_up(digits[keep], mode, value.is_sign_negative());
        digits.truncate(keep);
        if round_up {
            increment_digits(&mut digits);
        }
    }
    let exponent = value
        .abs()
        .to_string()
        .chars()
        .filter(|c| c.is_ascii_digit())
        .count();
    let total = exponent.max(digits.len());
    while digits.len() < minimum as usize {
        digits.push('0');
    }
    while digits.len() < total {
        digits.push('0');
    }
    Some(format!("{sign}{}", digits.into_iter().collect::<String>()))
}

fn should_round_up(next: char, mode: &str, negative: bool) -> bool {
    match mode {
        "ceil" => !negative,
        "floor" => negative,
        "trunc" => false,
        "expand" => true,
        _ => next >= '5',
    }
}

fn increment_digits(digits: &mut [char]) {
    for digit in digits.iter_mut().rev() {
        if *digit != '9' {
            *digit = ((*digit as u8) + 1) as char;
            return;
        }
        *digit = '0';
    }
}

fn round_units(value: f64, mode: &str) -> f64 {
    match mode {
        "ceil" => value.ceil(),
        "floor" => value.floor(),
        "trunc" => value.trunc(),
        "expand" => {
            if value.is_sign_negative() {
                value.floor()
            } else {
                value.ceil()
            }
        }
        "halfCeil" | "halfFloor" | "halfTrunc" | "halfEven" | "halfExpand" => {
            let lower = value.floor();
            let fraction = value - lower;
            if fraction < 0.5 - 1e-9 {
                lower
            } else if fraction > 0.5 + 1e-9 {
                lower + 1.0
            } else {
                match mode {
                    "halfCeil" => lower + 1.0,
                    "halfFloor" => lower,
                    "halfExpand" if value.is_sign_negative() => lower,
                    "halfTrunc" if value.is_sign_negative() => lower + 1.0,
                    "halfTrunc" => lower,
                    "halfEven" if (lower as i64) % 2 == 0 => lower,
                    _ => lower + 1.0,
                }
            }
        }
        _ => value.round(),
    }
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
    let symbol = currency_symbol(currency, display, locale);
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

fn currency_symbol<'a>(currency: Option<&'a str>, display: &str, locale: &str) -> &'a str {
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
    if (locale.starts_with("ko") || locale.starts_with("zh")) && currency == Some("USD") {
        "US$"
    } else {
        symbol
    }
}
