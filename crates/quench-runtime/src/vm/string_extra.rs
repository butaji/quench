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
pub(super) const RADIX_DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";

/// The most binary integer digits a finite double spells: its integer part is below 2^MAX_EXP,
/// with one more for a quotient that the division rounds up.
const RADIX_INTEGER_DIGITS: usize = f64::MAX_EXP as usize + 1;
/// The most fraction digits a radix in 2..=36 spells: from the point down to the least
/// subnormal, 2^-1074, which is 1074 binary digits, with one more for the rounding of each
/// scaled digit.
const RADIX_FRACTION_DIGITS: usize = (1 - f64::MIN_EXP) as usize + f64::MANTISSA_DIGITS as usize;
/// The index of the point in a radix text's buffer: after the byte for its sign and the
/// integer digits.
const RADIX_POINT: usize = 1 + RADIX_INTEGER_DIGITS;
/// A radix text on the stack: its sign, integer digits, point and fraction digits.
pub(super) const RADIX_TEXT_CAPACITY: usize = RADIX_POINT + 1 + RADIX_FRACTION_DIGITS;

/// `Number::toString(number, radix)` of a finite `number` and a radix in 2..=36, written into
/// `buffer` and answered as its slice: V8's `DoubleToRadixCString`, which Node's output
/// follows. Integer digits are written from the point backwards, fraction digits forwards from
/// it, so the text needs no growing storage. An integer below 2^53 has no fraction and no digit
/// below the double's precision, so it is only its integer digits.
pub(super) fn radix_text(number: f64, radix: u32, buffer: &mut [u8; RADIX_TEXT_CAPACITY]) -> &[u8] {
    let value = number.abs();
    let mut integer = value.floor();
    let fraction = value - integer;
    // Half the distance to the next double, at least the least positive one.
    let delta = (0.5 * (next_double(value) - value)).max(next_double(0.0));
    let (fraction_len, carried) = write_fraction_digits(fraction, delta, radix, &mut buffer[RADIX_POINT + 1..]);
    if carried {
        integer += 1.0;
    }
    let mut start = write_integer_digits(integer, radix, &mut buffer[..RADIX_POINT]);
    if number < 0.0 {
        start -= 1;
        buffer[start] = b'-';
    }
    let end = finish_fraction(buffer, fraction_len);
    &buffer[start..end]
}

/// Writes the fraction digits of `fraction` (in [0, 1)) into `digits`, as their values, from the
/// point down. A digit is written while the fraction still exceeds `delta`, the half gap to the
/// next double, which scales with each digit. Answers the digit count, and whether a round-up
/// carried through every digit into the integer part.
fn write_fraction_digits(mut fraction: f64, mut delta: f64, radix: u32, digits: &mut [u8]) -> (usize, bool) {
    let base = f64::from(radix);
    if fraction < delta {
        return (0, false);
    }
    let mut length = 0;
    loop {
        fraction *= base;
        delta *= base;
        let digit = fraction as usize;
        digits[length] = digit as u8;
        length += 1;
        fraction -= digit as f64;
        let rounds_up = fraction > 0.5 || (fraction == 0.5 && digit & 1 == 1);
        if rounds_up && fraction + delta > 1.0 {
            return round_up_digits(&mut digits[..length], radix);
        }
        if fraction < delta {
            return (length, false);
        }
    }
}

/// Rounds the fraction digit values `digits` up one place: the last digit below the radix's last
/// value is incremented, and each digit at that last value is dropped. Answers the digits kept,
/// and whether every digit was dropped, which carries into the integer part.
fn round_up_digits(digits: &mut [u8], radix: u32) -> (usize, bool) {
    let mut length = digits.len();
    while length > 0 {
        length -= 1;
        if u32::from(digits[length]) + 1 < radix {
            digits[length] += 1;
            return (length + 1, false);
        }
    }
    (0, true)
}

/// Writes the radix digits of the whole number `integer` ending at the end of `digits`, with
/// the zeros that the double's precision leaves at its end, and answers where they start.
fn write_integer_digits(mut integer: f64, radix: u32, digits: &mut [u8]) -> usize {
    let base = f64::from(radix);
    let mut start = digits.len();
    // Digits below the double's precision are zeros.
    while binary_exponent(integer / base) > 0 {
        integer /= base;
        start -= 1;
        digits[start] = b'0';
    }
    loop {
        let remainder = integer % base;
        start -= 1;
        digits[start] = RADIX_DIGITS[remainder as usize];
        integer = (integer - remainder) / base;
        if integer <= 0.0 {
            return start;
        }
    }
}

/// Spells the `count` fraction digit values after the point, and writes the point before them
/// when there are any. Answers the end of the text.
fn finish_fraction(buffer: &mut [u8; RADIX_TEXT_CAPACITY], count: usize) -> usize {
    if count == 0 {
        return RADIX_POINT;
    }
    buffer[RADIX_POINT] = b'.';
    let end = RADIX_POINT + 1 + count;
    for digit in &mut buffer[RADIX_POINT + 1..end] {
        *digit = RADIX_DIGITS[usize::from(*digit)];
    }
    end
}

/// `number_to_radix` as a string value, for a finite `number`: its text is written on the stack
/// and copied once into the string.
pub(super) fn number_to_radix_string(number: f64, radix: u32) -> super::wtf16::JsString {
    with_radix_text(number, radix, super::wtf16::JsString::from_ascii_bytes)
}

/// 2^53: integers of smaller magnitude are exact doubles.
const EXACT_INTEGER_LIMIT: f64 = (1_u64 << f64::MANTISSA_DIGITS) as f64;

/// The most units an exact integer below 2^53 takes in any radix: 53 binary digits and a sign.
const INTEGER_RADIX_CAPACITY: usize = f64::MANTISSA_DIGITS as usize + 1;

/// Hands `use_text` the radix-`radix` text of the finite `number`, written on the stack: an exact
/// integer into a buffer of `INTEGER_RADIX_CAPACITY`, any other number into the full
/// `RADIX_TEXT_CAPACITY` buffer, so the common integer call clears no large buffer.
pub(super) fn with_radix_text<T>(number: f64, radix: u32, use_text: impl FnOnce(&[u8]) -> T) -> T {
    let exact = number.trunc() == number && number.abs() < EXACT_INTEGER_LIMIT;
    if exact {
        let mut buffer = [0u8; INTEGER_RADIX_CAPACITY];
        return use_text(integer_radix_text(number as i64, radix, &mut buffer));
    }
    let mut buffer = [0u8; RADIX_TEXT_CAPACITY];
    use_text(radix_text(number, radix, &mut buffer))
}

/// The radix-`radix` digits of `integer` (|integer| < 2^53), written backwards into `buffer`.
fn integer_radix_text(integer: i64, radix: u32, buffer: &mut [u8; INTEGER_RADIX_CAPACITY]) -> &[u8] {
    let base = u64::from(radix);
    let mut magnitude = integer.unsigned_abs();
    let mut start = buffer.len();
    loop {
        start -= 1;
        buffer[start] = RADIX_DIGITS[(magnitude % base) as usize];
        magnitude /= base;
        if magnitude == 0 {
            break;
        }
    }
    if integer < 0 {
        start -= 1;
        buffer[start] = b'-';
    }
    &buffer[start..]
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
