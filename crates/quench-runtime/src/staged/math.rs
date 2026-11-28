//! Math functions of Number arguments that a machine runs inline: their value once, for the
//! interpreter (`Concrete`, `vm::number`) and for native code (`Emit`).
use super::numeric::{Domain, NumericStage};
use super::Concrete;
use crate::vm::operations::{NumberArithmetic, NumberRelation};
#[cfg(not(stencil_build))]
use crate::float_layout::{BINARY16, BINARY64, power_of_two};
// The fdlibm and libm bodies of `atan`, `atan2` and `cbrt`; the stencil build has no libm.
#[cfg(not(stencil_build))]
#[path = "fdlibm.rs"]
mod fdlibm;
#[cfg(not(stencil_build))]
pub(crate) use fdlibm::{atan, atan2, cbrt};

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
    #[inline(always)]
    pub(super) fn abs(value: f64) -> f64 { value.abs() }
    #[inline(always)]
    pub(super) fn constant(value: f64) -> f64 { value }
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
    #[inline(always)]
    pub(super) fn abs(value: f64) -> f64 { value.abs() }
    #[inline(always)]
    pub(super) fn constant(value: f64) -> f64 { value }
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
    /// The magnitude: the sign bit cleared through a mask in a register, which LLVM would otherwise load
    /// from a constant pool a stencil cannot reach.
    #[inline(always)]
    pub(super) fn abs(value: f64) -> f64 { f64::from_bits(value.to_bits() & constant_bits(!(1 << 63))) }
    /// `value` materialized as an immediate (`movabs`): a stencil holds no constant pool.
    #[inline(always)]
    pub(super) fn constant(value: f64) -> f64 { f64::from_bits(constant_bits(value.to_bits())) }
    #[inline(always)]
    fn constant_bits(mut bits: u64) -> u64 {
        // SAFETY: an empty template: the value passes through a register unchanged, opaque to constant folding.
        unsafe { core::arch::asm!("/* {0} */", inout(reg) bits, options(pure, nomem, nostack, preserves_flags)) };
        bits
    }
}

/// A Math function of one Number that is a machine primitive or a short composition of them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FloatUnary {
    Floor,
    Ceil,
    Trunc,
    Abs,
    Sqrt,
    /// The nearest binary32, as a double.
    Fround,
    /// The nearest integer, halves toward `+Infinity`, `-0` for a negative that rounds to zero.
    Round,
    /// `Math.sign`: the sign of a Number, with NaN and the zeros returned as they are.
    Sign,
}

/// The value of `operation` on `value`: the machine rows (`float_primitive`), and `Math.round` and
/// `Math.sign` as their generic bodies run on `Concrete`.
#[inline(always)]
pub(crate) fn float_unary(operation: FloatUnary, value: f64) -> f64 {
    match operation {
        FloatUnary::Round => f64::from_bits(round(&mut Concrete::new(core::ptr::null()), value.to_bits())),
        FloatUnary::Sign => f64::from_bits(sign(&mut Concrete::new(core::ptr::null()), value.to_bits())),
        primitive => float_primitive(primitive, value),
    }
}

/// The value of the machine primitive `operation` on `value`; a composite (`Round`, `Sign`) is not
/// one, and passes `value` through (`NumericStage::float_primitive` never asks for it).
#[inline(always)]
pub(crate) fn float_primitive(operation: FloatUnary, value: f64) -> f64 {
    match operation {
        FloatUnary::Floor => machine::floor(value),
        FloatUnary::Ceil => machine::ceil(value),
        FloatUnary::Trunc => machine::trunc(value),
        FloatUnary::Abs => machine::abs(value),
        FloatUnary::Sqrt => machine::sqrt(value),
        FloatUnary::Fround => f64::from(value as f32),
        FloatUnary::Round | FloatUnary::Sign => value,
    }
}

