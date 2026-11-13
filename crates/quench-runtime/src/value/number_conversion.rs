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
    if value.abs() < EXACT_I64_TRUNCATION_LIMIT {
        // NaN fails the comparison; -0 and small fractions truncate to 0.
        return value as i64 as u32;
    }
    if !value.is_finite() {
        return 0;
    }
    // At or above 2^63 the value is an integer: its significand times a power of two of
    // at least 2^11, whose low 32 bits are the significand's shifted, or none past 2^32.
    let (significand, exponent) = crate::float_layout::binary64_parts(value);
    let low = if exponent.unsigned_abs() >= u32::BITS { 0 } else { (significand << exponent) as u32 };
    if value.is_sign_negative() { low.wrapping_neg() } else { low }
}
