//! The Number rows over tagged `Value` words, written once as Rust: the semantics of
//! `quench-bjit/src/numeric` (int32 speculation, general Number rows, doubles in xmm registers,
//! pinned int32 registers), compiled by LLVM into one stencil per (kind, destination, operands,
//! speculate) combination. The place of an operand is a const generic, so a pinned register is a
//! named argument register and a frame slot a displacement from the frame register patched by
//! hole 0 (destination), 1 and 2 (operands).
//!
//! A row computes into `Out`: continue at the next row, branch to the taken successor, or leave
//! through exit 0 with the registers as they were at the row's start (every guard comes before
//! any write to a pinned register).

use crate::layout::words::{CANONICAL_NAN, FIRST_TAG_FIELD, INT_TAG, TAG_SHIFT};

pub(crate) const MEM: u8 = 0;
pub(crate) const MEMINT: u8 = 1;
pub(crate) const CONST: u8 = 2;
pub(crate) const IMM: u8 = 3;
pub(crate) const PIN: u8 = 4;
pub(crate) const PINF: u8 = 8;

pub(crate) const MOVE: u8 = 0;
pub(crate) const COPY: u8 = 1;
pub(crate) const ADD: u8 = 2;
pub(crate) const SUB: u8 = 3;
pub(crate) const MUL: u8 = 4;
pub(crate) const DIV: u8 = 5;
pub(crate) const AND: u8 = 6;
pub(crate) const OR: u8 = 7;
pub(crate) const XOR: u8 = 8;
pub(crate) const SHL: u8 = 9;
pub(crate) const SHR: u8 = 10;
pub(crate) const USHR: u8 = 11;
pub(crate) const LT: u8 = 12;
pub(crate) const LE: u8 = 13;
pub(crate) const GT: u8 = 14;
pub(crate) const GE: u8 = 15;
pub(crate) const EQ: u8 = 16;
pub(crate) const NE: u8 = 17;
pub(crate) const JUMP: u8 = 18;
pub(crate) const JUMP_POLL: u8 = 19;
pub(crate) const GUARD: u8 = 20;
pub(crate) const TAG_INIT: u8 = 21;
pub(crate) const SPILL: u8 = 22;

const FIRST_TAG: u64 = FIRST_TAG_FIELD << TAG_SHIFT;
const TAG_SPAN: u64 = 6 << TAG_SHIFT;
const INT_HIGH: u64 = INT_TAG >> 32;