/// `Math.sign` of the binary64 word `number`: NaN and either zero as they are, else -1 below zero
/// and 1 above. The one body every stage runs; `float_unary` and the u-jit's Emit both instantiate it.
#[inline(always)]
pub fn sign<S: NumericStage>(stage: &mut S, number: S::Word) -> S::Word {
    let ordered = stage.relation(NumberRelation::Equal, Domain::Float64, number, number);
    let unordered = stage.not(ordered);
    let zero = stage.f64_constant(0.0);
    let is_zero = stage.relation(NumberRelation::Equal, Domain::Float64, number, zero);
    let kept = stage.any(unordered, is_zero);
    let negative = stage.relation(NumberRelation::LessThan, Domain::Float64, number, zero);
    let (minus_one, one) = (stage.f64_constant(-1.0), stage.f64_constant(1.0));
    let unit = stage.select(negative, minus_one, one);
    stage.select(kept, number, unit)
}

/// `2^52`, the binary64 whose unit in the last place is one: adding it to a binary64 in `[0, 2^52)` leaves no
/// fraction bits, so the addition itself rounds the value to an integer, ties to even.
const ROUNDING_BIAS: f64 = 4_503_599_627_370_496.0;

/// ToUint8Clamp of the binary64 `number`, the one body the interpreter (`Concrete::clamp_u8`), the b-jit stencil
/// (`units::clamp_u8`) and `number_to_uint8_clamp` share: NaN and anything at or below zero give 0, `u8::MAX` and
/// above give `u8::MAX`, and the rest round to the nearest integer, ties to even. Where `number` lies in
/// `(0, u8::MAX)`, `number + 2^52` is `2^52` plus that integer, so the integer is the low byte of the sum's bits.
/// The constants are immediates (`machine::constant`), so the stencil build holds no constant pool.
#[inline(always)]
#[cfg_attr(feature = "test-support", quench_macros::test_visible)]
pub(crate) fn clamp_u8(number: f64) -> u8 {
    if !(number > machine::constant(0.0)) {
        return 0;
    }
    if number >= machine::constant(f64::from(u8::MAX)) {
        return u8::MAX;
    }
    (number + machine::constant(ROUNDING_BIAS)).to_bits() as u8
}

/// The fraction of the halfway point between two integers: `Math.round` rounds up at it, and
/// binary16 rounds to even at it.
pub(crate) const ROUND_HALF: f64 = 0.5;

/// `Math.round` of the binary64 word `value`, on `stage`: the floor, one more where the excess over
/// it reaches `ROUND_HALF`, so halves go toward `+Infinity`. A negative that rounds to zero is `-0`,
/// and a zero, NaN or infinity is its own value (`-0` stays `-0`).
#[inline(always)]
pub fn round<S: NumericStage>(stage: &mut S, value: S::Word) -> S::Word {
    let floor = stage.float_primitive(FloatUnary::Floor, value);
    let excess = stage.float_arithmetic(NumberArithmetic::Subtract, value, floor);
    let half = stage.f64_constant(ROUND_HALF);
    let up = stage.relation(NumberRelation::GreaterEqual, Domain::Float64, excess, half);
    let (one, zero) = (stage.f64_constant(1.0), stage.f64_constant(0.0));
    let step = stage.select(up, one, zero);
    let rounded = stage.float_arithmetic(NumberArithmetic::Add, floor, step);
    let rounds_to_zero = stage.relation(NumberRelation::Equal, Domain::Float64, rounded, zero);
    let negative = stage.relation(NumberRelation::LessThan, Domain::Float64, value, zero);
    let negative_zero = stage.both(rounds_to_zero, negative);
    let minus_zero = stage.f64_constant(-0.0);
    let signed = stage.select(negative_zero, minus_zero, rounded);
    let is_zero = stage.relation(NumberRelation::Equal, Domain::Float64, value, zero);
    stage.select(is_zero, value, signed)
}

/// `Math.max` (`maximum`) or `Math.min` of two Numbers: NaN if either is, and `-0` below `+0`
/// (the minimum of zeros is `-0` when either is, the maximum only when both are).
#[inline(always)]
pub(crate) fn float_extreme(maximum: bool, left: f64, right: f64) -> f64 {
    if left.is_nan() || right.is_nan() {
        return machine::constant(f64::NAN);
    }
    if left == 0.0 && right == 0.0 {
        let negative = if maximum {
            left.is_sign_negative() && right.is_sign_negative()
        } else {
            left.is_sign_negative() || right.is_sign_negative()
        };
        return if negative { machine::constant(-0.0) } else { machine::constant(0.0) };
    }
    if maximum { left.max(right) } else { left.min(right) }
}

