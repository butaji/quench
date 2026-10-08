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
    InvalidDecimal,
}

pub(crate) fn binary(
    left: &str,
    right: &str,
    operation: impl FnOnce(BigInt, BigInt) -> Result<BigInt, Error>,
) -> Result<String, Error> {
    let left = left.parse::<BigInt>().map_err(|_| Error::InvalidDecimal)?;
    let right = right.parse::<BigInt>().map_err(|_| Error::InvalidDecimal)?;
    operation(left, right).map(|value| value.to_str_radix(10))
}

pub(crate) fn shift(left: &str, right: &str, left_shift: bool) -> Result<String, Error> {
    let value = left.parse::<BigInt>().map_err(|_| Error::InvalidDecimal)?;
    let count = right.parse::<BigInt>().map_err(|_| Error::InvalidDecimal)?;
    let reverse = count.sign() == Sign::Minus;
    let magnitude = (-&count)
        .max(count.clone())
        .to_str_radix(10)
        .parse::<usize>()
        .map_err(|_| Error::ExponentTooLarge)?;
    let shift_left = left_shift != reverse;
    Ok(if shift_left {
        (value << magnitude).to_str_radix(10)
    } else {
        (value >> magnitude).to_str_radix(10)
    })
}
