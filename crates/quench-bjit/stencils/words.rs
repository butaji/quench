//! The words and registers the rows compute on: the state (`S`), a row's outcome (`Out`), the
//! patched holes, tagged-word tests and the float to int conversions that need no constant pool.
//! Value representation only: what a Number operation computes is `staged::numeric`'s.

use crate::layout::shared_words::language;
use crate::layout::words::{CANONICAL_NAN, DIRECT_ARRAY_TAG, DIRECT_FUNCTION_TAG, DIRECT_ITERATOR_TAG, DIRECT_OBJECT_TAG, DIRECT_PROMISE_TAG, FALSE, FIRST_TAG_FIELD, HEAP_FAMILY_MASK, INT_TAG, NULL, TAG_SHIFT, TRUE, UNDEFINED};

pub(crate) const FIRST_TAG: u64 = FIRST_TAG_FIELD << TAG_SHIFT;
pub(crate) const TAG_SPAN: u64 = 6 << TAG_SHIFT;
pub(crate) const HIGH_SHIFT: u32 = 32;
pub(crate) const INT_HIGH: u64 = INT_TAG >> HIGH_SHIFT;
/// Byte offset of the upper half of a tagged word.
pub(crate) const HIGH_HALF_AT: usize = 4;
/// The last value hole a row may name (`quench_bjit::HOLES` - 1).
pub(crate) const LAST_HOLE: u8 = 4;
/// The holes of the first and second operand.
pub(crate) const HOLE_A: u8 = 1;
pub(crate) const HOLE_B: u8 = 2;
/// The value holes after the operands' (`HOLE_V0`..`HOLE_V4` are 0..=`LAST_HOLE`).
pub(crate) const HOLE_THIRD: u8 = 2;
pub(crate) const HOLE_FOURTH: u8 = 3;

