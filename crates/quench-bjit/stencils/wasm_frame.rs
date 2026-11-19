//! The Wasm rows' frame plumbing, in one place: how a row reads its holes and frame words, how it
//! leaves, and the operand codes it decodes. No Wasm semantics live here; those are the staged
//! rules (`crate::staged::wasm*`) the rows call, and the stage primitives are `wasm_stage.rs`.
//! Not a rows file: every fn here is an accessor, shared by all `wasm_rows*.rs`.
//!
//! The decoders that read a stencil code (`W32`, `STORAGE_BITS32`, `ACCESSES`) live here, not in
//! `staged`, which cannot name `wasm_kinds.rs`; the tag decoders of the runtime's operators are the
//! staged ones (`staged::wasm::binary_operator` and kin).

use super::wasm_stage::StencilMemory;
use crate::staged::wasm::Width;
use crate::staged::wasm_float::Float;
use crate::staged::wasm_instance::Storage;
use crate::staged::wasm_memory::Access;
use crate::wasm::WasmType;
use crate::WasmTrap;
use crate::words::{HOLE_B, HOLE_FOURTH, Out, S, hole64, slot};

/// The low 32 bits of a packed hole: the first of two 32-bit values in one word.
#[inline(always)]
pub(crate) const fn low(word: u64) -> u32 {
    word as u32
}

/// The high 32 bits of a packed hole: the second of two 32-bit values in one word.
#[inline(always)]
pub(crate) const fn high(word: u64) -> u32 {
    (word >> 32) as u32
}

/// The integer width a `W` code names: `W32` is i32, any other i64.
#[inline(always)]
pub(crate) const fn width_of(width: u8) -> Width {
    if width == super::W32 { Width::I32 } else { Width::I64 }
}

/// The float width a `W` code names: `W32` is f32, any other f64.
#[inline(always)]
pub(crate) const fn float_of(width: u8) -> Float {
    if width == super::W32 { Float::F32 } else { Float::F64 }
}

/// The storage a global's operator code names.
#[inline(always)]
pub(crate) const fn storage_of(op: u32) -> Storage {
    if op == super::STORAGE_BITS32 { Storage::Bits32 } else { Storage::Bits64 }
}

/// The access the operator code `index` names (`ACCESSES`), an out-of-range index naming the first.
pub(crate) const fn access(index: u32) -> Access {
    let (ty, bytes, signed) = super::ACCESSES[if (index as usize) < super::ACCESSES.len() { index as usize } else { 0 }];
    let ty = match ty {
        super::T_I32 => WasmType::I32,
        super::T_I64 => WasmType::I64,
        super::T_F32 => WasmType::F32,
        _ => WasmType::F64,
    };
    Access { ty, bytes, signed }
}

/// The word of hole `H`: a register's frame word, or a hole's immediate.
#[inline(always)]
pub(crate) unsafe fn read<const H: u8>(s: &S) -> u64 {
    unsafe { *(slot::<H>(s) as *const u64) }
}

/// Writes `word` to hole `H`'s register.
#[inline(always)]
pub(crate) unsafe fn write<const H: u8>(s: &S, word: u64) {
    unsafe { *(slot::<H>(s) as *mut u64) = word }
}

/// The second operand: a register's word, or the immediate the row's hole holds.
#[inline(always)]
pub(crate) unsafe fn right<const SHAPE: u8>(s: &S) -> u64 {
    if SHAPE == super::IMMEDIATE { hole64::<HOLE_B>() } else { unsafe { read::<HOLE_B>(s) } }
}

/// Leaves with `trap`'s status.
#[inline(always)]
pub(crate) fn trap(mut s: S, trap: WasmTrap) -> Out {
    s.p[0] = match trap.compiled_status() {
        Some(status) => status as usize,
        None => 0,
    };
    Out::Exit(s)
}

/// The linear memory of the frame: its base in the second pin, its length in the third, as the
/// load and store rows read them (`reload_memory` keeps them current after a call).
#[inline(always)]
pub(crate) fn memory_of(s: &S) -> StencilMemory {
    StencilMemory { base: s.p[1] as *mut u8, length: s.p[2] as u64 }
}

/// The context's global area: the context plus hole FOURTH's offset of `globals` (`wasm_rows_global.rs`).
#[inline(always)]
pub(crate) fn global_area(s: &S) -> usize {
    s.ctx.wrapping_add(hole64::<HOLE_FOURTH>() as usize)
}

/// After a call (or a VM callout) that may have grown the memory: the frame's length word takes the
/// context's current length, read at `length_at`, since a callee's growth refreshes the context and
/// not this frame's header. The base is reloaded from the header; it never moves under a call
/// (`World::memory_stable` admits calls only where it cannot), and the pins are reloaded from the
/// header afterwards.
#[inline(always)]
pub(crate) unsafe fn reload_memory(s: &mut S, length_at: usize) {
    unsafe {
        let length = *((s.ctx + length_at) as *const u64) as usize;
        *(s.frame as *mut usize).add(super::FRAME_LENGTH_WORD) = length;
    }
    unsafe { restore_memory(s) }
}

/// After a call that cannot have grown the memory (`CALL_KEEP`): the base and length pins are the
/// frame header's, which hold the current length (the context's, by every growing call's reload).
/// The rest is as `reload_memory` leaves it.
#[inline(always)]
pub(crate) unsafe fn restore_memory(s: &mut S) {
    unsafe {
        let header = s.frame as *const usize;
        s.p[1] = *header.add(super::FRAME_BASE_WORD);
        s.p[2] = *header.add(super::FRAME_LENGTH_WORD);
    }
    // Wasm rows keep nothing in the other pins (the status pin is written only on a leave), so
    // a call row hands them on as constants: the call need not save them around itself.
    s.p[0] = 0;
    s.p[3] = 0;
    s.f = [0.0; 8];
}

/// The vector in the two frame words at hole `H`'s offset, low word first (a v128 register's
/// layout). Two word reads, never a 16-byte copy.
#[inline(always)]
pub(crate) unsafe fn vector_read<const H: u8>(s: &S) -> u128 {
    unsafe {
        let at = slot::<H>(s) as *const u64;
        u128::from(*at) | (u128::from(*at.add(1)) << u64::BITS)
    }
}

/// Writes `bits` to the two frame words at hole `H`'s offset, low word first.
#[inline(always)]
pub(crate) unsafe fn vector_write<const H: u8>(s: &S, bits: u128) {
    unsafe {
        let at = slot::<H>(s) as *mut u64;
        *at = bits as u64;
        *at.add(1) = (bits >> u64::BITS) as u64;
    }
}

/// The word at frame offset `offset`, or 0 where the operand is absent (`CALLOUT_ABSENT`).
#[inline(always)]
pub(crate) unsafe fn operand(frame: usize, offset: u32) -> u64 {
    if offset == crate::wasm_layout::CALLOUT_ABSENT { 0 } else { unsafe { *((frame + offset as usize) as *const u64) } }
}

/// The `NativeRoots` the context names at byte offset `at` of `CallContext` (`native_roots`).
#[inline(always)]
pub(crate) unsafe fn native_roots(context: usize, at: usize) -> *mut u8 {
    unsafe { *(context.wrapping_add(at) as *const usize) as *mut u8 }
}
