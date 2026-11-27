//! BigInt small-magnitude arithmetic and comparison: one body.
//!
//! `small_operation` is a guard in front of `JsBigInt`'s digit paths: when both operands are
//! two-digit integers and the result is one too, it computes in `i128`; otherwise it returns `None` and
//! the general path computes the same result. `wrap_bits` is BigInt.asIntN and
//! BigInt.asUintN, and `number_ordering` orders a BigInt against a Number exactly.

use std::cmp::Ordering;

use num_bigint::{BigInt, Sign};
use crate::operator::BinaryOperator;

use crate::bigint::{Error, JsBigInt};

/// Whether `value` is below zero.
#[inline(always)]
fn is_negative(value: &JsBigInt) -> bool {
    value.cmp(&JsBigInt::ZERO) == Ordering::Less
}

/// The value as an `i128`, when its magnitude is two digits at most and below 2^127.
#[inline(always)]
fn as_i128(value: &JsBigInt) -> Option<i128> {
    let magnitude = match value.digits() {
        [] => 0,
        [low] => i128::from(*low),
        [low, high] => i128::try_from((u128::from(*high) << u64::BITS) | u128::from(*low)).ok()?,
        _ => return None,
    };
    Some(if is_negative(value) { -magnitude } else { magnitude })
}

/// The BigInt of `value`: two digits at most.
#[inline(always)]
fn from_i128(value: i128) -> JsBigInt {
    let magnitude = value.unsigned_abs();
    JsBigInt::from_digits(value < 0, &[magnitude as u64, (magnitude >> u64::BITS) as u64])
}

/// The BigInt of `value`, whose magnitude is one digit at most.
#[inline(always)]
fn from_i64(value: i64) -> JsBigInt {
    JsBigInt::from_digits(value < 0, &[value.unsigned_abs()])
}

/// `a << count` (`left`) or `a >> count`, in `i64`. A negative count shifts the other way.
/// A left shift is taken only when no bit leaves the word; a right shift is arithmetic, so
/// it rounds toward negative infinity as BigInt's does.
#[inline(always)]
fn shift(a: i64, count: i64, left: bool) -> Option<i64> {
    let left = left != (count < 0);
    let distance = count.unsigned_abs();
    if !left {
        return Some(a >> distance.min(u64::from(i64::BITS) - 1));
    }
    if distance >= u64::from(i64::BITS) {
        return None;
    }
    let shifted = a.wrapping_shl(distance as u32);
    (shifted >> distance as u32 == a).then_some(shifted)
}

/// `left op right` for a BigInt operator when both operands are two-digit integers (`as_i128`)
/// and the result is one too; `None` otherwise, and the general path computes it. Only addition,
/// subtraction, multiplication and the shifts have small forms; a shift keeps to one word.
#[inline(always)]
pub(crate) fn small_operation(operator: BinaryOperator, left: &JsBigInt, right: &JsBigInt) -> Option<JsBigInt> {
    use BinaryOperator::{Addition, Multiplication, ShiftLeft, ShiftRight, Subtraction};
    let (a, b) = (as_i128(left)?, as_i128(right)?);
    let value = match operator {
        Addition => a.checked_add(b)?,
        Subtraction => a.checked_sub(b)?,
        Multiplication => a.checked_mul(b)?,
        ShiftLeft | ShiftRight => {
            let (a, b) = (i64::try_from(a).ok()?, i64::try_from(b).ok()?);
            i128::from(shift(a, b, operator == ShiftLeft)?)
        }
        _ => return None,
    };
    Some(from_i128(value))
}

/// BigInt.asUintN (`signed` false) or BigInt.asIntN of `value` at `width` bits: the value
/// reduced modulo 2^width, and for asIntN taken into [−2^(width−1), 2^(width−1)). Fails when
/// the modulus passes the BigInt size limit.
pub(crate) fn wrap_bits(value: &JsBigInt, width: usize, signed: bool) -> Result<JsBigInt, Error> {
    if width == 0 {
        return Ok(JsBigInt::ZERO);
    }
    let digits = value.digits();
    let bits = digits.last().map_or(0, |&top| {
        (digits.len() - 1) * u64::BITS as usize + (u64::BITS - top.leading_zeros()) as usize
    });
    let unchanged = if signed {
        width > bits + 1
    } else {
        !is_negative(value) && width >= bits
    };
    if unchanged {
        return Ok(value.clone());
    }
    if width <= u64::BITS as usize {
        // Modulo 2^width the value is its two's-complement low word: the magnitude's low digit,
        // negated for a negative value (the higher digits are multiples of 2^64). Masked to the
        // width, and for asIntN sign-extended from its top bit: one digit, no division.
        let low = digits.first().copied().unwrap_or(0);
        let word = if is_negative(value) { low.wrapping_neg() } else { low };
        let unused = u64::BITS as usize - width;
        return Ok(if signed {
            from_i64(((word << unused) as i64) >> unused)
        } else {
            JsBigInt::from_digits(false, &[(word << unused) >> unused])
        });
    }
    let modulus = JsBigInt::from(1_u8).shift(&JsBigInt::from_digits(false, &[width as u64]), true)?;
    let remainder = value.div_rem(&modulus, false)?;
    let mut reduced = if is_negative(&remainder) {
        remainder.add(&modulus)
    } else {
        remainder
    };
    if signed {
        let half = modulus.shift(&JsBigInt::from(1_u8), false)?;
        if reduced.cmp(&half) != Ordering::Less {
            reduced = reduced.sub(&modulus);
        }
    }
    Ok(reduced)
}

/// The ordering of a BigInt against a Number, `None` when the Number is NaN. Infinities
/// and signs decide first; otherwise the exact binary value of the Number is compared with
/// the integer, so no rounding can make them equal.
pub(crate) fn number_ordering(integer: &JsBigInt, number: f64) -> Option<Ordering> {
    let integer = integer.to_num();
    if number.is_nan() {
        return None;
    }
    if number == f64::INFINITY {
        return Some(Ordering::Less);
    }
    if number == f64::NEG_INFINITY {
        return Some(Ordering::Greater);
    }
    let sign = integer.sign();
    if number == 0.0 {
        return Some(integer.cmp(&BigInt::ZERO));
    }
    if sign == Sign::Minus && number.is_sign_positive() {
        return Some(Ordering::Less);
    }
    if sign != Sign::Minus && number.is_sign_negative() {
        return Some(Ordering::Greater);
    }
    let ordering = compare_positive_bigint_number(BigInt::from(integer.magnitude().clone()), number.abs());
    Some(if number.is_sign_negative() {
        ordering.reverse()
    } else {
        ordering
    })
}

/// The ordering of a positive integer against a positive finite Number.
fn compare_positive_bigint_number(integer: BigInt, number: f64) -> Ordering {
    let (significand, exponent) = crate::float_layout::binary64_parts(number);
    let significand = BigInt::from(significand);
    if exponent >= 0 {
        integer.cmp(&(significand << exponent as usize))
    } else {
        (integer << (-exponent) as usize).cmp(&significand)
    }
}
