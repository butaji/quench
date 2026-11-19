//! The b-jit's vector rows for the operators `wasm_rows_simd.rs` does not cover (kinds `SIMD_MORE_*`).
//! Holes and frame layout are `wasm_rows_simd.rs`'s. Semantics are the shared operator body
//! (`staged::wasm_simd_lanes`), reached through `SimdOperator::from_tag(OP)` in a const block, so each
//! row runs its one operator. The vector loads are `memory.rs`'s: an extension or splat of the loaded
//! bytes is the operator it names (`load_operator`), and the zero forms are the bytes themselves.

use super::wasm_frame::{vector_read, vector_write};
use super::{StencilMemory, StencilSimd, read, trap, write};
use crate::staged::wasm_memory_vector::load_lane;
use crate::staged::wasm_simd_rules::{selector_of, simd_scalar};
use crate::staged::wasm_simd_lanes::SimdOperator;
use crate::wasm_codes::*;
use crate::words::{HOLE_A, HOLE_B, Out, S};

/// The bits of operator `OP` on the operands: the operator's own body (`SimdOperator::bits`). A tag
/// that names no operator is zero; the const block folds the lookup.
#[inline(always)]
fn operate<const OP: u32>(left: u128, right: u128, lane: u8) -> u128 {
    let operator = const { SimdOperator::from_tag(OP) };
    match operator {
        Some(operator) => operator.bits(left, right, lane),
        None => 0,
    }
}

/// The bytes a vector load of `OP` reads.
const fn load_bytes(operator: u32) -> usize {
    match operator {
        SIMD_V128_LOAD8_SPLAT => 1,
        SIMD_V128_LOAD16_SPLAT => 2,
        SIMD_V128_LOAD32_SPLAT | SIMD_V128_LOAD32_ZERO => 4,
        _ => 8,
    }
}

/// The operator a vector load of `OP` extends or splats its bytes with, when one does (`memory.rs`).
const fn load_operator(operator: u32) -> Option<SimdOperator> {
    match operator {
        SIMD_V128_LOAD8X8_S => Some(SimdOperator::I16x8ExtendLowI8x16S),
        SIMD_V128_LOAD8X8_U => Some(SimdOperator::I16x8ExtendLowI8x16U),
        SIMD_V128_LOAD16X4_S => Some(SimdOperator::I32x4ExtendLowI16x8S),
        SIMD_V128_LOAD16X4_U => Some(SimdOperator::I32x4ExtendLowI16x8U),
        SIMD_V128_LOAD32X2_S => Some(SimdOperator::I64x2ExtendLowI32x4S),
        SIMD_V128_LOAD32X2_U => Some(SimdOperator::I64x2ExtendLowI32x4U),
        SIMD_V128_LOAD8_SPLAT => Some(SimdOperator::I8x16Splat),
        SIMD_V128_LOAD16_SPLAT => Some(SimdOperator::I16x8Splat),
        SIMD_V128_LOAD32_SPLAT => Some(SimdOperator::I32x4Splat),
        SIMD_V128_LOAD64_SPLAT => Some(SimdOperator::I64x2Splat),
        _ => None,
    }
}

/// The vector a load of `OP` makes from its bytes' word: extended, splatted or zeroed.
#[inline(always)]
fn load_value<const OP: u32>(word: u64) -> u128 {
    let operator = const { load_operator(OP) };
    match operator {
        Some(operator) => operator.bits(u128::from(word), 0, 0),
        None => match OP {
            SIMD_V128_LOAD32_ZERO => u128::from(word as u32),
            _ => u128::from(word),
        },
    }
}

/// `D = A op B` on two vectors, the operator `OP` names.
#[inline(always)]
unsafe fn pair<const OP: u32>(s: &S) {
    unsafe {
        let value = operate::<OP>(vector_read::<HOLE_A>(s), vector_read::<HOLE_B>(s), 0);
        vector_write::<0>(s, value);
    }
}

/// `D = op A` on one vector, the operator `OP` names.
#[inline(always)]
unsafe fn single<const OP: u32>(s: &S) {
    unsafe {
        let value = operate::<OP>(vector_read::<HOLE_A>(s), 0, 0);
        vector_write::<0>(s, value);
    }
}

/// `D = A op B` over the lanes of two vectors, the binary family.
#[inline(always)]
pub(crate) unsafe fn binary<const OP: u32>(s: S) -> Out {
    unsafe { pair::<OP>(&s) };
    Out::Next(s)
}

/// The lane comparisons of two vectors (all ones where they hold).
#[inline(always)]
pub(crate) unsafe fn compare<const OP: u32>(s: S) -> Out {
    unsafe { pair::<OP>(&s) };
    Out::Next(s)
}

/// The float lane operators of two vectors (min, max, pseudo-min and -max, relaxed forms).
#[inline(always)]
pub(crate) unsafe fn float<const OP: u32>(s: S) -> Out {
    unsafe { pair::<OP>(&s) };
    Out::Next(s)
}

/// The narrowing, extended-multiply and dot operators of two vectors.
#[inline(always)]
pub(crate) unsafe fn wide<const OP: u32>(s: S) -> Out {
    unsafe { pair::<OP>(&s) };
    Out::Next(s)
}

/// `D = op A` on one vector: the unary family (popcount, abs, neg, float rounding).
#[inline(always)]
pub(crate) unsafe fn unary<const OP: u32>(s: S) -> Out {
    unsafe { single::<OP>(&s) };
    Out::Next(s)
}

/// `D = op A` widening the low or high half of one vector.
#[inline(always)]
pub(crate) unsafe fn widen<const OP: u32>(s: S) -> Out {
    unsafe { single::<OP>(&s) };
    Out::Next(s)
}

/// `D = op A` converting lanes between int and float widths, trapping or saturating.
#[inline(always)]
pub(crate) unsafe fn convert<const OP: u32>(s: S) -> Out {
    unsafe { single::<OP>(&s) };
    Out::Next(s)
}

/// `D = A` shifted per lane by the scalar count in `HOLE_B`.
#[inline(always)]
pub(crate) unsafe fn shift<const OP: u32>(s: S) -> Out {
    unsafe {
        let count = u128::from(read::<HOLE_B>(&s));
        let value = operate::<OP>(vector_read::<HOLE_A>(&s), count, 0);
        vector_write::<0>(&s, value);
    }
    Out::Next(s)
}

/// `D` (a scalar word) = the any, all or bitmask test of vector `A`.
#[inline(always)]
pub(crate) unsafe fn test<const OP: u32>(s: S) -> Out {
    let left = unsafe { vector_read::<HOLE_A>(&s) };
    let word = simd_scalar(&mut StencilSimd, selector_of(OP, 0), left, None);
    unsafe { write::<0>(&s, word) };
    Out::Next(s)
}

/// `D = v128.load` of the form `OP` at the 64-bit address in `HOLE_A`: the staged lane load of its
/// `load_bytes`, then the operator's widening or splat. Traps out of bounds, writing nothing.
#[inline(always)]
pub(crate) unsafe fn load<const OP: u32>(s: S) -> Out {
    let index = unsafe { read::<HOLE_A>(&s) };
    let bytes = const { load_bytes(OP) };
    let mut memory = StencilMemory::of(&s);
    match load_lane(&mut memory, bytes, index, 0, false) {
        Ok(word) => {
            unsafe { vector_write::<0>(&s, load_value::<OP>(word)) };
            Out::Next(s)
        }
        Err(error) => trap(s, error),
    }
}
