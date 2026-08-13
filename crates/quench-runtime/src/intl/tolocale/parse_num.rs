//! Spec `parseInt`/`parseFloat` algorithms plus string-to-number parsing.

use crate::{execute::VmError, value::Value};

pub(crate) fn parse_number(value: &str) -> f64 {
    let value = value.trim();
    if value.is_empty() {
        return 0.0;
    }
    if matches!(value, "INFINITY" | "infinity" | "+infinity" | "-infinity") {
        return f64::NAN;
    }
    let Some((prefix, digits)) = value.get(0..2).map(|prefix| (prefix, &value[2..])) else {
        return value.parse().unwrap_or(f64::NAN);
    };
    let radix = match prefix {
        "0b" | "0B" => Some(2),
        "0o" | "0O" => Some(8),
        "0x" | "0X" => Some(16),
        _ => None,
    };
    if let Some(radix) = radix {
        return i64::from_str_radix(digits, radix).map_or(f64::NAN, |n| n as f64);
    }
    value.parse().unwrap_or(f64::NAN)
}

pub(crate) fn parse_int(arguments: &[Value]) -> Result<f64, VmError> {
    let text = crate::conversion::to_string(arguments.first().unwrap_or(&Value::Undefined))?;
    let text = trim_js_whitespace(&text);
    let radix = match arguments.get(1) {
        Some(value) => to_int32(crate::conversion::to_number(value)?),
        None => 0,
    };
    let (sign, rest) = match text.strip_prefix('-') {
        Some(rest) => (-1.0, rest),
        None => (1.0, text.strip_prefix('+').unwrap_or(text)),
    };
    if radix != 0 && !(2..=36).contains(&radix) {
        return Ok(f64::NAN);
    }
    let (digits, radix) = match radix {
        0 => match hex_digits(rest) {
            Some(digits) => (digits, 16),
            None => (rest, 10),
        },
        16 => (hex_digits(rest).unwrap_or(rest), 16),
        radix => (rest, radix as u32),
    };
    Ok(sign * digit_prefix_value(digits, radix))
}

fn hex_digits(rest: &str) -> Option<&str> {
    rest.strip_prefix("0x").or_else(|| rest.strip_prefix("0X"))
}

fn digit_prefix_value(digits: &str, radix: u32) -> f64 {
    let mut value = 0.0;
    let mut any = false;
    for digit in digits.chars().map_while(|c| c.to_digit(36)) {
        if digit >= radix {
            break;
        }
        value = value * f64::from(radix) + f64::from(digit);
        any = true;
    }
    if any { value } else { f64::NAN }
}

pub(crate) fn parse_float(value: Option<&Value>) -> Result<f64, VmError> {
    let text = crate::conversion::to_string(value.unwrap_or(&Value::Undefined))?;
    let text = trim_js_whitespace(&text);
    let end = decimal_prefix_len(text);
    if end == 0 {
        return Ok(f64::NAN);
    }
    Ok(text[..end].parse().unwrap_or(f64::NAN))
}

fn decimal_prefix_len(text: &str) -> usize {
    let bytes = text.as_bytes();
    let sign = usize::from(matches!(bytes.first(), Some(b'+') | Some(b'-')));
    if text[sign..].starts_with("Infinity") {
        return sign + 8;
    }
    let int_end = scan_digits(bytes, sign);
    let (end, fraction) = scan_fraction(bytes, sign, int_end);
    if int_end - sign + fraction == 0 {
        return 0;
    }
    scan_exponent(bytes, end)
}

fn scan_fraction(bytes: &[u8], sign: usize, int_end: usize) -> (usize, usize) {
    if bytes.get(int_end) != Some(&b'.') {
        return (int_end, 0);
    }
    let frac_end = scan_digits(bytes, int_end + 1);
    if int_end > sign || frac_end > int_end + 1 {
        return (frac_end, frac_end - int_end - 1);
    }
    (int_end, 0)
}

fn scan_exponent(bytes: &[u8], end: usize) -> usize {
    if !matches!(bytes.get(end), Some(b'e') | Some(b'E')) {
        return end;
    }
    let sign = usize::from(matches!(bytes.get(end + 1), Some(b'+') | Some(b'-')));
    let digits_end = scan_digits(bytes, end + 1 + sign);
    if digits_end > end + 1 + sign {
        digits_end
    } else {
        end
    }
}

fn scan_digits(bytes: &[u8], mut index: usize) -> usize {
    while bytes.get(index).is_some_and(u8::is_ascii_digit) {
        index += 1;
    }
    index
}

fn trim_js_whitespace(text: &str) -> &str {
    text.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}')
}

fn to_int32(value: f64) -> i32 {
    if !value.is_finite() {
        return 0;
    }
    (value.trunc().rem_euclid(4_294_967_296.0) as i64 as u32) as i32
}