/// `Math.imul`: the one wrapping int32 product every family shares (`int_math`).
pub(crate) use super::int_math::int_multiply;

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

/// `Math.clz32`: the leading zero bits of a ToUint32, given as that ToUint32. Every caller takes it from
/// `value::number_to_u32` (or, in a stencil row, `value::exact_uint32`, which leaves the rest to it).
#[inline(always)]
pub(crate) fn leading_zeros(uint32: u32) -> u32 {
    uint32.leading_zeros()
}

/// `Math.clz32` of the int32 word `value`, on `stage`: its unsigned low 32 bits' leading zeros.
#[inline(always)]
pub fn count_leading_zeros<S: NumericStage>(stage: &mut S, value: S::Word) -> S::Word {
    stage.int_leading_zeros(value)
}

/// `operation` of the binary64 word `value`, on `stage`.
#[inline(always)]
pub fn unary<S: NumericStage>(stage: &mut S, operation: FloatUnary, value: S::Word) -> S::Word {
    stage.float_unary(operation, value)
}

/// The wrapping product of two int32 words, on `stage`.
#[inline(always)]
pub fn multiply<S: NumericStage>(stage: &mut S, left: S::Word, right: S::Word) -> S::Word {
    stage.int_multiply(left, right)
}

/// `Math.max` (`maximum`) or `Math.min` of two binary64 words, on `stage`.
#[inline(always)]
pub fn extreme<S: NumericStage>(stage: &mut S, maximum: bool, left: S::Word, right: S::Word) -> S::Word {
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

#[cfg(not(stencil_build))]
const EXPONENTIATION_ZERO: f64 = 0.0;
#[cfg(not(stencil_build))]
const EXPONENTIATION_ONE: f64 = 1.0;
#[cfg(not(stencil_build))]
const EXPONENTIATION_TWO: f64 = 2.0;
#[cfg(not(stencil_build))]
const ODD_INTEGER_PARITY: f64 = 1.0;

/// `Number ** Number` (`Math.pow`): NaN for a NaN exponent or an infinite exponent with a base
/// of ±1, `1` for a zero exponent, and the signed infinities and zeros of the base at the edges.
#[cfg(not(stencil_build))]
pub(crate) fn power(base: f64, exponent: f64) -> f64 {
    if exponent.is_nan() {
        return f64::NAN;
    }
    if exponent == EXPONENTIATION_ZERO {
        return EXPONENTIATION_ONE;
    }
    if base.is_nan() || base.abs() == EXPONENTIATION_ONE && exponent.is_infinite() {
        return f64::NAN;
    }
    if base.is_infinite() {
        return infinite_power(base, exponent);
    }
    if base == EXPONENTIATION_ZERO {
        return zero_power(base, exponent);
    }
    base.powf(exponent)
}

/// The power of an infinite base: infinity for a positive exponent, zero otherwise, signed by the base.
#[cfg(not(stencil_build))]
fn infinite_power(base: f64, exponent: f64) -> f64 {
    let magnitude = if exponent.is_sign_positive() {
        f64::INFINITY
    } else {
        EXPONENTIATION_ZERO
    };
    signed_power_magnitude(base, exponent, magnitude)
}

/// The power of a zero base: zero for a positive exponent, infinity otherwise, signed by the base.
#[cfg(not(stencil_build))]
fn zero_power(base: f64, exponent: f64) -> f64 {
    let magnitude = if exponent.is_sign_positive() {
        EXPONENTIATION_ZERO
    } else {
        f64::INFINITY
    };
    signed_power_magnitude(base, exponent, magnitude)
}

/// `magnitude`, negated when the base is negative and the exponent an odd integer.
#[cfg(not(stencil_build))]
fn signed_power_magnitude(base: f64, exponent: f64, magnitude: f64) -> f64 {
    if base.is_sign_negative() && is_odd_integer(exponent) {
        -magnitude
    } else {
        magnitude
    }
}

/// Whether `value` is a finite integer with an odd magnitude.
#[cfg(not(stencil_build))]
fn is_odd_integer(value: f64) -> bool {
    value.is_finite()
        && value.fract() == EXPONENTIATION_ZERO
        && value.abs() % EXPONENTIATION_TWO == ODD_INTEGER_PARITY
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
