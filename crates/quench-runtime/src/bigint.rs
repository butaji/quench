mod digits;
#[cfg(test)]
mod tests;
mod value;

pub(crate) use value::{Bitwise, JsBigInt};

use num_bigint::{BigInt, Sign};

pub(crate) fn parse_string(value: &str) -> Option<BigInt> {
    let value = value.trim();
    if value.is_empty() {
        return Some(BigInt::from(0));
    }
    let (radix, digits) = match value.as_bytes() {
        [b'0', b'x' | b'X', digits @ ..] => (16, digits),
        [b'0', b'o' | b'O', digits @ ..] => (8, digits),
        [b'0', b'b' | b'B', digits @ ..] => (2, digits),
        _ => return value.parse().ok(),
    };
    BigInt::parse_bytes(digits, radix)
}

pub(crate) fn number_as_bigint(value: f64) -> Option<BigInt> {
    if !value.is_finite() || value.fract() != 0.0 {
        return None;
    }
    if value == 0.0 {
        return Some(BigInt::from(0));
    }
    // An integral value's significand shifted right drops only zero bits.
    let (significand, exponent) = crate::float_layout::binary64_parts(value);
    let mut integer = if exponent >= 0 {
        BigInt::from(significand) << exponent.unsigned_abs()
    } else {
        BigInt::from(significand >> exponent.unsigned_abs())
    };
    if value.is_sign_negative() {
        integer = -integer;
    }
    Some(integer)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Error {
    DivisionByZero,
    NegativeExponent,
    /// The result would pass `value::MAX_BITS`.
    TooLarge,
}

/// The value of a BigInt literal; OXC normalizes its digits to base 10 without separators.
pub(crate) fn decimal_literal(digits: &str) -> BigInt {
    digits.parse().expect("OXC normalizes BigInt literal digits to base 10")
}

/// The Number nearest `value`, ties to even; magnitudes past the finite range are infinite.
pub(crate) fn to_f64(value: &BigInt) -> f64 {
    num_traits::ToPrimitive::to_f64(value).unwrap_or_else(|| if value.sign() == Sign::Minus {
        f64::NEG_INFINITY
    } else {
        f64::INFINITY
    })
}

