//! Number to 32-bit integer conversions: ToUint32 and the truncation the value encoding uses.

/// `value` truncated toward zero when it fits `i32`; any other result is some `i32` whose
/// conversion back differs from `value`. x86-64 truncates directly (out-of-range and NaN
/// inputs give `i32::MIN`), skipping the saturation a Rust cast adds.
#[cfg(target_arch = "x86_64")]
#[inline(always)]
pub(super) fn truncate_to_i32(value: f64) -> i32 {
    use std::arch::x86_64::{_mm_cvttsd_si32, _mm_set_sd};
    // SAFETY: SSE2 is part of the x86-64 baseline.
    #[allow(unused_unsafe)]
    unsafe {
        _mm_cvttsd_si32(_mm_set_sd(value))
    }
}

#[cfg(not(target_arch = "x86_64"))]
#[inline(always)]
pub(super) fn truncate_to_i32(value: f64) -> i32 {
    value as i32
}

/// Magnitudes below 2^63 truncate exactly into an `i64`, whose low 32 bits are the value
/// modulo 2^32.
pub(crate) const EXACT_I64_TRUNCATION_LIMIT: f64 = 9_223_372_036_854_775_808.0;

/// ToUint32 (ToInt32 is its bits reinterpreted as `i32`).
#[inline]
pub fn number_to_u32(value: f64) -> u32 {
    // One 64-bit `cvttsd2si` and a compare: below 2^63 in magnitude the truncation is exact
    // and its low 32 bits are the value modulo 2^32. Only `i64::MIN` (out of range, NaN, or
    // exactly -2^63) takes the wide conversion, which answers each of those.
    #[cfg(target_arch = "x86_64")]
    {
        use std::arch::x86_64::{_mm_cvttsd_si64, _mm_set_sd};
        // SAFETY: SSE2 is part of the x86-64 baseline.
        #[allow(unused_unsafe)]
        let truncated = unsafe { _mm_cvttsd_si64(_mm_set_sd(value)) };
        if truncated != i64::MIN { truncated as u32 } else { number_to_u32_wide(value) }
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        if value.abs() < EXACT_I64_TRUNCATION_LIMIT {
            // NaN fails the comparison; -0 and small fractions truncate to 0.
            return value as i64 as u32;
        }
        number_to_u32_wide(value)
    }
}

/// ToInt32: ToUint32's bits reinterpreted as `i32`.
#[inline(always)]
pub fn number_to_i32(value: f64) -> i32 {
    number_to_u32(value) as i32
}

/// Adding 2^52 to a binary64 in [0, 2^52) leaves no fraction bits, so the addition itself
/// rounds the value to an integer, ties to even.
const ROUNDING_BIAS: f64 = 4_503_599_627_370_496.0;

/// ToUint8Clamp: NaN and anything at or below 0 give 0, 255 and above give 255, and the rest
/// round to the nearest integer, ties to even. The rounding is the bias's add and subtract,
/// two instructions where `round_ties_even` is a libm call on the x86-64 baseline.
#[inline(always)]
pub fn number_to_uint8_clamp(number: f64) -> u8 {
    if !(number > 0.0) {
        return 0;
    }
    if number >= f64::from(u8::MAX) {
        return u8::MAX;
    }
    ((number + ROUNDING_BIAS) - ROUNDING_BIAS) as u8
}

/// `number_to_u32` at or past 2^63 and of non-finite values, out of line.
#[cold]
#[inline(never)]
fn number_to_u32_wide(value: f64) -> u32 {
    if !value.is_finite() {
        return 0;
    }
    // At or above 2^63 the value is an integer: its significand times a power of two of
    // at least 2^11, whose low 32 bits are the significand's shifted, or none past 2^32.
    let (significand, exponent) = crate::float_layout::binary64_parts(value);
    let low = if exponent.unsigned_abs() >= u32::BITS { 0 } else { (significand << exponent) as u32 };
    if value.is_sign_negative() { low.wrapping_neg() } else { low }
}
