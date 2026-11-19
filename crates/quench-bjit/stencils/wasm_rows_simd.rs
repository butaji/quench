//! The b-jit's vector rows over raw frame words (kinds `SIMD_*` in `wasm_kinds.rs`).
//!
//! Representation: a v128 register is two consecutive frame words, low word first (bits 0..64,
//! 64..128 of the vector, the layout `compiled_simd_kernel` reads), in the vector area past the
//! scalar registers (`wasm_stitch_simd::vector_slot`). A row's operand hole names the low word's
//! byte offset; the high word is at +8 (`wasm_frame::vector_read`, `vector_write`). No row boxes a
//! vector, allocates, or calls the VM: a stitched function never hands a v128 across its frame.
//!
//! Semantics are the one operator body the interpreter runs too: `staged::wasm_simd_lanes`, which
//! each row reaches through `SimdOperator::from_tag(OP)` in a const block, so a row's body is the
//! one operator it names (the lookup folds). Shuffles call the same `shuffle` body.
//!
//! Holes (the same meaning in every row): `0` is the destination (`HOLE_V0`), `HOLE_A` and
//! `HOLE_B` the first and second operands, `HOLE_FOURTH` and `LAST_HOLE` the rest. A vector
//! operand's hole is its low word's offset; a scalar operand's hole is its frame word's offset.
//! Row by row, with the OP tag the row is instantiated at:
//! - `SIMD_BINARY`:  D (vector) = A op B (vectors).
//! - `SIMD_UNARY`:   D = op A.
//! - `SIMD_SHUFFLE`: D = shuffle(A, B) by the sixteen byte indices in `HOLE_FOURTH` (bytes 0..8)
//!                   and `LAST_HOLE` (bytes 8..16).
//! - `SIMD_CONST`:   D = the vector whose low word is `HOLE_A` and high word `HOLE_B`.
//! - `SIMD_LOAD`:    D = the 16 bytes at the 64-bit address in `HOLE_A` (the staged rule's bounds).
//! - `SIMD_STORE`:   the 16 bytes at the 64-bit address in `HOLE_A` = D.
//! - `SIMD_SPLAT`:   D = splat(A) (A a scalar word, its low bits).
//! - `SIMD_EXTRACT`: D (scalar word) = lane `HOLE_B` of A.
//! - `SIMD_REPLACE`: D = A with lane `HOLE_FOURTH` replaced by B (a scalar word in `HOLE_B`).
//! - `SIMD_MOVE`:    D = A (a vector).
//!
//! The rows are the library's (`wasm_library.rs`), so each has its symbol `w_<kind>_<op>_1_0`
//! (width code `W64` = 1, shape `REGISTER` = 0) and is dispatched from `wasm_rows::run`. A row that
//! traps leaves through `trap`, as the scalar rows do (the access is the only trap).

use super::wasm_frame::{vector_read, vector_write};
use super::{StencilMemory, StencilSimd, read, trap, write};
use crate::staged::wasm_memory_vector::{load_vector, store_vector};
use crate::staged::wasm_simd_rules::{selector_of, simd_scalar};
use crate::staged::wasm_simd_lanes::{SimdOperator, shuffle as shuffle_bytes};
use crate::words::{HOLE_A, HOLE_B, HOLE_FOURTH, LAST_HOLE, Out, S, hole64};

/// The bits of operator `OP` on the operands: the operator's own body (`SimdOperator::bits`). A tag
/// that names no operator is zero; the const block folds the lookup, so each row keeps one body.
#[inline(always)]
fn operate<const OP: u32>(left: u128, right: u128, lane: u8) -> u128 {
    let operator = const { SimdOperator::from_tag(OP) };
    match operator {
        Some(operator) => operator.bits(left, right, lane),
        None => 0,
    }
}

/// `D = A op B` on vectors, the operator `OP` names.
#[inline(always)]
pub(crate) unsafe fn binary<const OP: u32>(s: S) -> Out {
    unsafe {
        let value = operate::<OP>(vector_read::<HOLE_A>(&s), vector_read::<HOLE_B>(&s), 0);
        vector_write::<0>(&s, value);
    }
    Out::Next(s)
}

