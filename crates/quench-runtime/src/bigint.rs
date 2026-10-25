use num_bigint::{BigInt, Sign};

pub(crate) const IEEE754_FRACTION_BITS: u32 = f64::MANTISSA_DIGITS - 1;
pub(crate) const IEEE754_EXPONENT_BIAS: i32 = 1023;
pub(crate) const IEEE754_MAX_EXPONENT_BITS: u64 = 0x7ff;
pub(crate) const IEEE754_SUBNORMAL_EXPONENT: i32 = -1074;

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
    let bits = value.abs().to_bits();
    let exponent =
        i32::from(u16::try_from((bits >> IEEE754_FRACTION_BITS) & IEEE754_MAX_EXPONENT_BITS).ok()?)
            - IEEE754_EXPONENT_BIAS;
    let significand =
        (bits & ((1_u64 << IEEE754_FRACTION_BITS) - 1)) | (1_u64 << IEEE754_FRACTION_BITS);
    let mut integer = if exponent >= IEEE754_FRACTION_BITS as i32 {
        BigInt::from(significand)
            << usize::try_from(exponent - IEEE754_FRACTION_BITS as i32).ok()?
    } else {
        BigInt::from(significand >> u32::try_from(IEEE754_FRACTION_BITS as i32 - exponent).ok()?)
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
    ExponentTooLarge,
}

pub(crate) fn shift(value: &BigInt, count: &BigInt, left_shift: bool) -> Result<BigInt, Error> {
    let reverse = count.sign() == Sign::Minus;
    let magnitude = usize::try_from(count.magnitude()).map_err(|_| Error::ExponentTooLarge)?;
    Ok(if left_shift != reverse {
        value << magnitude
    } else {
        value >> magnitude
    })
}

/// The value of a BigInt literal; OXC normalizes its digits to base 10 without separators.
pub(crate) fn decimal_literal(digits: &str) -> BigInt {
    digits
        .parse()
        .expect("OXC normalizes BigInt literal digits to base 10")
}

/// The Number nearest `value`, ties to even; magnitudes past the finite range are infinite.
pub(crate) fn to_f64(value: &BigInt) -> f64 {
    num_traits::ToPrimitive::to_f64(value).unwrap_or(if value.sign() == Sign::Minus {
        f64::NEG_INFINITY
    } else {
        f64::INFINITY
    })
}

/// Bytes the magnitude's digits occupy, for heap accounting.
pub(crate) fn magnitude_bytes(value: &BigInt) -> usize {
    usize::try_from(value.bits().div_ceil(u64::from(u8::BITS))).unwrap_or(usize::MAX)
}
