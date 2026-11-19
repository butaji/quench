//! Math functions of Number arguments that a machine runs inline: their value once, for the
//! interpreter (`Concrete`, `vm::number`) and for native code (`Emit`).
use super::numeric::NumericStage;
#[cfg(not(stencil_build))]
use crate::float_layout::{BINARY16, BINARY64, power_of_two};

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

/// The fraction of the halfway point between two integers: `Math.round` rounds up at it, and
/// binary16 rounds to even at it.
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

/// `Math.max` (`maximum`) or `Math.min` of two int32 values: an int32 is never -0.
#[inline(always)]
pub(crate) fn int_extreme(maximum: bool, left: i32, right: i32) -> i32 {
    if maximum { left.max(right) } else { left.min(right) }
}

/// `Math.abs` of an int32, whose magnitude is an int32 except at -2^31 (`None`, for the double path).
#[inline(always)]
pub(crate) fn int_abs(value: i32) -> Option<i32> {
    (value != i32::MIN).then(|| value.abs())
}

/// `Math.sign`: `NaN` and the zeros unchanged, otherwise -1 or 1 by the sign.
#[inline(always)]
pub(crate) fn sign(value: f64) -> f64 {
    if value.is_nan() || value == 0.0 {
        value
    } else if value.is_sign_negative() {
        -1.0
    } else {
        1.0
    }
}

/// `Math.clz32`: the leading zero bits of the ToUint32 of `value`, as a Number.
#[inline(always)]
#[cfg(not(stencil_build))]
pub(crate) fn clz32(value: f64) -> f64 {
    f64::from(to_uint32(value).leading_zeros())
}

/// The modulus of ToUint32, 2^32.
#[cfg(not(stencil_build))]
const UINT32_MODULUS: f64 = 4_294_967_296.0;

/// ToUint32 of `value`: its integer part modulo 2^32, zero for NaN and the infinities. Here
/// rather than in `value`, since this module also builds into the baseline JIT's stencils.
#[cfg(not(stencil_build))]
#[inline(always)]
fn to_uint32(value: f64) -> u32 {
    if !value.is_finite() {
        return 0;
    }
    value.trunc().rem_euclid(UINT32_MODULUS) as u32
}

/// `Math.hypot` of Numbers: infinite if any is (even beside a NaN), NaN if any is, else the
/// root of the sum of squares, scaled by the largest magnitude so the squares do not overflow.
#[cfg(not(stencil_build))]
pub(crate) fn hypot(values: &[f64]) -> f64 {
    if values.iter().any(|value| value.is_infinite()) {
        return f64::INFINITY;
    }
    if values.iter().any(|value| value.is_nan()) {
        return f64::NAN;
    }
    let largest = values.iter().copied().map(f64::abs).fold(0.0, f64::max);
    if largest == 0.0 {
        return 0.0;
    }
    let squares = values.iter().map(|value| value / largest).map(|ratio| ratio * ratio).sum::<f64>();
    largest * squares.sqrt()
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

// The log1p forms meet at this magnitude. Below it, retain the correction
// separately from 2x to avoid cancellation; above it, use 2x / (1 - x).
#[cfg(not(stencil_build))]
const ATANH_LOG_REDUCTION_BOUNDARY: f64 = 0.5;
#[cfg(not(stencil_build))]
const ATANH_DOMAIN_ENDPOINT: f64 = 1.0;
#[cfg(not(stencil_build))]
const ATANH_LOG_SCALE: f64 = 0.5;

/// `Math.atanh`: an odd function, NaN beyond ±1, and ±Infinity at ±1.
#[cfg(not(stencil_build))]
pub(crate) fn atanh(value: f64) -> f64 {
    if value.is_nan() || value == 0.0 {
        return value;
    }
    let magnitude = value.abs();
    if magnitude > ATANH_DOMAIN_ENDPOINT {
        return f64::NAN;
    }
    if magnitude == ATANH_DOMAIN_ENDPOINT {
        return f64::INFINITY.copysign(value);
    }
    let doubled = magnitude + magnitude;
    let argument = if magnitude < ATANH_LOG_REDUCTION_BOUNDARY {
        doubled + doubled * magnitude / (1.0 - magnitude)
    } else {
        doubled / (1.0 - magnitude)
    };
    (ATANH_LOG_SCALE * argument.ln_1p()).copysign(value)
}

/// `Math.f16round`: the nearest binary16, as a double.
#[cfg(not(stencil_build))]
pub(crate) fn f16_round(value: f64) -> f64 {
    half_to_f64(f64_to_half(value))
}

/// The binary16 bits of `value`, rounded to nearest with ties to even; the conversion that
/// Float16Array and DataView share with `Math.f16round`.
#[cfg(not(stencil_build))]
pub(crate) fn f64_to_half(value: f64) -> u16 {
    let (wide, half) = (BINARY64, BINARY16);
    let narrowing = wide.fraction_bits - half.fraction_bits;
    let bits = value.to_bits();
    let sign = ((bits >> wide.sign_shift()) as u16) << half.sign_shift();
    let exponent = (bits >> wide.fraction_bits) & wide.exponent_mask();
    let fraction = bits & wide.fraction_mask();
    let infinity = half.infinity() as u16;
    if exponent == wide.exponent_mask() {
        // A NaN keeps its top payload bits, and at least one so it stays a NaN.
        return sign
            | if fraction == 0 { infinity } else { infinity | ((fraction >> narrowing) as u16).max(1) };
    }
    let absolute = value.abs();
    if absolute < power_of_two(half.min_normal_exponent()) {
        // Rounding up past the subnormals gives the smallest normal.
        let rounded = round_half(absolute * power_of_two(-half.subnormal_exponent()));
        return sign | rounded.min(half.implicit_bit() as u16);
    }
    let unbiased = exponent as i32 - wide.bias();
    if unbiased > half.bias() {
        return sign | infinity;
    }
    let mut significand = (fraction >> narrowing) as u16;
    let remainder = fraction & ((1 << narrowing) - 1);
    let halfway = 1 << (narrowing - 1);
    if remainder > halfway || (remainder == halfway && significand & 1 != 0) {
        significand += 1;
    }
    // A significand rounded up to the implicit bit carries into the exponent, and from the
    // largest exponent into infinity.
    let encoded = (((unbiased + half.bias()) as u16) << half.fraction_bits) + significand;
    sign | encoded.min(infinity)
}

/// The integer nearest `value` (a binary16 subnormal's scaled magnitude), halves to even.
#[cfg(not(stencil_build))]
fn round_half(value: f64) -> u16 {
    let lower = value.floor() as u64;
    let fraction = value - lower as f64;
    (lower + u64::from(fraction > ROUND_HALF || (fraction == ROUND_HALF && lower & 1 != 0))) as u16
}

/// The binary64 value of the binary16 `bits`, exact.
#[cfg(not(stencil_build))]
pub(crate) fn half_to_f64(bits: u16) -> f64 {
    let (wide, half) = (BINARY64, BINARY16);
    let bits = u64::from(bits);
    let sign = (bits >> half.sign_shift()) << wide.sign_shift();
    let magnitude = bits & !(1 << half.sign_shift());
    let magnitude = if magnitude >= half.infinity() {
        // Infinity and NaN keep their payload in the top fraction bits.
        let payload = (magnitude & half.fraction_mask()) << (wide.fraction_bits - half.fraction_bits);
        f64::from_bits(wide.infinity() | payload)
    } else {
        let (significand, exponent) = half.parts(magnitude);
        significand as f64 * power_of_two(exponent)
    };
    f64::from_bits(magnitude.to_bits() | sign)
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
