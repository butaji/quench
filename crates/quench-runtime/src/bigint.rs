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
