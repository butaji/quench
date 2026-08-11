//! Canonical BigInt arithmetic helpers backed by `num-bigint`.

use num_bigint::{BigInt, Sign};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    DivisionByZero,
    NegativeExponent,
    ExponentTooLarge,
    InvalidDecimal,
}

pub fn add(a: &str, b: &str) -> Result<String, Error> {
    binary(a, b, |left, right| Ok(left + right))
}

pub fn subtract(a: &str, b: &str) -> Result<String, Error> {
    binary(a, b, |left, right| Ok(left - right))
}

pub fn multiply(a: &str, b: &str) -> Result<String, Error> {
    binary(a, b, |left, right| Ok(left * right))
}

pub fn divide(a: &str, b: &str) -> Result<String, Error> {
    binary(a, b, |left, right| {
        if right == BigInt::from(0) {
            return Err(Error::DivisionByZero);
        }
        Ok(left / right)
    })
}

pub fn remainder(a: &str, b: &str) -> Result<String, Error> {
    binary(a, b, |left, right| {
        if right == BigInt::from(0) {
            return Err(Error::DivisionByZero);
        }
        Ok(left % right)
    })
}

pub fn exponentiate(a: &str, b: &str) -> Result<String, Error> {
    binary(a, b, |left, right| {
        if right.sign() == Sign::Minus {
            return Err(Error::NegativeExponent);
        }
        let exponent = right
            .to_str_radix(10)
            .parse::<u32>()
            .map_err(|_| Error::ExponentTooLarge)?;
        Ok(left.pow(exponent))
    })
}

pub fn negate(a: &str) -> Result<String, Error> {
    parse(a).map(|value| render(-value))
}

pub fn bitwise_and(a: &str, b: &str) -> Result<String, Error> {
    binary(a, b, |left, right| Ok(left & right))
}

pub fn bitwise_or(a: &str, b: &str) -> Result<String, Error> {
    binary(a, b, |left, right| Ok(left | right))
}

pub fn bitwise_xor(a: &str, b: &str) -> Result<String, Error> {
    binary(a, b, |left, right| Ok(left ^ right))
}

pub fn shift_left(a: &str, b: &str) -> Result<String, Error> {
    shift(a, b, true)
}

pub fn shift_right(a: &str, b: &str) -> Result<String, Error> {
    shift(a, b, false)
}

pub fn parse_string(value: &str) -> Option<BigInt> {
    let value = value.trim();
    if value.is_empty() {
        return Some(0.into());
    }
    let (radix, digits) = match value.as_bytes() {
        [b'0', b'x' | b'X', digits @ ..] => (16, digits),
        [b'0', b'o' | b'O', digits @ ..] => (8, digits),
        [b'0', b'b' | b'B', digits @ ..] => (2, digits),
        _ => return value.parse().ok(),
    };
    BigInt::parse_bytes(digits, radix)
}

fn shift(a: &str, b: &str, left: bool) -> Result<String, Error> {
    let value = parse(a)?;
    let count = parse(b)?;
    let reverse = count.sign() == Sign::Minus;
    let magnitude = render(if reverse { -count } else { count })
        .parse::<usize>()
        .map_err(|_| Error::ExponentTooLarge)?;
    let shift_left = left != reverse;
    Ok(render(if shift_left {
        value << magnitude
    } else {
        value >> magnitude
    }))
}

fn binary(
    a: &str,
    b: &str,
    operation: impl FnOnce(BigInt, BigInt) -> Result<BigInt, Error>,
) -> Result<String, Error> {
    let left = parse(a)?;
    let right = parse(b)?;
    operation(left, right).map(render)
}

fn parse(value: &str) -> Result<BigInt, Error> {
    value.parse::<BigInt>().map_err(|_| Error::InvalidDecimal)
}

fn render(value: BigInt) -> String {
    value.to_str_radix(10)
}
