//! BigInt small-magnitude arithmetic and comparison: one body.
//!
//! `small_operation` is a guard in front of `JsBigInt`'s digit paths: when both operands are
//! `i64`s and the result is one too, it computes in `i64`; otherwise it returns `None` and
//! the general path computes the same result. `wrap_bits` is BigInt.asIntN and
//! BigInt.asUintN, and `number_ordering` orders a BigInt against a Number exactly.

use std::cmp::Ordering;

use num_bigint::{BigInt, Sign};
use oxc_ast::ast::BinaryOperator;

use crate::bigint::{Error, JsBigInt};

/// Whether `value` is below zero.
#[inline(always)]
fn is_negative(value: &JsBigInt) -> bool {
    value.cmp(&JsBigInt::ZERO) == Ordering::Less
}

/// The value as an `i64`, when its magnitude is at most `i64::MAX`.
#[inline(always)]
fn as_i64(value: &JsBigInt) -> Option<i64> {
    let magnitude = match value.digits() {
        [] => 0,
        [digit] => i64::try_from(*digit).ok()?,
        _ => return None,
    };
    Some(if is_negative(value) { -magnitude } else { magnitude })
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

/// `left op right` for a BigInt operator when both operands are `i64`s and the result is
/// one too; `None` otherwise, and the general path computes it. Only addition, subtraction,
/// multiplication and the shifts have small forms.
#[inline(always)]
pub(crate) fn small_operation(operator: BinaryOperator, left: &JsBigInt, right: &JsBigInt) -> Option<JsBigInt> {
    use BinaryOperator::{Addition, Multiplication, ShiftLeft, ShiftRight, Subtraction};
    if !matches!(operator, Addition | Subtraction | Multiplication | ShiftLeft | ShiftRight) {
        return None;
    }
    let (a, b) = (as_i64(left)?, as_i64(right)?);
    let value = match operator {
        Addition => a.checked_add(b)?,
        Subtraction => a.checked_sub(b)?,
        Multiplication => a.checked_mul(b)?,
        ShiftLeft => shift(a, b, true)?,
        _ => shift(a, b, false)?,
    };
    Some(from_i64(value))
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
