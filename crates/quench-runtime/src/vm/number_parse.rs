//! Number text read in place as ASCII bytes, shared by StringToNumber, parseInt and parseFloat.
//! Text with non-ASCII code units is read with each such unit marked as `NON_ASCII_MARK`, a byte
//! no numeric grammar accepts, after its ECMAScript whitespace is trimmed.

use super::primitives::is_ecma_whitespace_character;
use super::wtf16::Units;
use crate::units::DECIMAL_RADIX;
use num_bigint::{BigInt, BigUint};

/// The byte standing for a non-ASCII code unit in text read as bytes.
const NON_ASCII_MARK: u8 = 0x80;
/// The most significant digits a mantissa takes on the Clinger fast path; 19 digits fit a `u64`.
const MAX_MANTISSA_DIGITS: usize = 19;
/// The largest integer a double holds exactly, 2^53.
const EXACT_INTEGER_LIMIT: u64 = 1 << f64::MANTISSA_DIGITS;
/// The largest power of ten a double holds exactly, 10^22.
const MAX_EXACT_POWER: usize = 22;
/// The exact powers of ten from 10^0 through `MAX_EXACT_POWER`, for the Clinger fast path.
const EXACT_POWERS_OF_TEN: [f64; MAX_EXACT_POWER + 1] = [
    1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15, 1e16,
    1e17, 1e18, 1e19, 1e20, 1e21, 1e22,
];
/// The exponent magnitude a literal saturates at; any exponent past it is out of the fast path.
const EXPONENT_LIMIT: i64 = 1 << 20;
/// The text of the Infinity literal, which parseFloat and ToNumber accept after a sign.
pub(super) const INFINITY_LITERAL: &[u8] = b"Infinity";

/// A decimal literal read from the front of some text: its sign, digits and exponent.
pub(super) struct Decimal<'a> {
    /// Whether the literal has a leading `-`.
    pub(super) negative: bool,
    /// The integer digits, possibly none.
    pub(super) integer: &'a [u8],
    /// The fraction digits after the point, possibly none.
    pub(super) fraction: &'a [u8],
    /// The decimal exponent, saturated at `EXPONENT_LIMIT` in magnitude.
    pub(super) exponent: i64,
    /// The literal's bytes after its sign, exponent included.
    pub(super) unsigned: &'a [u8],
    /// The bytes the literal spans, its sign included.
    pub(super) len: usize,
}

/// Whether `byte` is an ASCII WhiteSpace or LineTerminator code unit.
pub(super) fn is_ascii_space(byte: u8) -> bool {
    matches!(byte, b'\t'..=b'\r' | b' ')
}

/// `bytes` without its leading ASCII whitespace.
pub(super) fn trim_leading_space(bytes: &[u8]) -> &[u8] {
    let leading = bytes.iter().take_while(|byte| is_ascii_space(**byte)).count();
    &bytes[leading..]
}

/// `bytes` without its ASCII whitespace at either end.
pub(super) fn trim_ascii_space(bytes: &[u8]) -> &[u8] {
    let text = trim_leading_space(bytes);
    let end = text.iter().rposition(|byte| !is_ascii_space(*byte)).map_or(0, |last| last + 1);
    &text[..end]
}

/// `text` split into whether it has a leading `-` and the bytes after its sign, if any.
pub(super) fn split_sign(text: &[u8]) -> (bool, &[u8]) {
    match text.first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    }
}

/// The count of leading bytes of `bytes` that are ASCII decimal digits.
pub(super) fn digit_run(bytes: &[u8]) -> usize {
    bytes.iter().take_while(|byte| byte.is_ascii_digit()).count()
}

/// The count of leading bytes of `bytes` that are digits in `radix`, `radix` at most 36.
pub(super) fn radix_digit_run(bytes: &[u8], radix: u32) -> usize {
    bytes.iter().take_while(|byte| char::from(**byte).is_digit(radix)).count()
}

