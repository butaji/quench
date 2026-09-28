const LEADING_SURROGATE_BASE: u32 = 0xd800;
const TRAILING_SURROGATE_BASE: u32 = 0xdc00;
const SURROGATE_OFFSET_MASK: u32 = 0x3ff;
const SURROGATE_OFFSET_BITS: u32 = 10;
const SURROGATE_CODE_POINT_OFFSET: u32 = 0x10000;
const LEADING_SURROGATES: std::ops::RangeInclusive<u16> =
    LEADING_SURROGATE_BASE as u16..=(LEADING_SURROGATE_BASE + SURROGATE_OFFSET_MASK) as u16;
const TRAILING_SURROGATES: std::ops::RangeInclusive<u16> =
    TRAILING_SURROGATE_BASE as u16..=(TRAILING_SURROGATE_BASE + SURROGATE_OFFSET_MASK) as u16;
const SURROGATE_CODE_POINTS: std::ops::RangeInclusive<u32> =
    LEADING_SURROGATE_BASE..=(TRAILING_SURROGATE_BASE + SURROGATE_OFFSET_MASK);
const HEX_DIGITS: &[u8; 16] = b"0123456789ABCDEF";
const HEX_NIBBLE_MASK: u8 = 0x0f;
const ESCAPE_ASCII_MAX: u16 = 0x7f;
const ESCAPE_LATIN1_MAX: u16 = 0xff;
const ESCAPE_WIDE_MARKER: u16 = b'u' as u16;
const ESCAPE_BYTE_HEX_DIGITS: usize = 2;
const ESCAPE_WIDE_HEX_DIGITS: usize = 4;
const URI_RESERVED: &[u8] = b";/?:@&=+$,#";
const URI_UNESCAPED: &[u8] = b"-_.!~*'()";
const ESCAPE_UNESCAPED: &[u8] = b"@*_+-./";

fn hex_digit(unit: u16) -> Option<u8> {
    match unit {
        unit if (b'0' as u16..=b'9' as u16).contains(&unit) => Some((unit - b'0' as u16) as u8),
        unit if (b'a' as u16..=b'f' as u16).contains(&unit) => {
            Some((unit - b'a' as u16 + 10) as u8)
        }
        unit if (b'A' as u16..=b'F' as u16).contains(&unit) => {
            Some((unit - b'A' as u16 + 10) as u8)
        }
        _ => None,
    }
}

fn append_escape_hex(output: &mut String, value: u16, digit_count: usize) {
    for shift in (0..digit_count).rev() {
        let digit = ((value >> (shift * 4)) & u16::from(HEX_NIBBLE_MASK)) as usize;
        output.push(HEX_DIGITS[digit] as char);
    }
}

pub(super) fn escape(value: &super::wtf16::JsString) -> String {
    let mut output = String::with_capacity(value.units().len());
    for &unit in value.units() {
        if unit <= ESCAPE_ASCII_MAX && (unit as u8).is_ascii_alphanumeric() {
            output.push(unit as u8 as char);
        } else if unit <= ESCAPE_ASCII_MAX && ESCAPE_UNESCAPED.contains(&(unit as u8)) {
            output.push(unit as u8 as char);
        } else {
            output.push('%');
            let digit_count = if unit <= ESCAPE_LATIN1_MAX {
                ESCAPE_BYTE_HEX_DIGITS
            } else {
                output.push(ESCAPE_WIDE_MARKER as u8 as char);
                ESCAPE_WIDE_HEX_DIGITS
            };
            append_escape_hex(&mut output, unit, digit_count);
        }
    }
    output
}

pub(super) fn unescape(value: &super::wtf16::JsString) -> super::wtf16::JsString {
    let units = value.units();
    let mut output = Vec::with_capacity(units.len());
    let mut index = 0;
    while index < units.len() {
        if units[index] == b'%' as u16 {
            if let Some(escaped) = unescape_escape(units, index) {
                let (unit, width) = escaped;
                output.push(unit);
                index += width;
                continue;
            }
        }
        output.push(units[index]);
        index += 1;
    }
    super::wtf16::JsString::from_units(&output)
}

fn unescape_escape(units: &[u16], index: usize) -> Option<(u16, usize)> {
    if units.get(index + 1) == Some(&ESCAPE_WIDE_MARKER) {
        let digits = units.get(index + 2..index + 2 + ESCAPE_WIDE_HEX_DIGITS)?;
        let unit = digits.iter().try_fold(0_u16, |value, digit| {
            Some((value << 4) | u16::from(hex_digit(*digit)?))
        })?;
        return Some((unit, 2 + ESCAPE_WIDE_HEX_DIGITS));
    }
    let high = hex_digit(*units.get(index + 1)?)?;
    let low = hex_digit(*units.get(index + 2)?)?;
    Some((u16::from((high << 4) | low), 3))
}

