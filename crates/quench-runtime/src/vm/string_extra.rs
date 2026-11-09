const LEADING_SURROGATE_BASE: u32 = 0xd800;
const TRAILING_SURROGATE_BASE: u32 = 0xdc00;
const SURROGATE_OFFSET_MASK: u32 = 0x3ff;
const SURROGATE_OFFSET_BITS: u32 = 10;
const SURROGATE_CODE_POINT_OFFSET: u32 = 0x10000;
const LEADING_SURROGATES: std::ops::RangeInclusive<u16> =
    LEADING_SURROGATE_BASE as u16..=(LEADING_SURROGATE_BASE + SURROGATE_OFFSET_MASK) as u16;
const TRAILING_SURROGATES: std::ops::RangeInclusive<u16> =
    TRAILING_SURROGATE_BASE as u16..=(TRAILING_SURROGATE_BASE + SURROGATE_OFFSET_MASK) as u16;
const HEX_DIGITS: &[u8; 16] = b"0123456789ABCDEF";
const HEX_NIBBLE_MASK: u8 = 0x0f;
const ESCAPE_ASCII_MAX: u16 = 0x7f;
const ESCAPE_LATIN1_MAX: u16 = 0xff;
const ESCAPE_WIDE_MARKER: u16 = b'u' as u16;
const ESCAPE_BYTE_HEX_DIGITS: usize = 2;
const ESCAPE_WIDE_HEX_DIGITS: usize = 4;
use crate::units::HEXADECIMAL_RADIX;
/// Bits one hex digit spells.
const HEX_DIGIT_BITS: u32 = HEXADECIMAL_RADIX.ilog2();
/// A URI escape is `%` and two hex digits.
const URI_ESCAPE_LEN: usize = 1 + ESCAPE_BYTE_HEX_DIGITS;
/// The longest UTF-8 sequence.
const UTF8_MAX_WIDTH: usize = 4;
const URI_RESERVED: &[u8] = b";/?:@&=+$,#";
const URI_UNESCAPED: &[u8] = b"-_.!~*'()";
const ESCAPE_UNESCAPED: &[u8] = b"@*_+-./";

fn hex_digit(unit: u16) -> Option<u8> {
    let digit = char::from_u32(u32::from(unit))?.to_digit(HEXADECIMAL_RADIX)?;
    u8::try_from(digit).ok()
}

fn append_escape_hex(output: &mut String, value: u16, digit_count: usize) {
    for shift in (0..digit_count).rev() {
        let digit = ((value >> (shift as u32 * HEX_DIGIT_BITS)) & u16::from(HEX_NIBBLE_MASK)) as usize;
        output.push(HEX_DIGITS[digit] as char);
    }
}

pub(super) fn escape(value: &super::wtf16::JsString) -> String {
    let mut output = String::with_capacity(value.units().len());
    for unit in value.units().iter() {
        if unit <= ESCAPE_ASCII_MAX
            && ((unit as u8).is_ascii_alphanumeric() || ESCAPE_UNESCAPED.contains(&(unit as u8)))
        {
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
        if units[index] == b'%' as u16
            && let Some(escaped) = unescape_escape(units, index) {
                let (unit, width) = escaped;
                output.push(unit);
                index += width;
                continue;
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
            Some((value << HEX_DIGIT_BITS) | u16::from(hex_digit(*digit)?))
        })?;
        return Some((unit, 2 + ESCAPE_WIDE_HEX_DIGITS));
    }
    let high = hex_digit(*units.get(index + 1)?)?;
    let low = hex_digit(*units.get(index + 2)?)?;
    Some((u16::from((high << HEX_DIGIT_BITS) | low), URI_ESCAPE_LEN))
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
    if !units.iter().any(|unit| unit == u16::from(b'%')) {
        return Ok(value.clone());
    }
    // ASCII strings are Quench's common representation for escaped URI input; keep that loop
    // on bytes, since the general `Units` accessor branches on ASCII vs UTF-16 per code unit.
    // Decoding never expands past one UTF-16 unit per escaped input byte, so the ASCII bound
    // keeps one allocation without reserving six times the result.
    let output = match units {
        super::wtf16::Units::Ascii(bytes) => {
            let mut output = Vec::with_capacity(bytes.len().div_ceil(URI_ESCAPE_LEN));
            decode_uri_units(bytes.len(), |index| u16::from(bytes[index]), component, &mut output)?;
            output
        }
        _ => {
            let mut output = Vec::with_capacity(units.len());
            decode_uri_units(units.len(), |index| units.at(index), component, &mut output)?;
            output
        }
    };
    Ok(super::wtf16::JsString::from_units(&output))
}

/// ECMAScript's `Decode` over `len` code units read through `at`: each `%XX` escape run that
/// spells one UTF-8 sequence becomes its UTF-16 units, and an escaped reserved character
/// stays escaped unless decoding a URI component.
#[inline(always)]
fn decode_uri_units(
    len: usize,
    at: impl Fn(usize) -> u16,
    component: bool,
    output: &mut Vec<u16>,
) -> Result<(), &'static str> {
    let mut index = 0;
    while index < len {
        if at(index) == u16::from(b'%') {
            index = decode_uri_escape(len, &at, component, index, output)?;
        } else {
            output.push(at(index));
            index += 1;
        }
    }
    Ok(())
}