unsafe extern "C" {
    pub(crate) static HOLE_V0: u8;
    static HOLE_V1: u8;
    static HOLE_V2: u8;
    static HOLE_V3: u8;
    static HOLE_V4: u8;
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

/// A 64-bit value hole as the form the target patches (the relocation `build.rs` reads):
/// x86-64 a `movabs` immediate (`R_X86_64_64`); AArch64 ELF four `movz`/`movk` groups
/// (`R_AARCH64_MOVW_UABS_G0_NC`..`G3`); AArch64 Mach-O, which has no `MOVW` relocation, a literal
/// word the code loads and branches over (`ARM64_RELOC_UNSIGNED`).
#[cfg(target_arch = "x86_64")]
macro_rules! hole_word {
    ($sym:ident) => {{
        let v: u64;
        core::arch::asm!("movabs {v}, offset {h}", v = out(reg) v, h = sym $sym, options(pure, nomem, nostack, preserves_flags));
        v
    }};
}
#[cfg(all(target_arch = "aarch64", not(target_vendor = "apple")))]
macro_rules! hole_word {
    ($sym:ident) => {{
        let v: u64;
        core::arch::asm!(
            "movz {v}, #:abs_g3:{h}",
            "movk {v}, #:abs_g2_nc:{h}",
            "movk {v}, #:abs_g1_nc:{h}",
            "movk {v}, #:abs_g0_nc:{h}",
            v = out(reg) v, h = sym $sym, options(pure, nomem, nostack, preserves_flags)
        );
        v
    }};
}
#[cfg(all(target_arch = "aarch64", target_vendor = "apple"))]
macro_rules! hole_word {
    ($sym:ident) => {{
        let v: u64;
        core::arch::asm!(
            "ldr {v}, 2f",
            "b 3f",
            "2:",
            ".quad {h}",
            "3:",
            v = out(reg) v, h = sym $sym, options(pure, nomem, nostack, preserves_flags)
        );
        v
    }};
}

/// The value of hole `H` as a 64-bit constant (a tagged word).
#[inline(always)]
pub(crate) fn hole64<const H: u8>() -> u64 {
    const { assert!(H <= LAST_HOLE) };
    // SAFETY: the template only materializes a symbol's value in a register.
    unsafe {
        match H {
            0 => hole_word!(HOLE_V0),
            1 => hole_word!(HOLE_V1),
            HOLE_THIRD => hole_word!(HOLE_V2),
            HOLE_FOURTH => hole_word!(HOLE_V3),
            _ => hole_word!(HOLE_V4),
        }
    }
}

/// The value of hole `H` as a 32-bit field or a displacement: on x86-64 the address of the hole
/// symbol itself (a 32-bit relocation LLVM folds into the using instruction), on AArch64 the
/// 64-bit hole (an address would be `adrp`/`add`, which no patch can fill).
#[cfg(target_arch = "x86_64")]
#[inline(always)]
pub(crate) fn hole<const H: u8>() -> usize {
    const { assert!(H <= LAST_HOLE) };
    match H {
        0 => &raw const HOLE_V0 as usize,
        1 => &raw const HOLE_V1 as usize,
        HOLE_THIRD => &raw const HOLE_V2 as usize,
        HOLE_FOURTH => &raw const HOLE_V3 as usize,
        _ => &raw const HOLE_V4 as usize,
    }
}
#[cfg(target_arch = "aarch64")]
#[inline(always)]
pub(crate) fn hole<const H: u8>() -> usize {
    hole64::<H>() as usize
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
#[cfg(target_arch = "x86_64")]
#[inline(always)]
pub(crate) fn truncate32(f: f64) -> i32 {
    // SAFETY: SSE2 is the x86-64 baseline.
    unsafe { core::arch::x86_64::_mm_cvttsd_si32(core::arch::x86_64::_mm_set_sd(f)) }
}

/// `fcvtzs` to 32 bits, the row's conversion on AArch64: NaN takes `i32::MIN` and a value out of
/// range saturates, which the int32 rows' back-conversion rejects exactly as `cvttsd2si`'s
/// out-of-range result does. Written in assembly because a stencil may use no constant pool.
#[cfg(target_arch = "aarch64")]
#[inline(always)]
pub(crate) fn truncate32(f: f64) -> i32 {
    let out: i32;
    // SAFETY: one scalar conversion, no memory or flags touched.
    unsafe { core::arch::asm!("fcvtzs {out:w}, {input:d}", out = out(reg) out, input = in(vreg) f) };
    out
}

/// `cvttsd2si` to 64 bits: the truncated integer, or `i64::MIN` out of range or for NaN.
#[cfg(target_arch = "x86_64")]
#[inline(always)]
pub(crate) fn truncate64(f: f64) -> i64 {
    // SAFETY: SSE2 is the x86-64 baseline.
    unsafe { core::arch::x86_64::_mm_cvttsd_si64(core::arch::x86_64::_mm_set_sd(f)) }
}

/// `fcvtzs` to 64 bits, the 64-bit rows' conversion on AArch64 (`truncate32`'s contract).
#[cfg(target_arch = "aarch64")]
#[inline(always)]
pub(crate) fn truncate64(f: f64) -> i64 {
    let out: i64;
    // SAFETY: one scalar conversion, no memory or flags touched.
    unsafe { core::arch::asm!("fcvtzs {out:x}, {input:d}", out = out(reg) out, input = in(vreg) f) };
    out
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

/// The language bits (`shared_words::language`) a tagged word's tag alone sets: a Number (an int32 or a
/// double), a Boolean, `null`, `undefined` (the deleted marker is none), a direct object family record
/// (`Object`, a direct function included) and a direct function (`Function`). A heap value no tag decides
/// (a string, a symbol, a BigInt, a legacy slot) sets none, so its mask test fails.
#[inline(always)]
pub(crate) fn tag_languages(word: u64) -> u64 {
    let family = word & HEAP_FAMILY_MASK;
    let object = [DIRECT_OBJECT_TAG, DIRECT_ARRAY_TAG, DIRECT_FUNCTION_TAG, DIRECT_PROMISE_TAG, DIRECT_ITERATOR_TAG].contains(&family);
    let classes = [
        (is_int(word) || is_double(word), language::NUMBER),
        (word & !(TRUE ^ FALSE) == FALSE, language::BOOLEAN),
        (word == NULL, language::NULL),
        (word == UNDEFINED, language::UNDEFINED),
        (object, language::OBJECT),
        (family == DIRECT_OBJECT_TAG, language::OBJECT_PLAIN),
        (family == DIRECT_ARRAY_TAG, language::ARRAY),
        (family == DIRECT_FUNCTION_TAG, language::FUNCTION),
    ];
    classes.iter().fold(0, |languages, &(hit, bit)| if hit { languages | u64::from(bit) } else { languages })
}

#[inline(always)]
pub(crate) fn f64_to_i32(f: f64) -> Option<i32> {
    let wide = truncate64(f);
    if wide != i64::MIN { Some(wide as i32) } else { None }
}

/// Whether both tagged words are int words (one test of their upper halves).
#[inline(always)]
pub(crate) fn both_int(x: u64, y: u64) -> bool {
    ((x ^ INT_TAG) | (y ^ INT_TAG)) >> HIGH_SHIFT == 0
}

/// Whether the tagged word's upper half is the int tag (an int word).
#[inline(always)]
pub(crate) fn has_int_high(x: u64) -> bool {
    x >> HIGH_SHIFT == INT_HIGH
}