/// `D = op A` on a vector, the operator `OP` names.
#[inline(always)]
pub(crate) unsafe fn unary<const OP: u32>(s: S) -> Out {
    unsafe {
        let value = operate::<OP>(vector_read::<HOLE_A>(&s), 0, 0);
        vector_write::<0>(&s, value);
    }
    Out::Next(s)
}

/// `D = shuffle(A, B)` by the sixteen byte indices in holes `HOLE_FOURTH` and `LAST_HOLE`.
#[inline(always)]
pub(crate) unsafe fn shuffle(s: S) -> Out {
    unsafe {
        let (left, right) = (vector_read::<HOLE_A>(&s), vector_read::<HOLE_B>(&s));
        let indices = u128::from(hole64::<HOLE_FOURTH>()) | (u128::from(hole64::<LAST_HOLE>()) << u64::BITS);
        vector_write::<0>(&s, shuffle_bytes(left, right, indices));
    }
    Out::Next(s)
}

/// `D = ` the vector whose low word is hole `HOLE_A` and high word `HOLE_B`.
#[inline(always)]
pub(crate) unsafe fn constant(s: S) -> Out {
    unsafe {
        vector_write::<0>(&s, u128::from(hole64::<HOLE_A>()) | (u128::from(hole64::<HOLE_B>()) << u64::BITS));
    }
    Out::Next(s)
}

/// `D = v128.load` at the 64-bit address in `HOLE_A`; traps out of bounds, writing nothing.
#[inline(always)]
pub(crate) unsafe fn load(s: S) -> Out {
    let index = unsafe { read::<HOLE_A>(&s) };
    let mut memory = StencilMemory::of(&s);
    match load_vector(&mut memory, index, 0, false) {
        Ok(value) => {
            unsafe { vector_write::<0>(&s, value) };
            Out::Next(s)
        }
        Err(error) => trap(s, error),
    }
}

/// `v128.store D` at the 64-bit address in `HOLE_A`; traps out of bounds, writing nothing.
#[inline(always)]
pub(crate) unsafe fn store(s: S) -> Out {
    let index = unsafe { read::<HOLE_A>(&s) };
    let value = unsafe { vector_read::<0>(&s) };
    let mut memory = StencilMemory::of(&s);
    match store_vector(&mut memory, index, 0, value, false) {
        Ok(()) => Out::Next(s),
        Err(error) => trap(s, error),
    }
}

/// `D = splat(A)`, the lane width the operator `OP` names; A's low bits are the lane.
#[inline(always)]
pub(crate) unsafe fn splat<const OP: u32>(s: S) -> Out {
    let scalar = unsafe { read::<HOLE_A>(&s) };
    unsafe { vector_write::<0>(&s, operate::<OP>(u128::from(scalar), 0, 0)) };
    Out::Next(s)
}

/// `D = ` lane `HOLE_B` of A, as the scalar word the operator `OP` names (a signed lane sign-extends
/// to an i32, as the operator does).
#[inline(always)]
pub(crate) unsafe fn extract<const OP: u32>(s: S) -> Out {
    let (left, lane) = unsafe { (vector_read::<HOLE_A>(&s), hole64::<HOLE_B>() as u8) };
    let value = simd_scalar(&mut StencilSimd, selector_of(OP, lane), left, None);
    unsafe { write::<0>(&s, value) };
    Out::Next(s)
}

/// `D = A` with lane `HOLE_FOURTH` replaced by B's low bits, the lane width the operator `OP` names.
#[inline(always)]
pub(crate) unsafe fn replace<const OP: u32>(s: S) -> Out {
    let (value, scalar, index) = unsafe { (vector_read::<HOLE_A>(&s), read::<HOLE_B>(&s), hole64::<HOLE_FOURTH>() as u8) };
    let result = operate::<OP>(value, u128::from(scalar), index);
    unsafe { vector_write::<0>(&s, result) };
    Out::Next(s)
}

/// `D = A`, a vector.
#[inline(always)]
pub(crate) unsafe fn moves(s: S) -> Out {
    unsafe { vector_write::<0>(&s, vector_read::<HOLE_A>(&s)) };
    Out::Next(s)
}