/// The exponent of a literal at the front of `bytes`, `e` or `E` with an optional sign and
/// digits, and the bytes it spans; `None` when no digit follows the `e`.
fn scan_exponent(bytes: &[u8]) -> Option<(i64, usize)> {
    if !matches!(bytes.first(), Some(b'e' | b'E')) {
        return None;
    }
    let (negative, unsigned) = split_sign(&bytes[1..]);
    let digits = digit_run(unsigned);
    if digits == 0 {
        return None;
    }
    let magnitude = unsigned[..digits]
        .iter()
        .fold(0_i64, |value, byte| (value * 10 + i64::from(byte - b'0')).min(EXPONENT_LIMIT));
    let value = if negative { -magnitude } else { magnitude };
    Some((value, bytes.len() - unsigned.len() + digits))
}

/// The decimal literal at the front of `bytes`, a StrDecimalLiteral without its Infinity form:
/// an optional sign, integer digits with an optional point and fraction digits, and an optional
/// exponent. `None` when no digit is present.
pub(super) fn scan_decimal(bytes: &[u8]) -> Option<Decimal<'_>> {
    let (negative, body) = split_sign(bytes);
    let integer_len = digit_run(body);
    let integer = &body[..integer_len];
    let mut end = integer_len;
    let mut fraction: &[u8] = &[];
    if body.get(end) == Some(&b'.') {
        let fraction_start = end + 1;
        let fraction_len = digit_run(&body[fraction_start..]);
        if integer_len > 0 || fraction_len > 0 {
            fraction = &body[fraction_start..fraction_start + fraction_len];
            end = fraction_start + fraction_len;
        }
    }
    if integer.is_empty() && fraction.is_empty() {
        return None;
    }
    let (exponent, exponent_len) = scan_exponent(&body[end..]).unwrap_or((0, 0));
    end += exponent_len;
    Some(Decimal {
        negative,
        integer,
        fraction,
        exponent,
        unsigned: &body[..end],
        len: bytes.len() - body.len() + end,
    })
}

/// The value of an integer whose digits are `digits` in `radix`, when it is at most 2^53, and
/// `None` otherwise or for an invalid digit. Accumulated in a `u64`, which the bound keeps exact.
pub(super) fn exact_integer(digits: &[u8], radix: u32) -> Option<u64> {
    digits.iter().try_fold(0_u64, |value, byte| {
        let digit = char::from(*byte).to_digit(radix)?;
        let next = value * u64::from(radix) + u64::from(digit);
        (next <= EXACT_INTEGER_LIMIT).then_some(next)
    })
}

/// The Number nearest the non-negative integer that `digits` (all valid in `radix`) spell.
/// Values up to 2^53 are exact; larger values in radix 10 or a power of two are rounded once
/// from the exact integer, as StringToNumber and parseInt require. Other radices keep the
/// accumulated approximation that parseInt permits (ECMA-262 19.2.5).
pub(super) fn radix_digits_value(digits: &[u8], radix: u32) -> f64 {
    if let Some(exact) = exact_integer(digits, radix) {
        return exact as f64;
    }
    if radix == DECIMAL_RADIX {
        return full_magnitude(digits);
    }
    if radix.is_power_of_two() {
        return power_of_two_digits_value(digits, radix);
    }
    accumulated_digits_value(digits, radix)
}

/// The approximate value of `digits` in `radix`, accumulated as doubles.
fn accumulated_digits_value(digits: &[u8], radix: u32) -> f64 {
    digits
        .iter()
        .filter_map(|byte| char::from(*byte).to_digit(radix))
        .fold(0.0, |value, digit| value * f64::from(radix) + f64::from(digit))
}

/// The correctly rounded value of power-of-two radix `digits`, from their exact big integer.
fn power_of_two_digits_value(digits: &[u8], radix: u32) -> f64 {
    BigUint::parse_bytes(digits, radix).map_or_else(
        || accumulated_digits_value(digits, radix),
        |exact| crate::bigint::to_f64(&BigInt::from(exact)),
    )
}