unsafe extern "C" {
    static HOLE_V0: u8;
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

macro_rules! need {
    ($s:expr, $e:expr) => {
        match $e {
            Some(v) => v,
            None => return Out::Exit($s),
        }
    };
}

/// The value of hole `H` as a 32-bit field or a displacement.
#[inline(always)]
fn hole<const H: u8>() -> usize {
    match H {
        0 => &raw const HOLE_V0 as usize,
        1 => &raw const HOLE_V1 as usize,
        _ => &raw const HOLE_V2 as usize,
    }
}

/// The value of hole `H` as a 64-bit constant (a tagged word).
#[inline(always)]
fn hole64<const H: u8>() -> u64 {
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
fn slot<const H: u8>(s: &S) -> usize {
    s.frame.wrapping_add(hole::<H>())
}

#[inline(always)]
fn is_int(x: u64) -> bool {
    x >> TAG_SHIFT == INT_TAG >> TAG_SHIFT
}

#[inline(always)]
fn is_double(x: u64) -> bool {
    x.wrapping_sub(FIRST_TAG) >= TAG_SPAN
}

#[inline(always)]
fn int_word(v: i32) -> u64 {
    u64::from(v as u32) | INT_TAG
}

/// `cvttsd2si` to 32 bits: the truncated int32, or `i32::MIN` where `f` is out of range or NaN
/// (Rust's saturating `as` would load its bounds from a constant pool, which a stencil may not use).
#[inline(always)]
fn truncate32(f: f64) -> i32 {
    // SAFETY: SSE2 is the x86-64 baseline.
    unsafe { core::arch::x86_64::_mm_cvttsd_si32(core::arch::x86_64::_mm_set_sd(f)) }
}

/// `cvttsd2si` to 64 bits: the truncated integer, or `i64::MIN` out of range or for NaN.
#[inline(always)]
fn truncate64(f: f64) -> i64 {
    // SAFETY: SSE2 is the x86-64 baseline.
    unsafe { core::arch::x86_64::_mm_cvttsd_si64(core::arch::x86_64::_mm_set_sd(f)) }
}

/// `Value::number(f)`.
#[inline(always)]
fn canonical(f: f64) -> u64 {
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
fn to_f64(x: u64) -> Option<f64> {
    if is_int(x) {
        Some(x as u32 as i32 as f64)
    } else if is_double(x) {
        Some(f64::from_bits(x))
    } else {
        None
    }
}

#[inline(always)]
fn f64_to_i32(f: f64) -> Option<i32> {
    let wide = truncate64(f);
    if wide != i64::MIN { Some(wide as i32) } else { None }
}

#[inline(always)]
fn to_i32(x: u64) -> Option<i32> {
    if is_int(x) {
        Some(x as u32 as i32)
    } else if is_double(x) {
        f64_to_i32(f64::from_bits(x))
    } else {
        None
    }
}

#[inline(always)]
fn unsigned_word(v: i32) -> u64 {
    if v >= 0 { int_word(v) } else { (v as u32 as f64).to_bits() }
}

/// The tagged word of an operand.
#[inline(always)]
fn word_of<const P: u8, const H: u8>(s: &S) -> u64 {
    match P {
        MEM | MEMINT => unsafe { (slot::<H>(s) as *const u64).read() },
        CONST => hole64::<H>(),
        IMM => int_word(hole::<H>() as u32 as i32),
        PIN..=7 => int_word(s.p[(P - PIN) as usize] as u32 as i32),
        _ => canonical(s.f[(P - PINF) as usize]),
    }
}

/// The int32 of an operand; `None` where a frame slot or constant is no int word.
#[inline(always)]
fn int_of<const P: u8, const H: u8>(s: &S) -> Option<i32> {
    match P {
        PIN..=7 => Some(s.p[(P - PIN) as usize] as u32 as i32),
        IMM => Some(hole::<H>() as u32 as i32),
        MEMINT => Some(unsafe { (slot::<H>(s) as *const u32).read() } as i32),
        PINF.. => f64_to_i32(s.f[(P - PINF) as usize]),
        _ => {
            let word = word_of::<P, H>(s);
            if word >> 32 == INT_HIGH { Some(word as u32 as i32) } else { None }
        }
    }
}

/// An operand as `f64`; `None` where a frame slot is no Number.
#[inline(always)]
fn f64_of<const P: u8, const H: u8>(s: &S) -> Option<f64> {
    match P {
        PINF.. => Some(s.f[(P - PINF) as usize]),
        PIN..=7 | IMM | MEMINT => int_of::<P, H>(s).map(f64::from),
        CONST => Some(f64::from_bits(word_of::<P, H>(s))),
        _ => to_f64(word_of::<P, H>(s)),
    }
}

#[inline(always)]
fn both_int(x: u64, y: u64) -> bool {
    ((x ^ INT_TAG) | (y ^ INT_TAG)) >> 32 == 0
}

#[inline(always)]
const fn holds_int(p: u8) -> bool {
    matches!(p, PIN..=7 | MEMINT)
}

/// Writes the int32 `v` to the destination and continues.
#[inline(always)]
fn finish_int<const D: u8>(mut s: S, v: i32) -> Out {
    match D {
        PIN..=7 => s.p[(D - PIN) as usize] = v as u32 as usize,
        MEMINT => unsafe { (slot::<0>(&s) as *mut u32).write(v as u32) },
        _ => unsafe { (slot::<0>(&s) as *mut u64).write(int_word(v)) },
    }
    Out::Next(s)
}

/// As `finish_int` for a tagged word (a pinned double takes it as a double).
#[inline(always)]
fn finish_word<const D: u8>(s: S, word: u64) -> Out {
    if D >= PINF {
        return deliver::<D>(s, word);
    }
    unsafe { (slot::<0>(&s) as *mut u64).write(word) };
    Out::Next(s)
}

/// `dst` receives the tagged `word`: a pinned register or int slot an int word, a pinned double
/// a Number, else any word.
#[inline(always)]
pub(crate) fn deliver<const D: u8>(mut s: S, word: u64) -> Out {
    match D {
        PINF.. => {
            let i = (D - PINF) as usize;
            if is_double(word) {
                s.f[i] = f64::from_bits(word);
            } else {
                core::hint::cold_path();
                if !is_int(word) {
                    return Out::Exit(s);
                }
                s.f[i] = f64::from(word as u32 as i32);
            }
        }
        PIN..=7 => {
            if !is_int(word) {
                return Out::Exit(s);
            }
            s.p[(D - PIN) as usize] = word as u32 as usize;
        }
        MEMINT => {
            if !is_int(word) {
                return Out::Exit(s);
            }
            unsafe { (slot::<0>(&s) as *mut u32).write(word as u32) };
        }
        _ => unsafe { (slot::<0>(&s) as *mut u64).write(word) },
    }
    Out::Next(s)
}

/// The int32 an array index operand holds; a double must be integral.
#[inline(always)]
pub(crate) fn index_of<const P: u8, const H: u8>(s: &S) -> Option<i32> {
    if P < PINF {
        return int_of::<P, H>(s);
    }
    let f = s.f[(P - PINF) as usize];
    let narrow = truncate32(f);
    (narrow as f64 == f).then_some(narrow)
}

/// The tagged word of a Number operand a store writes.
#[inline(always)]
pub(crate) fn number_word<const P: u8, const H: u8>(s: &S) -> Option<u64> {
    let word = word_of::<P, H>(s);
    if P == MEM && !(is_int(word) || is_double(word)) {
        return None;
    }
    Some(word)
}

#[inline(always)]
fn float_arith<const K: u8>(x: f64, y: f64) -> f64 {
    match K {
        ADD => x + y,
        SUB => x - y,
        MUL => x * y,
        _ => x / y,
    }
}

#[inline(always)]
fn relation_int<const K: u8>(x: i32, y: i32) -> bool {
    match K {
        LT => x < y,
        LE => x <= y,
        GT => x > y,
        GE => x >= y,
        EQ => x == y,
        _ => x != y,
    }
}

#[inline(always)]
fn relation_float<const K: u8>(x: f64, y: f64) -> bool {
    match K {
        LT => x < y,
        LE => x <= y,
        GT => x > y,
        GE => x >= y,
        EQ => x == y,
        _ => x != y,
    }
}

#[inline(always)]
fn jump_if(s: S, fails: bool) -> Out {
    if fails { Out::Taken(s) } else { Out::Next(s) }
}

#[inline(always)]
fn checked<const K: u8>(x: i32, y: i32) -> (i32, bool) {
    match K {
        ADD => x.overflowing_add(y),
        SUB => x.overflowing_sub(y),
        _ => x.overflowing_mul(y),
    }
}

#[inline(always)]
fn double_path<const K: u8, const D: u8>(s: S, x: u64, y: u64) -> Out {
    let (fx, fy) = (need!(s, to_f64(x)), need!(s, to_f64(y)));
    finish_word::<D>(s, canonical(float_arith::<K>(fx, fy)))
}

#[inline(always)]
fn arith<const K: u8, const D: u8, const A: u8, const B: u8, const SPEC: bool>(s: S) -> Out {
    if SPEC {
        let (x, y) = (need!(s, int_of::<A, 1>(&s)), need!(s, int_of::<B, 2>(&s)));
        let (value, overflow) = checked::<K>(x, y);
        if overflow || (K == MUL && value == 0 && (x | y) < 0) {
            return Out::Exit(s);
        }
        return finish_int::<D>(s, value);
    }
    let (x, y) = (word_of::<A, 1>(&s), word_of::<B, 2>(&s));
    if K == DIV {
        return double_path::<K, D>(s, x, y);
    }
    let (value, overflow) = checked::<K>(x as u32 as i32, y as u32 as i32);
    let exact = !overflow && (K != MUL || value != 0);
    if both_int(x, y) && exact {
        finish_word::<D>(s, int_word(value))
    } else {
        double_path::<K, D>(s, x, y)
    }
}

#[inline(always)]
fn int_bitwise<const K: u8>(x: i32, y: i32) -> i32 {
    match K {
        AND => x & y,
        OR => x | y,
        XOR => x ^ y,
        SHL => x.wrapping_shl(y as u32),
        SHR => x.wrapping_shr(y as u32),
        _ => (x as u32).wrapping_shr(y as u32) as i32,
    }
}

#[inline(always)]
fn bitwise<const K: u8, const D: u8, const A: u8, const B: u8, const SPEC: bool>(s: S) -> Out {
    if SPEC {
        let (x, y) = (need!(s, int_of::<A, 1>(&s)), need!(s, int_of::<B, 2>(&s)));
        let value = int_bitwise::<K>(x, y);
        if K != USHR {
            return finish_int::<D>(s, value);
        }
        if holds_int(D) {
            if value < 0 {
                return Out::Exit(s);
            }
            return finish_int::<D>(s, value);
        }
        return finish_word::<D>(s, unsigned_word(value));
    }
    let (x, y) = (word_of::<A, 1>(&s), word_of::<B, 2>(&s));
    let (x32, y32) = (need!(s, to_i32(x)), need!(s, to_i32(y)));
    let value = int_bitwise::<K>(x32, y32);
    finish_word::<D>(s, if K == USHR { unsigned_word(value) } else { int_word(value) })
}

#[inline(always)]
fn copy<const D: u8, const A: u8>(s: S) -> Out {
    if holds_int(D) {
        let value = need!(s, int_of::<A, 1>(&s));
        return finish_int::<D>(s, value);
    }
    finish_word::<D>(s, word_of::<A, 1>(&s))
}

#[inline(always)]
fn compare<const K: u8, const A: u8, const B: u8, const SPEC: bool>(s: S) -> Out {
    if SPEC {
        let (x, y) = (need!(s, int_of::<A, 1>(&s)), need!(s, int_of::<B, 2>(&s)));
        return jump_if(s, !relation_int::<K>(x, y));
    }
    let (x, y) = (word_of::<A, 1>(&s), word_of::<B, 2>(&s));
    if both_int(x, y) {
        return jump_if(s, !relation_int::<K>(x as u32 as i32, y as u32 as i32));
    }
    let (fx, fy) = (need!(s, to_f64(x)), need!(s, to_f64(y)));
    jump_if(s, !relation_float::<K>(fx, fy))
}

#[inline(always)]
fn float_row<const K: u8, const D: u8, const A: u8, const B: u8>(mut s: S) -> Out {
    let x = need!(s, f64_of::<A, 1>(&s));
    let value = if K == MOVE || K == COPY {
        x
    } else {
        let y = need!(s, f64_of::<B, 2>(&s));
        if K >= LT {
            return jump_if(s, !relation_float::<K>(x, y));
        }
        float_arith::<K>(x, y)
    };
    if D >= PINF {
        s.f[(D - PINF) as usize] = value;
    } else {
        unsafe { (slot::<0>(&s) as *mut u64).write(canonical(value)) };
    }
    Out::Next(s)
}

/// A taken backward edge counts in the run's counter, word 0 of the loop entry record: hole 1
/// and 2 are the offsets of the context's payload pointer and of the entry pointer in the payload.
#[inline(always)]
fn count_backedge(s: &S) {
    unsafe {
        let payload = ((s.ctx + hole::<1>()) as *const usize).read();
        let entry = ((payload + hole::<2>()) as *const usize).read();
        let count = (entry as *const u64).read();
        (entry as *mut u64).write(count.wrapping_add(1));
    }
}

#[inline(always)]
fn jump<const POLL: bool>(s: S) -> Out {
    if POLL {
        // The interrupt flag: the context's pointer to a byte, nonzero once requested.
        let byte = unsafe { *(((s.ctx + hole::<0>()) as *const *const u8).read()) };
        if byte != 0 {
            return Out::Exit(s);
        }
        count_backedge(&s);
    }
    Out::Taken(s)
}

#[inline(always)]
fn guard<const D: u8, const A: u8>(mut s: S) -> Out {
    let word = word_of::<A, 1>(&s);
    match D {
        MEMINT => jump_if(s, word >> 32 != INT_HIGH),
        PIN..=7 => {
            // Loads the pinned register: an int word, else the entry fails.
            s.p[(D - PIN) as usize] = word as u32 as usize;
            jump_if(s, word >> 32 != INT_HIGH)
        }
        PINF.. => {
            let (int, double) = (is_int(word), is_double(word));
            s.f[(D - PINF) as usize] = if int { f64::from(word as u32 as i32) } else { f64::from_bits(word) };
            jump_if(s, !(int || double))
        }
        _ => jump_if(s, !(is_int(word) || is_double(word))),
    }
}

/// One row, by kind and places. `SPEC` rows take int32 operands only.
#[inline(always)]
pub(crate) fn run<const K: u8, const D: u8, const A: u8, const B: u8, const SPEC: bool>(s: S) -> Out {
    let floats = D >= PINF || A >= PINF || B >= PINF;
    if floats && K <= NE && !(AND..=USHR).contains(&K) {
        return float_row::<K, D, A, B>(s);
    }
    match K {
        MOVE | COPY => copy::<D, A>(s),
        ADD | SUB | MUL | DIV => arith::<K, D, A, B, SPEC>(s),
        AND..=USHR => bitwise::<K, D, A, B, SPEC>(s),
        LT..=NE => compare::<K, A, B, SPEC>(s),
        JUMP => jump::<false>(s),
        JUMP_POLL => jump::<true>(s),
        GUARD => guard::<D, A>(s),
        TAG_INIT => {
            unsafe { (slot::<0>(&s).wrapping_add(4) as *mut u32).write(INT_HIGH as u32) };
            Out::Next(s)
        }
        _ => {
            // SPILL: a pinned double (operand `a`) to the frame slot as a canonical word.
            unsafe { (slot::<0>(&s) as *mut u64).write(word_of::<A, 1>(&s)) };
            Out::Next(s)
        }
    }
}
