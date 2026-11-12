//! The words and registers the rows compute on: the state (`S`), a row's outcome (`Out`), the
//! patched holes, tagged-word tests and the float to int conversions that need no constant pool.

use crate::layout::words::{CANONICAL_NAN, FIRST_TAG_FIELD, INT_TAG, TAG_SHIFT};

pub(crate) const FIRST_TAG: u64 = FIRST_TAG_FIELD << TAG_SHIFT;
pub(crate) const TAG_SPAN: u64 = 6 << TAG_SHIFT;
pub(crate) const HIGH_SHIFT: u32 = 32;
pub(crate) const INT_HIGH: u64 = INT_TAG >> HIGH_SHIFT;
/// Byte offset of the upper half of a tagged word.
pub(crate) const HIGH_HALF_AT: usize = 4;
/// The holes of the first and second operand.
pub(crate) const HOLE_A: u8 = 1;
pub(crate) const HOLE_B: u8 = 2;

unsafe extern "C" {
    pub(crate) static HOLE_V0: u8;
    static HOLE_V1: u8;
    static HOLE_V2: u8;
}

/// The frame, the context and the pinned registers: ints and doubles.
#[derive(Clone, Copy)]
pub(crate) struct S {
    pub(crate) frame: usize,
    pub(crate) ctx: usize,
    pub(crate) p: [usize; 4],
    pub(crate) f: [f64; 8],
}

pub(crate) enum Out {
    Next(S),
    Taken(S),
    Exit(S),
}

/// The value of hole `H` as a 32-bit field or a displacement.
#[inline(always)]
pub(crate) fn hole<const H: u8>() -> usize {
    match H {
        0 => &raw const HOLE_V0 as usize,
        1 => &raw const HOLE_V1 as usize,
        _ => &raw const HOLE_V2 as usize,
    }
}

/// The value of hole `H` as a 64-bit constant (a tagged word).
#[inline(always)]
pub(crate) fn hole64<const H: u8>() -> u64 {
    let v: u64;
    // SAFETY: `movabs` of a symbol's address into a register: an R_X86_64_64 relocation.
    unsafe {
        match H {
            0 => core::arch::asm!("movabs {v}, offset {h}", v = out(reg) v, h = sym HOLE_V0, options(pure, nomem, nostack, preserves_flags)),
            1 => core::arch::asm!("movabs {v}, offset {h}", v = out(reg) v, h = sym HOLE_V1, options(pure, nomem, nostack, preserves_flags)),
            _ => core::arch::asm!("movabs {v}, offset {h}", v = out(reg) v, h = sym HOLE_V2, options(pure, nomem, nostack, preserves_flags)),
        }
    }
    v
}

#[inline(always)]
pub(crate) fn slot<const H: u8>(s: &S) -> usize {
    s.frame.wrapping_add(hole::<H>())
}

#[inline(always)]
pub(crate) fn is_int(x: u64) -> bool {
    x >> TAG_SHIFT == INT_TAG >> TAG_SHIFT
}

#[inline(always)]
pub(crate) fn is_double(x: u64) -> bool {
    x.wrapping_sub(FIRST_TAG) >= TAG_SPAN
}

#[inline(always)]
pub(crate) fn int_word(v: i32) -> u64 {
    u64::from(v as u32) | INT_TAG
}

/// `cvttsd2si` to 32 bits: the truncated int32, or `i32::MIN` where `f` is out of range or NaN
/// (Rust's saturating `as` would load its bounds from a constant pool, which a stencil may not use).
#[inline(always)]
pub(crate) fn truncate32(f: f64) -> i32 {
    // SAFETY: SSE2 is the x86-64 baseline.
    unsafe { core::arch::x86_64::_mm_cvttsd_si32(core::arch::x86_64::_mm_set_sd(f)) }
}

/// `cvttsd2si` to 64 bits: the truncated integer, or `i64::MIN` out of range or for NaN.
#[inline(always)]
pub(crate) fn truncate64(f: f64) -> i64 {
    // SAFETY: SSE2 is the x86-64 baseline.
    unsafe { core::arch::x86_64::_mm_cvttsd_si64(core::arch::x86_64::_mm_set_sd(f)) }
}

/// `Value::number(f)`.
#[inline(always)]
pub(crate) fn canonical(f: f64) -> u64 {
    let i = truncate32(f);
    let back = i as f64;
    if back.to_bits() == f.to_bits() {
        int_word(i)
    } else if f.is_nan() {
        CANONICAL_NAN
    } else {
        f.to_bits()
    }
}

#[inline(always)]
pub(crate) fn to_f64(x: u64) -> Option<f64> {
    if is_int(x) {
        Some(x as u32 as i32 as f64)
    } else if is_double(x) {
        Some(f64::from_bits(x))
    } else {
        None
    }
}

#[inline(always)]
pub(crate) fn f64_to_i32(f: f64) -> Option<i32> {
    let wide = truncate64(f);
    if wide != i64::MIN { Some(wide as i32) } else { None }
}

#[inline(always)]
pub(crate) fn to_i32(x: u64) -> Option<i32> {
    if is_int(x) {
        Some(x as u32 as i32)
    } else if is_double(x) {
        f64_to_i32(f64::from_bits(x))
    } else {
        None
    }
}

#[inline(always)]
pub(crate) fn unsigned_word(v: i32) -> u64 {
    if v >= 0 { int_word(v) } else { (v as u32 as f64).to_bits() }
}
