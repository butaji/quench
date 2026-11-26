//! The words and registers the rows compute on: the state (`S`), a row's outcome (`Out`), the
//! patched holes, tagged-word tests and the float to int conversions that need no constant pool.
//! Value representation only: what a Number operation computes is `staged::numeric`'s.

use crate::layout::words::{CANONICAL_NAN, FIRST_TAG_FIELD, INT_TAG, TAG_FIELD_SPAN, TAG_SHIFT};
use crate::layout::CALL_CONTEXT_ADAPTER_PAYLOAD;

pub(crate) const FIRST_TAG: u64 = FIRST_TAG_FIELD << TAG_SHIFT;
/// The tag fields are `TAG_FIELD_SPAN + 1` values from the first tag's field on.
pub(crate) const TAG_SPAN: u64 = (TAG_FIELD_SPAN + 1) << TAG_SHIFT;
pub(crate) const HIGH_SHIFT: u32 = 32;
pub(crate) const INT_HIGH: u64 = INT_TAG >> HIGH_SHIFT;
/// Byte offset of the upper half of a tagged word.
pub(crate) const HIGH_HALF_AT: usize = 4;
/// The last value hole the Wasm SIMD rows name for their shuffle bytes (holes 4 and 5 hold the
/// sixteen indices as two 64-bit words).
pub(crate) const LAST_HOLE: u8 = 4;
/// The last value hole a row may name (`quench_bjit::HOLES` - 1): the a-jit rows' sites number
/// past the Wasm shuffle's pair (`LAST_HOLE`).
pub(crate) const MAX_HOLE: u8 = 7;
/// The holes of the first and second operand.
pub(crate) const HOLE_A: u8 = 1;
pub(crate) const HOLE_B: u8 = 2;
/// The value holes after the operands' (`HOLE_V0`..`HOLE_V7` are 0..=`MAX_HOLE`).
pub(crate) const HOLE_THIRD: u8 = 2;
pub(crate) const HOLE_FOURTH: u8 = 3;
pub(crate) const HOLE_FIFTH: u8 = 4;
pub(crate) const HOLE_SIXTH: u8 = 5;
pub(crate) const HOLE_SEVENTH: u8 = 6;

unsafe extern "C" {
    pub(crate) static HOLE_V0: u8;
    static HOLE_V1: u8;
    static HOLE_V2: u8;
    static HOLE_V3: u8;
    static HOLE_V4: u8;
    static HOLE_V5: u8;
    static HOLE_V6: u8;
    static HOLE_V7: u8;
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
    const { assert!(H <= MAX_HOLE) };
    // SAFETY: the template only materializes a symbol's value in a register.
    unsafe {
        match H {
            0 => hole_word!(HOLE_V0),
            1 => hole_word!(HOLE_V1),
            HOLE_THIRD => hole_word!(HOLE_V2),
            HOLE_FOURTH => hole_word!(HOLE_V3),
            HOLE_FIFTH => hole_word!(HOLE_V4),
            HOLE_SIXTH => hole_word!(HOLE_V5),
            HOLE_SEVENTH => hole_word!(HOLE_V6),
            _ => hole_word!(HOLE_V7),
        }
    }
}