/// Decodes the escape run that starts at `index` into `output`; answers where the run ends.
#[inline(always)]
fn decode_uri_escape(
    len: usize,
    at: &impl Fn(usize) -> u16,
    component: bool,
    index: usize,
    output: &mut Vec<u16>,
) -> Result<usize, &'static str> {
    let escaped_byte = |index: usize| -> Option<u8> {
        if index + URI_ESCAPE_LEN > len || at(index) != u16::from(b'%') {
            return None;
        }
        Some(hex_digit(at(index + 1))? << HEX_DIGIT_BITS | hex_digit(at(index + 2))?)
    };
    let lead = escaped_byte(index).ok_or(MALFORMED_URI_MESSAGE)?;
    // A UTF-8 lead byte's leading ones count the sequence's bytes; an ASCII byte has none.
    let width = match lead.leading_ones() as usize {
        0 => 1,
        width @ 2..=UTF8_MAX_WIDTH => width,
        _ => return Err(MALFORMED_URI_MESSAGE),
    };
    let escapes = index..index + width * URI_ESCAPE_LEN;
    let end = escapes.end;
    if width == 1 && !component && URI_RESERVED.contains(&lead) {
        output.extend(escapes.map(at));
        return Ok(end);
    }
    let mut sequence = [0; UTF8_MAX_WIDTH];
    for (byte, escape) in sequence[..width].iter_mut().zip(escapes.step_by(URI_ESCAPE_LEN)) {
        *byte = escaped_byte(escape).ok_or(MALFORMED_URI_MESSAGE)?;
    }
    // UTF-8 validity rejects overlong forms, surrogates and code points past U+10FFFF,
    // exactly the sequences `Decode` rejects.
    let decoded = std::str::from_utf8(&sequence[..width]).map_err(|_| MALFORMED_URI_MESSAGE)?;
    output.extend(decoded.encode_utf16());
    Ok(end)
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

/// The digits `Number.prototype.toString(radix)` spells, after their sign.
const RADIX_DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";

/// `Number::toString(number, radix)` for a finite `number` and a radix in 2..=36 other than
/// 10, which the caller spells in decimal: V8's `DoubleToRadixCString`, which Node's output
/// follows. Fraction digits stop where they no longer distinguish the double from its
/// neighbours (half the gap to the next double, scaled with each digit), the last rounds to
/// even and may carry into the integer part, and integer digits below the double's precision
/// are zeros.
pub(super) fn number_to_radix(number: f64, radix: u32) -> String {
    let base = f64::from(radix);
    let negative = number < 0.0;
    let value = number.abs();
    let mut integer = value.floor();
    let mut fraction = value - integer;
    // Half the distance to the next double, at least the least positive one.
    let mut delta = (0.5 * (next_double(value) - value)).max(next_double(0.0));
    let mut fraction_digits = Vec::new();
    if fraction >= delta {
        loop {
            fraction *= base;
            delta *= base;
            let digit = fraction as usize;
            fraction_digits.push(digit);
            fraction -= digit as f64;
            let rounds_up = fraction > 0.5 || (fraction == 0.5 && digit & 1 == 1);
            if rounds_up && fraction + delta > 1.0 {
                // Round up, carrying through every written digit at the radix's last value.
                loop {
                    match fraction_digits.pop() {
                        None => {
                            integer += 1.0;
                            break;
                        }
                        Some(last) if last + 1 < radix as usize => {
                            fraction_digits.push(last + 1);
                            break;
                        }
                        Some(_) => {}
                    }
                }
                break;
            }
            if fraction < delta {
                break;
            }
        }
    }
    let mut integer_digits = Vec::new();
    // Digits below the double's precision are zeros.
    while binary_exponent(integer / base) > 0 {
        integer /= base;
        integer_digits.push(b'0');
    }
    loop {
        let remainder = integer % base;
        integer_digits.push(RADIX_DIGITS[remainder as usize]);
        integer = (integer - remainder) / base;
        if integer <= 0.0 {
            break;
        }
    }
    if negative {
        integer_digits.push(b'-');
    }
    integer_digits.reverse();
    let mut text = String::from_utf8(integer_digits).expect("radix digits are ASCII");
    if !fraction_digits.is_empty() {
        text.push('.');
        text.extend(fraction_digits.into_iter().map(|digit| char::from(RADIX_DIGITS[digit])));
    }
    text
}

/// The least double greater than the finite, non-negative `value`.
fn next_double(value: f64) -> f64 {
    f64::from_bits(value.to_bits() + 1)
}

/// The power of two that scales a double's integer significand to its value (V8's
/// `Double::Exponent`): the subnormal exponent for a subnormal or zero.
fn binary_exponent(value: f64) -> i32 {
    const SIGNIFICAND_BITS: u32 = f64::MANTISSA_DIGITS - 1;
    const EXPONENT_MASK: u64 = (1 << (64 - 1 - SIGNIFICAND_BITS)) - 1;
    const BIAS: i32 = f64::MAX_EXP - 1 + SIGNIFICAND_BITS as i32;
    let biased = ((value.to_bits() >> SIGNIFICAND_BITS) & EXPONENT_MASK) as i32;
    if biased == 0 { 1 - BIAS } else { biased - BIAS }
}
