//! Math functions of Number arguments that a machine runs inline: their value once, for the
//! interpreter (`Concrete`, `vm::number`) and for native code (`Emit`).
use super::numeric::NumericStage;

/// The `f64` rounding and root operations as machine primitives: the standard library's methods in the
/// runtime, `core`'s float math in the stencil build, which has no standard library.
#[cfg(all(not(stencil_build), not(target_arch = "x86_64")))]
mod machine {
    #[inline(always)]
    pub(super) fn floor(value: f64) -> f64 { value.floor() }
    #[inline(always)]
    pub(super) fn ceil(value: f64) -> f64 { value.ceil() }
    #[inline(always)]
    pub(super) fn trunc(value: f64) -> f64 { value.trunc() }
    #[inline(always)]
    pub(super) fn sqrt(value: f64) -> f64 { value.sqrt() }
}
/// Baseline x86-64 has no `roundsd` (SSE4.1), so the standard library's rounding is a libm
/// call (~23 instructions for `floor`). Below 2^52 in magnitude a value truncates exactly
/// with one `cvttsd2si`, and floor and ceil step that by one; the sign is the operand's, so
/// `-0` and negatives that round to zero stay `-0`. At or past 2^52 every double is an
/// integer (or NaN, or an infinity), its own rounding.
#[cfg(all(not(stencil_build), target_arch = "x86_64"))]
mod machine {
    const INTEGRAL: f64 = 4_503_599_627_370_496.0;
    /// `value` truncated toward zero, without the sign; `value` below 2^52 in magnitude.
    #[inline(always)]
    fn truncated(value: f64) -> f64 {
        use std::arch::x86_64::{_mm_cvttsd_si64, _mm_set_sd};
        // SAFETY: SSE2 is part of the x86-64 baseline.
        #[allow(unused_unsafe)]
        let integer = unsafe { _mm_cvttsd_si64(_mm_set_sd(value)) };
        integer as f64
    }
    #[inline(always)]
    pub(super) fn floor(value: f64) -> f64 {
        if !(value.abs() < INTEGRAL) { return value; }
        let truncated = truncated(value);
        (if truncated > value { truncated - 1.0 } else { truncated }).copysign(value)
    }
    #[inline(always)]
    pub(super) fn ceil(value: f64) -> f64 {
        if !(value.abs() < INTEGRAL) { return value; }
        let truncated = truncated(value);
        (if truncated < value { truncated + 1.0 } else { truncated }).copysign(value)
    }
    #[inline(always)]
    pub(super) fn trunc(value: f64) -> f64 {
        if !(value.abs() < INTEGRAL) { return value; }
        truncated(value).copysign(value)
    }
    #[inline(always)]
    pub(super) fn sqrt(value: f64) -> f64 { value.sqrt() }
}
#[cfg(stencil_build)]
mod machine {
    #[inline(always)]
    pub(super) fn floor(value: f64) -> f64 { core::f64::math::floor(value) }
    #[inline(always)]
    pub(super) fn ceil(value: f64) -> f64 { core::f64::math::ceil(value) }
    #[inline(always)]
    pub(super) fn trunc(value: f64) -> f64 { core::f64::math::trunc(value) }
    #[inline(always)]
    pub(super) fn sqrt(value: f64) -> f64 { core::f64::math::sqrt(value) }
}

/// A Math function of one Number that is a machine primitive or a short composition of them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum FloatUnary {
    Floor,
    Ceil,
    Trunc,
    Abs,
    Sqrt,
    /// The nearest binary32, as a double.
    Fround,
    /// The nearest integer, halves toward `+Infinity`, `-0` for a negative that rounds to zero.
    Round,
}

/// The value of `operation` on `value`.
#[inline(always)]
pub(crate) fn float_unary(operation: FloatUnary, value: f64) -> f64 {
    match operation {
        FloatUnary::Floor => machine::floor(value),
        FloatUnary::Ceil => machine::ceil(value),
        FloatUnary::Trunc => machine::trunc(value),
        FloatUnary::Abs => value.abs(),
        FloatUnary::Sqrt => machine::sqrt(value),
        FloatUnary::Fround => f64::from(value as f32),
        FloatUnary::Round => round_half_up(value),
    }
}

/// The fraction at which `Math.round` rounds up.
pub(crate) const ROUND_HALF: f64 = 0.5;

/// `Math.round`: the nearest integer, halves toward `+Infinity`, `-0` for a negative that rounds to zero.
#[inline(always)]
fn round_half_up(value: f64) -> f64 {
    if value.is_nan() || value == 0.0 || value.is_infinite() {
        value
    } else if (-ROUND_HALF..0.0).contains(&value) {
        -0.0
    } else {
        let floor = machine::floor(value);
        if value - floor < ROUND_HALF { floor } else { floor + 1.0 }
    }
}

/// `Math.max` (`maximum`) or `Math.min` of two Numbers: NaN if either is, and `-0` below `+0`
/// (the minimum of zeros is `-0` when either is, the maximum only when both are).
#[inline(always)]
pub(crate) fn float_extreme(maximum: bool, left: f64, right: f64) -> f64 {
    if left.is_nan() || right.is_nan() {
        return f64::NAN;
    }
    if left == 0.0 && right == 0.0 {
        let negative = if maximum {
            left.is_sign_negative() && right.is_sign_negative()
        } else {
            left.is_sign_negative() || right.is_sign_negative()
        };
        return if negative { -0.0 } else { 0.0 };
    }
    if maximum { left.max(right) } else { left.min(right) }
}

/// `Math.imul`: the wrapping product of two int32 values.
#[inline(always)]
pub(crate) fn int_multiply(left: i32, right: i32) -> i32 {
    left.wrapping_mul(right)
}

/// `operation` of the binary64 word `value`, on `stage`.
#[inline(always)]
pub(crate) fn unary<S: NumericStage>(stage: &mut S, operation: FloatUnary, value: S::Word) -> S::Word {
    stage.float_unary(operation, value)
}

/// The wrapping product of two int32 words, on `stage`.
#[inline(always)]
pub(crate) fn multiply<S: NumericStage>(stage: &mut S, left: S::Word, right: S::Word) -> S::Word {
    stage.int_multiply(left, right)
}

/// `Math.max` (`maximum`) or `Math.min` of two binary64 words, on `stage`.
#[inline(always)]
pub(crate) fn extreme<S: NumericStage>(stage: &mut S, maximum: bool, left: S::Word, right: S::Word) -> S::Word {
    stage.float_extreme(maximum, left, right)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extremes_order_signed_zeros_and_propagate_nan() {
        assert!(float_extreme(true, f64::NAN, 1.0).is_nan());
        assert!(float_extreme(false, 1.0, f64::NAN).is_nan());
        assert!(float_extreme(false, 0.0, -0.0).is_sign_negative());
        assert!(float_extreme(true, 0.0, -0.0).is_sign_positive());
        assert!(float_extreme(true, -0.0, -0.0).is_sign_negative());
        assert_eq!(float_extreme(true, 1.0, 2.0), 2.0);
        assert_eq!(float_extreme(false, 1.0, 2.0), 1.0);
    }

    #[test]
    fn round_takes_halves_up_and_keeps_negative_zero() {
        assert_eq!(float_unary(FloatUnary::Round, 2.5), 3.0);
        assert_eq!(float_unary(FloatUnary::Round, -2.5), -2.0);
        assert!(float_unary(FloatUnary::Round, -0.2).is_sign_negative());
        assert!(float_unary(FloatUnary::Round, f64::NAN).is_nan());
    }
}
