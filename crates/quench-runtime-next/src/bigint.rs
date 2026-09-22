use num_bigint::{BigInt, Sign};

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