pub(super) fn encode_uri(
    value: &super::wtf16::JsString,
    component: bool,
) -> Result<String, &'static str> {
    let units = value.units();
    let mut output = String::with_capacity(units.len());
    let mut index = 0;
    while index < units.len() {
        let unit = units[index];
        let (code_point, width) = if LEADING_SURROGATES.contains(&unit) {
            let Some(&trailing) = units.get(index + 1) else {
                return Err("malformed URI sequence");
            };
            if !TRAILING_SURROGATES.contains(&trailing) {
                return Err("malformed URI sequence");
            }
            (
                SURROGATE_CODE_POINT_OFFSET
                    + ((u32::from(unit) - LEADING_SURROGATE_BASE) << SURROGATE_OFFSET_BITS)
                    + (u32::from(trailing) - TRAILING_SURROGATE_BASE),
                2,
            )
        } else if TRAILING_SURROGATES.contains(&unit) {
            return Err("malformed URI sequence");
        } else {
            (u32::from(unit), 1)
        };
        let character = char::from_u32(code_point).ok_or("malformed URI sequence")?;
        let mut utf8 = [0; 4];
        for byte in character.encode_utf8(&mut utf8).as_bytes() {
            let unescaped = byte.is_ascii_alphanumeric()
                || URI_UNESCAPED.contains(byte)
                || (!component && URI_RESERVED.contains(byte));
            if unescaped {
                output.push(*byte as char);
            } else {
                output.push('%');
                output.push(HEX_DIGITS[(byte >> 4) as usize] as char);
                output.push(HEX_DIGITS[(byte & HEX_NIBBLE_MASK) as usize] as char);
            }
        }
        index += width;
    }
    Ok(output)
}

pub(super) fn decode_uri(
    value: &super::wtf16::JsString,
    component: bool,
) -> Result<super::wtf16::JsString, &'static str> {
    let units = value.units();
    let mut output = Vec::with_capacity(units.len());
    let mut index = 0;
    while index < units.len() {
        if units[index] != b'%' as u16 {
            output.push(units[index]);
            index += 1;
            continue;
        }
        if index + 2 >= units.len() {
            return Err("malformed URI escape");
        }
        let first = (hex_digit(units[index + 1]).ok_or("malformed URI escape")? << 4)
            | hex_digit(units[index + 2]).ok_or("malformed URI escape")?;
        let (width, mut code_point, minimum) = match first {
            0x00..=0x7f => (1, u32::from(first), 0),
            0xc2..=0xdf => (2, u32::from(first & 0x1f), 0x80),
            0xe0..=0xef => (3, u32::from(first & 0x0f), 0x800),
            0xf0..=0xf4 => (4, u32::from(first & 0x07), 0x10000),
            _ => return Err("malformed URI sequence"),
        };
        if width == 1 && !component && URI_RESERVED.contains(&(first as u8)) {
            output.extend_from_slice(&units[index..index + 3]);
            index += 3;
            continue;
        }
        for continuation in 1..width {
            let escape = index + continuation * 3;
            if units.get(escape) != Some(&(b'%' as u16)) || escape + 2 >= units.len() {
                return Err("malformed URI sequence");
            }
            let byte = (hex_digit(units[escape + 1]).ok_or("malformed URI escape")? << 4)
                | hex_digit(units[escape + 2]).ok_or("malformed URI escape")?;
            if byte & 0xc0 != 0x80 {
                return Err("malformed URI sequence");
            }
            if continuation == 1
                && ((first == 0xe0 && byte < 0xa0)
                    || (first == 0xf0 && byte < 0x90)
                    || (first == 0xf4 && byte > 0x8f))
            {
                return Err("malformed URI sequence");
            }
            code_point = (code_point << 6) | u32::from(byte & 0x3f);
        }
        if code_point < minimum
            || code_point > 0x10ffff
            || SURROGATE_CODE_POINTS.contains(&code_point)
        {
            return Err("malformed URI sequence");
        }
        if code_point <= 0xffff {
            output.push(code_point as u16);
        } else {
            let scalar = code_point - 0x10000;
            output.push(0xd800 | (scalar >> 10) as u16);
            output.push(0xdc00 | (scalar & 0x3ff) as u16);
        }
        index += width * 3;
    }
    Ok(super::wtf16::JsString::from_units(&output))
}

pub(super) fn parse_integer(text: &str, mut radix: i32) -> f64 {
    let mut input = text.trim_start();
    let sign = if let Some(rest) = input.strip_prefix('-') {
        input = rest;
        -1.0
    } else {
        input = input.strip_prefix('+').unwrap_or(input);
        1.0
    };
    if radix == 0 {
        radix = if input.starts_with("0x") || input.starts_with("0X") {
            16
        } else {
            10
        };
    }
    if !(2..=36).contains(&radix) {
        return f64::NAN;
    }
    if radix == 16 {
        input = input
            .strip_prefix("0x")
            .or_else(|| input.strip_prefix("0X"))
            .unwrap_or(input);
    }
    let mut result = 0.0;
    let mut digits = 0;
    for digit in input
        .chars()
        .map_while(|character| character.to_digit(radix as u32))
    {
        result = result * f64::from(radix) + f64::from(digit);
        digits += 1;
    }
    if digits == 0 { f64::NAN } else { sign * result }
}

pub(super) fn number_to_radix(number: f64, radix: u32) -> String {
    if radix == 10 || !number.is_finite() || number.fract() != 0.0 {
        return if number.fract() == 0.0 {
            format!("{number:.0}")
        } else {
            number.to_string()
        };
    }
    if number == 0.0 {
        return "0".into();
    }
    let negative = number < 0.0;
    let mut value = number.abs();
    let alphabet = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut digits = Vec::new();
    while value >= 1.0 {
        let digit = (value % f64::from(radix)) as usize;
        digits.push(alphabet[digit] as char);
        value = (value / f64::from(radix)).floor();
    }
    if negative {
        digits.push('-');
    }
    digits.iter().rev().collect()
}