/// The value of hole `H` as a 32-bit field or a displacement: on x86-64 the address of the hole
/// symbol itself (a 32-bit relocation LLVM folds into the using instruction), on AArch64 the
/// 64-bit hole (an address would be `adrp`/`add`, which no patch can fill).
#[cfg(target_arch = "x86_64")]
#[inline(always)]
pub(crate) fn hole<const H: u8>() -> usize {
    const { assert!(H <= MAX_HOLE) };
    match H {
        0 => &raw const HOLE_V0 as usize,
        1 => &raw const HOLE_V1 as usize,
        HOLE_THIRD => &raw const HOLE_V2 as usize,
        HOLE_FOURTH => &raw const HOLE_V3 as usize,
        HOLE_FIFTH => &raw const HOLE_V4 as usize,
        HOLE_SIXTH => &raw const HOLE_V5 as usize,
        HOLE_SEVENTH => &raw const HOLE_V6 as usize,
        _ => &raw const HOLE_V7 as usize,
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

/// The state a successor receives where nothing is live: the frame and context, pinned
/// registers junk (no successor reads them).
#[inline(always)]
pub(crate) fn dead(s: &S) -> S {
    S { frame: s.frame, ctx: s.ctx, p: [junk(), junk(), junk(), junk()], f: [junk_f64(), junk_f64(), junk_f64(), junk_f64(), junk_f64(), junk_f64(), junk_f64(), junk_f64()] }
}

/// A word no successor reads (every pinned register is dead past a call): an empty assembly
/// output, so the row writes nothing into the register, where zeroing it cost an instruction
/// per register (12 per call: 4 integer `xor`, 8 `vxorps`). Not `pure`: LLVM would merge the
/// calls into one value and copy it into every register.
#[inline(always)]
pub(crate) fn junk() -> usize {
    let word: usize;
    // SAFETY: an empty template; the register's prior contents are the value.
    unsafe { core::arch::asm!("/* {0} */", out(reg) word, options(nomem, nostack, preserves_flags)) };
    word
}

/// `junk` for a pinned double.
#[inline(always)]
pub(crate) fn junk_f64() -> f64 {
    let word: f64;
    #[cfg(target_arch = "x86_64")]
    // SAFETY: as `junk`.
    unsafe { core::arch::asm!("/* {0} */", out(xmm_reg) word, options(nomem, nostack, preserves_flags)) };
    #[cfg(target_arch = "aarch64")]
    // SAFETY: as `junk`.
    unsafe { core::arch::asm!("/* {0} */", out(vreg) word, options(nomem, nostack, preserves_flags)) };
    word
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

/// `bits`, held in an integer register the optimizer cannot see through: a Number constant made from it reaches its
/// use as a move from that register (`movabs` + `movq`, `mov` + `fmov`), never as a constant-pool load, which a
/// stencil may not make (`Concrete::f64_constant`, so a staged Number rule's constants build into a row).
#[inline(always)]
pub(crate) fn register_bits(mut bits: u64) -> u64 {
    // SAFETY: a template of only a comment naming the register, which keeps its value.
    unsafe { core::arch::asm!("/* {0} */", inout(reg) bits, options(pure, nomem, nostack, preserves_flags)) };
    bits
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


/// ToInt32 of `f` when its truncation is exact (`value::exact_uint32`'s test), else `None`.
#[inline(always)]
pub(crate) fn f64_to_i32(f: f64) -> Option<i32> {
    crate::value::exact_uint32(f).map(|uint32| uint32 as i32)
}

/// Whether the tagged word's upper half is the int tag (an int word).
#[inline(always)]
pub(crate) fn has_int_high(x: u64) -> bool {
    x >> HIGH_SHIFT == INT_HIGH
}

/// The machine word at `base + offset`, a layout offset of a live record or of the dispatch payload.
#[inline(always)]
pub(crate) fn word_at(base: usize, offset: usize) -> u64 {
    // SAFETY: a layout offset of a live record, as every staged load.
    unsafe { ((base + offset) as *const u64).read() }
}

/// The activation's dispatch payload: the context's adapter pointer.
#[inline(always)]
pub(crate) fn payload(s: &S) -> usize {
    word_at(s.ctx, CALL_CONTEXT_ADAPTER_PAYLOAD) as usize
}

/// The header word `index` (a `staged::frame` `CONTEXT_*`) of the window at `frame`: the context words
/// count down from its register 0, so a window's receiver and captures word are its own.
#[inline(always)]
pub(crate) fn context_word(frame: usize, index: usize) -> u64 {
    // SAFETY: a window's context words lie below its registers, as every frame's header does.
    unsafe { ((frame as *const u64).sub(index)).read() }
}

/// Writes the header word `index` of the window at `frame` (`context_word`'s pair).
#[inline(always)]
pub(crate) fn set_context_word(frame: usize, index: usize, word: u64) {
    // SAFETY: as `context_word`; the caller owns the window it writes a callee's header of.
    unsafe { ((frame as *mut u64).sub(index)).write(word) }
}

/// A row's hit continues at TAKEN, past the helper step laid out after it where it is a fast arm; a miss
/// leaves as it did.
#[inline(always)]
pub(crate) fn hop(out: Out) -> Out {
    match out {
        Out::Next(s) => Out::Taken(s),
        left => left,
    }
}

/// The row that leaves through EXIT0 with every register as it came, writing nothing: the operation only the
/// interpreter runs, which the helper step laid out after the row then runs.
#[inline(always)]
pub(crate) fn exit_here(s: S) -> Out {
    Out::Exit(s)
}
