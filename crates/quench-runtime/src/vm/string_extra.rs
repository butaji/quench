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

#[inline(always)]
fn hex_digit_ascii(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
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
    for unit in value.units().iter() {
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
    let wide = value.units().wide();
    let units: &[u16] = &wide;
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

const MALFORMED_URI_MESSAGE: &str = "URI malformed";

pub(super) fn encode_uri(
    value: &super::wtf16::JsString,
    component: bool,
) -> Result<String, &'static str> {
    let wide = value.units().wide();
    let units: &[u16] = &wide;
    let mut output = String::with_capacity(units.len());
    let mut index = 0;
    while index < units.len() {
        let unit = units[index];
        let (code_point, width) = if LEADING_SURROGATES.contains(&unit) {
            let Some(&trailing) = units.get(index + 1) else {
                return Err(MALFORMED_URI_MESSAGE);
            };
            if !TRAILING_SURROGATES.contains(&trailing) {
                return Err(MALFORMED_URI_MESSAGE);
            }
            (
                SURROGATE_CODE_POINT_OFFSET
                    + ((u32::from(unit) - LEADING_SURROGATE_BASE) << SURROGATE_OFFSET_BITS)
                    + (u32::from(trailing) - TRAILING_SURROGATE_BASE),
                2,
            )
        } else if TRAILING_SURROGATES.contains(&unit) {
            return Err(MALFORMED_URI_MESSAGE);
        } else {
            (u32::from(unit), 1)
        };
        let character = char::from_u32(code_point).ok_or(MALFORMED_URI_MESSAGE)?;
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
    if !units.iter().any(|unit| unit == b'%' as u16) {
        return Ok(value.clone());
    }
    if let super::wtf16::Units::Ascii(bytes) = units {
        return decode_uri_ascii(bytes, component);
    }
    let mut output = Vec::with_capacity(units.len());
    let mut index = 0;
    while index < units.len() {
        if units.at(index) != b'%' as u16 {
            output.push(units.at(index));
            index += 1;
            continue;
        }
        if index + 2 >= units.len() {
            return Err(MALFORMED_URI_MESSAGE);
        }
        let first = (hex_digit(units.at(index + 1)).ok_or(MALFORMED_URI_MESSAGE)? << 4)
            | hex_digit(units.at(index + 2)).ok_or(MALFORMED_URI_MESSAGE)?;
        let (width, mut code_point, minimum) = match first {
            0x00..=0x7f => (1, u32::from(first), 0),
            0xc2..=0xdf => (2, u32::from(first & 0x1f), 0x80),
            0xe0..=0xef => (3, u32::from(first & 0x0f), 0x800),
            0xf0..=0xf4 => (4, u32::from(first & 0x07), 0x10000),
            _ => return Err(MALFORMED_URI_MESSAGE),
        };
        if width == 1 && !component && URI_RESERVED.contains(&(first as u8)) {
            units.slice(index..index + 3).extend_into(&mut output);
            index += 3;
            continue;
        }
        for continuation in 1..width {
            let escape = index + continuation * 3;
            if units.get(escape) != Some(b'%' as u16) || escape + 2 >= units.len() {
                return Err(MALFORMED_URI_MESSAGE);
            }
            let byte = (hex_digit(units.at(escape + 1)).ok_or(MALFORMED_URI_MESSAGE)? << 4)
                | hex_digit(units.at(escape + 2)).ok_or(MALFORMED_URI_MESSAGE)?;
            if byte & 0xc0 != 0x80 {
                return Err(MALFORMED_URI_MESSAGE);
            }
            if continuation == 1
                && ((first == 0xe0 && byte < 0xa0)
                    || (first == 0xf0 && byte < 0x90)
                    || (first == 0xf4 && byte > 0x8f))
            {
                return Err(MALFORMED_URI_MESSAGE);
            }
            code_point = (code_point << 6) | u32::from(byte & 0x3f);
        }
        if code_point < minimum
            || code_point > 0x10ffff
            || SURROGATE_CODE_POINTS.contains(&code_point)
        {
            return Err(MALFORMED_URI_MESSAGE);
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

/// ASCII strings are Quench's common representation for escaped URI input. Keep this loop on
/// bytes: the general `Units` accessor has to branch on ASCII vs UTF-16 for every code unit.
fn decode_uri_ascii(
    bytes: &[u8],
    component: bool,
) -> Result<super::wtf16::JsString, &'static str> {
    // Each escaped input byte occupies three ASCII bytes, and decoding never expands the
    // resulting UTF-16 code-unit count past the encoded byte count. This avoids reserving six
    // times the result size for the common percent-encoded astral text while keeping one
    // allocation for ordinary ASCII-heavy strings.
    let mut output = Vec::with_capacity(bytes.len().div_ceil(3));
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            output.push(u16::from(bytes[index]));
            index += 1;
            continue;
        }
        if index + 2 >= bytes.len() {
            return Err(MALFORMED_URI_MESSAGE);
        }
        let first = (hex_digit_ascii(bytes[index + 1]).ok_or(MALFORMED_URI_MESSAGE)? << 4)
            | hex_digit_ascii(bytes[index + 2]).ok_or(MALFORMED_URI_MESSAGE)?;
        let (width, mut code_point, minimum) = match first {
            0x00..=0x7f => (1, u32::from(first), 0),
            0xc2..=0xdf => (2, u32::from(first & 0x1f), 0x80),
            0xe0..=0xef => (3, u32::from(first & 0x0f), 0x800),
            0xf0..=0xf4 => (4, u32::from(first & 0x07), 0x10000),
            _ => return Err(MALFORMED_URI_MESSAGE),
        };
        if width == 1 && !component && URI_RESERVED.contains(&first) {
            output.extend([
                u16::from(b'%'),
                u16::from(bytes[index + 1]),
                u16::from(bytes[index + 2]),
            ]);
            index += 3;
            continue;
        }
        for continuation in 1..width {
            let escape = index + continuation * 3;
            if bytes.get(escape) != Some(&b'%') || escape + 2 >= bytes.len() {
                return Err(MALFORMED_URI_MESSAGE);
            }
            let byte = (hex_digit_ascii(bytes[escape + 1]).ok_or(MALFORMED_URI_MESSAGE)? << 4)
                | hex_digit_ascii(bytes[escape + 2]).ok_or(MALFORMED_URI_MESSAGE)?;
            if byte & 0xc0 != 0x80 {
                return Err(MALFORMED_URI_MESSAGE);
            }
            if continuation == 1
                && ((first == 0xe0 && byte < 0xa0)
                    || (first == 0xf0 && byte < 0x90)
                    || (first == 0xf4 && byte > 0x8f))
            {
                return Err(MALFORMED_URI_MESSAGE);
            }
            code_point = (code_point << 6) | u32::from(byte & 0x3f);
        }
        if code_point < minimum
            || code_point > 0x10ffff
            || SURROGATE_CODE_POINTS.contains(&code_point)
        {
            return Err(MALFORMED_URI_MESSAGE);
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
    let mut input = text.trim_start_matches(super::primitives::is_ecma_whitespace_character);
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
    let digits_end = input
        .find(|character: char| !character.is_digit(radix as u32))
        .unwrap_or(input.len());
    if digits_end == 0 {
        return f64::NAN;
    }
    sign * super::number::integer_digits_value(&input[..digits_end], radix as u32)
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
