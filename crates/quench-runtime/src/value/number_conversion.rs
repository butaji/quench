//! Number to 32-bit integer conversions: ToUint32 and the truncation the value encoding uses.

#[cfg(not(stencil_build))]
use crate::float_layout::binary64_parts;
// The stencil build mounts the same file (`float_layout.rs`), which the runtime's root names `crate::float_layout`.
#[cfg(stencil_build)]
#[allow(dead_code)]
#[path = "../float_layout.rs"]
mod float_layout;
#[cfg(stencil_build)]
use float_layout::binary64_parts;

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

/// The bits of 2^63, the same limit as `EXACT_I64_TRUNCATION_LIMIT`, for the integer test in `exact_uint32`.
const EXACT_I64_TRUNCATION_LIMIT_BITS: u64 = 0x43E0_0000_0000_0000;

/// `value` truncated toward zero to an `i64`, for a `value` below 2^63 in magnitude (see
/// `exact_uint32`). The hardware conversion, because Rust's `as` saturates, and its bounds would
/// be loaded from a constant pool, which a stencil may not use.
#[cfg(target_arch = "x86_64")]
#[inline(always)]
fn truncate_to_i64(value: f64) -> i64 {
    use std::arch::x86_64::{_mm_cvttsd_si64, _mm_set_sd};
    // SAFETY: SSE2 is part of the x86-64 baseline.
    #[allow(unused_unsafe)]
    unsafe {
        _mm_cvttsd_si64(_mm_set_sd(value))
    }
}

/// `fcvtzs` to 64 bits, the same conversion for the stencil build on AArch64 (no constant pool).
#[cfg(target_arch = "aarch64")]
#[inline(always)]
fn truncate_to_i64(value: f64) -> i64 {
    let out: i64;
    // SAFETY: one scalar conversion, no memory or flags touched.
    unsafe { core::arch::asm!("fcvtzs {out:x}, {input:d}", out = out(reg) out, input = in(vreg) value) };
    out
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
#[inline(always)]
fn truncate_to_i64(value: f64) -> i64 {
    value as i64
}

/// ToUint32 of `value` when `value` truncates exactly into an `i64`: below 2^63 in magnitude,
/// where the truncation's low 32 bits are the value modulo 2^32. `None` for NaN, the infinities
/// and magnitudes from 2^63 up (-2^63 included), which take `number_to_u32_wide`. The test reads
/// the bits, so it is one integer compare and needs no float constant.
#[inline(always)]
pub(crate) fn exact_uint32(value: f64) -> Option<u32> {
    let magnitude = value.to_bits() & !(1 << 63);
    (magnitude < EXACT_I64_TRUNCATION_LIMIT_BITS).then(|| truncate_to_i64(value) as u32)
}

/// ToUint32 (ToInt32 is its bits reinterpreted as `i32`): `exact_uint32`, or the wide conversion.
#[inline]
pub fn number_to_u32(value: f64) -> u32 {
    match exact_uint32(value) {
        Some(uint32) => uint32,
        None => number_to_u32_wide(value),
    }
}

/// ToInt32: ToUint32's bits reinterpreted as `i32`.
#[inline(always)]
pub fn number_to_i32(value: f64) -> i32 {
    number_to_u32(value) as i32
}

/// ToUint8Clamp, the body `staged::math::clamp_u8` shares with the interpreter and the stencils.
/// Not in the Wasm rows' stencil family, which does not compile `staged::math` (`stencil_family`).
#[cfg(not(any(stencil_family = "wasm", stencil_family = "regexp")))]
#[inline(always)]
pub fn number_to_uint8_clamp(number: f64) -> u8 {
    crate::staged::math::clamp_u8(number)
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
    let (significand, exponent) = binary64_parts(value);
    let low = if exponent.unsigned_abs() >= u32::BITS { 0 } else { (significand << exponent) as u32 };
    if value.is_sign_negative() { low.wrapping_neg() } else { low }
}