/// The magnitude of a literal by the Clinger fast path: a mantissa of at most 2^53 scaled by an
/// exact power of ten up to 10^22 needs one correctly rounded operation. `None` otherwise.
pub(super) fn clinger_magnitude(decimal: &Decimal<'_>) -> Option<f64> {
    let mantissa = short_mantissa(decimal.integer, decimal.fraction)?;
    if mantissa == 0 {
        return Some(0.0);
    }
    if mantissa > EXACT_INTEGER_LIMIT {
        return None;
    }
    let exponent = decimal.exponent - decimal.fraction.len() as i64;
    let power = *EXACT_POWERS_OF_TEN.get(usize::try_from(exponent.unsigned_abs()).ok()?)?;
    let value = mantissa as f64;
    Some(if exponent < 0 { value / power } else { value * power })
}

/// The integer the digits of `integer` then `fraction` spell, leading zeros dropped, when they
/// hold at most `MAX_MANTISSA_DIGITS` significant digits; `None` for more.
fn short_mantissa(integer: &[u8], fraction: &[u8]) -> Option<u64> {
    let mut mantissa: u64 = 0;
    let mut significant = 0;
    for byte in integer.iter().chain(fraction) {
        let digit = u64::from(byte - b'0');
        if significant == 0 && digit == 0 {
            continue;
        }
        significant += 1;
        if significant > MAX_MANTISSA_DIGITS {
            return None;
        }
        mantissa = mantissa * 10 + digit;
    }
    Some(mantissa)
}

/// The correctly rounded magnitude of the unsigned literal `unsigned`, by the full conversion.
pub(super) fn full_magnitude(unsigned: &[u8]) -> f64 {
    std::str::from_utf8(unsigned)
        .ok()
        .and_then(|text| text.parse::<f64>().ok())
        .unwrap_or(f64::NAN)
}

/// The signed value of a decimal literal: the exact integer path for a plain integer, then the
/// Clinger fast path, then the full correct conversion.
pub(super) fn decimal_value(decimal: &Decimal<'_>) -> f64 {
    let magnitude = plain_integer_magnitude(decimal)
        .or_else(|| clinger_magnitude(decimal))
        .unwrap_or_else(|| full_magnitude(decimal.unsigned));
    if decimal.negative { -magnitude } else { magnitude }
}

/// The value of a literal with no point and no exponent that is at most 2^53, `None` otherwise.
fn plain_integer_magnitude(decimal: &Decimal<'_>) -> Option<f64> {
    if !decimal.fraction.is_empty() || decimal.exponent != 0 {
        return None;
    }
    exact_integer(decimal.integer, DECIMAL_RADIX).map(|value| value as f64)
}

/// Runs `parse` on the bytes of `units`: the ASCII units are read in place, and wide units are
/// first copied as ASCII bytes with their non-ASCII units marked (`ascii_or_marked`).
pub(super) fn with_ascii_units<R>(units: Units<'_>, parse: impl FnOnce(&[u8]) -> R) -> R {
    match units {
        Units::Ascii(bytes) => parse(bytes),
        Units::Latin1(_) | Units::Wide(_) => parse(&ascii_or_marked(&units.wide())),
    }
}

/// The ASCII bytes of `units` with their ECMAScript whitespace trimmed at both ends, each
/// non-ASCII unit replaced by `NON_ASCII_MARK`.
fn ascii_or_marked(units: &[u16]) -> Vec<u8> {
    let is_space = |unit: u16| char::from_u32(u32::from(unit)).is_some_and(is_ecma_whitespace_character);
    let start = units.iter().position(|unit| !is_space(*unit)).unwrap_or(units.len());
    let end = units.iter().rposition(|unit| !is_space(*unit)).map_or(start, |last| last + 1);
    units[start..end]
        .iter()
        .map(|unit| u8::try_from(*unit).ok().filter(u8::is_ascii).unwrap_or(NON_ASCII_MARK))
        .collect()
}
