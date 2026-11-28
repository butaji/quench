//! The b-jit's vector steps beyond `wasm_stitch_simd.rs`: the operators and the splatting, extending
//! and zeroing vector loads that `wasm_rows_simd_more.rs` runs (kinds `SIMD_MORE_*`, 36..=45). A row
//! is planned as `wasm_stitch_simd.rs` plans its vector rows: a vector operand's hole is its register's
//! low-word offset (`vector_slot`), a scalar operand's its frame slot (`slot`).
//!
//! `simd_more` is the operator arm's fallback in `plan()` (`simd(...).or_else(...)`), and it also plans
//! the V128 splat, extend and zero loads: `Step::of` is `None` for those, so `plan()` calls it where its
//! step match would have returned. The memory binding needs no check here, as for `Simd::Load`: a
//! stitched function binds only its frame's memory (`plan` rejects any other binding).

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::wasm_stitch_simd::{vector_row, vector_slot};
use super::{Planned, World, codes, slot};
use quench_runtime::bytecode::WideInstruction;
use quench_runtime::staged::wasm_simd::Simd;
use quench_runtime::staged::wasm_step::{Family, Step};
use quench_runtime::wasm::memory::MemoryLoad;
use quench_runtime::wasm::simd::SimdOperator;
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

/// Each operator tag is the runtime's `SimdOperator` discriminant, checked at compile time: a
/// reordered table fails the build here, not in a row.
macro_rules! tags {
    ($($operator:ident = $code:ident;)*) => {
        $(const _: () = assert!(SimdOperator::$operator as u32 == codes::$code);)*
    };
}
tags! {
    I8x16MinS = SIMD_I8X16_MIN_S; I8x16MinU = SIMD_I8X16_MIN_U; I8x16MaxS = SIMD_I8X16_MAX_S;
    I8x16MaxU = SIMD_I8X16_MAX_U; I16x8MinS = SIMD_I16X8_MIN_S; I16x8MinU = SIMD_I16X8_MIN_U;
    I16x8MaxS = SIMD_I16X8_MAX_S; I16x8MaxU = SIMD_I16X8_MAX_U; I32x4MinS = SIMD_I32X4_MIN_S;
    I32x4MinU = SIMD_I32X4_MIN_U; I32x4MaxS = SIMD_I32X4_MAX_S; I32x4MaxU = SIMD_I32X4_MAX_U;
    I8x16AddSatS = SIMD_I8X16_ADD_SAT_S; I8x16AddSatU = SIMD_I8X16_ADD_SAT_U; I8x16SubSatS = SIMD_I8X16_SUB_SAT_S;
    I8x16SubSatU = SIMD_I8X16_SUB_SAT_U; I16x8AddSatS = SIMD_I16X8_ADD_SAT_S; I16x8AddSatU = SIMD_I16X8_ADD_SAT_U;
    I16x8SubSatS = SIMD_I16X8_SUB_SAT_S; I16x8SubSatU = SIMD_I16X8_SUB_SAT_U; I8x16AvgrU = SIMD_I8X16_AVGR_U;
    I16x8AvgrU = SIMD_I16X8_AVGR_U; I16x8Q15MulrSatS = SIMD_I16X8_Q15_MULR_SAT_S; I16x8RelaxedQ15mulrS = SIMD_I16X8_RELAXED_Q15MULR_S;
    I8x16RelaxedSwizzle = SIMD_I8X16_RELAXED_SWIZZLE; I16x8LtS = SIMD_I16X8_LT_S; I16x8GtS = SIMD_I16X8_GT_S;
    I16x8LeS = SIMD_I16X8_LE_S; I16x8GeS = SIMD_I16X8_GE_S; I16x8LtU = SIMD_I16X8_LT_U;
    I16x8GtU = SIMD_I16X8_GT_U; I16x8LeU = SIMD_I16X8_LE_U; I16x8GeU = SIMD_I16X8_GE_U;
    I32x4LtS = SIMD_I32X4_LT_S; I32x4GtS = SIMD_I32X4_GT_S; I32x4LeS = SIMD_I32X4_LE_S;
    I32x4GeS = SIMD_I32X4_GE_S; I32x4LtU = SIMD_I32X4_LT_U; I32x4GtU = SIMD_I32X4_GT_U;
    I32x4LeU = SIMD_I32X4_LE_U; I32x4GeU = SIMD_I32X4_GE_U; I64x2Eq = SIMD_I64X2_EQ;
    I64x2Ne = SIMD_I64X2_NE; I64x2LtS = SIMD_I64X2_LT_S; I64x2GtS = SIMD_I64X2_GT_S;
    I64x2LeS = SIMD_I64X2_LE_S; I64x2GeS = SIMD_I64X2_GE_S; F32x4Min = SIMD_F32X4_MIN;
    F32x4Max = SIMD_F32X4_MAX; F32x4PMin = SIMD_F32X4_PMIN; F32x4PMax = SIMD_F32X4_PMAX;
    F32x4RelaxedMin = SIMD_F32X4_RELAXED_MIN; F32x4RelaxedMax = SIMD_F32X4_RELAXED_MAX; F64x2Min = SIMD_F64X2_MIN;
    F64x2Max = SIMD_F64X2_MAX; F64x2PMin = SIMD_F64X2_PMIN; F64x2PMax = SIMD_F64X2_PMAX;
    F64x2RelaxedMin = SIMD_F64X2_RELAXED_MIN; F64x2RelaxedMax = SIMD_F64X2_RELAXED_MAX; I8x16Shl = SIMD_I8X16_SHL;
    I8x16ShrS = SIMD_I8X16_SHR_S; I8x16ShrU = SIMD_I8X16_SHR_U; I16x8Shl = SIMD_I16X8_SHL;
    I16x8ShrS = SIMD_I16X8_SHR_S; I16x8ShrU = SIMD_I16X8_SHR_U; I32x4Shl = SIMD_I32X4_SHL;
    I32x4ShrS = SIMD_I32X4_SHR_S; I32x4ShrU = SIMD_I32X4_SHR_U; I64x2Shl = SIMD_I64X2_SHL;
    I64x2ShrS = SIMD_I64X2_SHR_S; I64x2ShrU = SIMD_I64X2_SHR_U; V128AnyTrue = SIMD_V128_ANY_TRUE;
    I8x16AllTrue = SIMD_I8X16_ALL_TRUE; I8x16Bitmask = SIMD_I8X16_BITMASK; I16x8AllTrue = SIMD_I16X8_ALL_TRUE;
    I16x8Bitmask = SIMD_I16X8_BITMASK; I32x4AllTrue = SIMD_I32X4_ALL_TRUE; I32x4Bitmask = SIMD_I32X4_BITMASK;
    I64x2AllTrue = SIMD_I64X2_ALL_TRUE; I64x2Bitmask = SIMD_I64X2_BITMASK; I8x16Popcnt = SIMD_I8X16_POPCNT;
    I16x8Abs = SIMD_I16X8_ABS; I16x8Neg = SIMD_I16X8_NEG; I32x4Abs = SIMD_I32X4_ABS;
    I32x4Neg = SIMD_I32X4_NEG; I64x2Abs = SIMD_I64X2_ABS; I64x2Neg = SIMD_I64X2_NEG;
    F32x4Sqrt = SIMD_F32X4_SQRT; F32x4Ceil = SIMD_F32X4_CEIL; F32x4Floor = SIMD_F32X4_FLOOR;
    F32x4Trunc = SIMD_F32X4_TRUNC; F32x4Nearest = SIMD_F32X4_NEAREST; F64x2Sqrt = SIMD_F64X2_SQRT;
    F64x2Ceil = SIMD_F64X2_CEIL; F64x2Floor = SIMD_F64X2_FLOOR; F64x2Trunc = SIMD_F64X2_TRUNC;
    F64x2Nearest = SIMD_F64X2_NEAREST; I16x8ExtendLowI8x16S = SIMD_I16X8_EXTEND_LOW_I8X16_S; I16x8ExtendLowI8x16U = SIMD_I16X8_EXTEND_LOW_I8X16_U;
    I16x8ExtendHighI8x16S = SIMD_I16X8_EXTEND_HIGH_I8X16_S; I16x8ExtendHighI8x16U = SIMD_I16X8_EXTEND_HIGH_I8X16_U; I32x4ExtendLowI16x8S = SIMD_I32X4_EXTEND_LOW_I16X8_S;
    I32x4ExtendLowI16x8U = SIMD_I32X4_EXTEND_LOW_I16X8_U; I32x4ExtendHighI16x8S = SIMD_I32X4_EXTEND_HIGH_I16X8_S; I32x4ExtendHighI16x8U = SIMD_I32X4_EXTEND_HIGH_I16X8_U;
    I64x2ExtendLowI32x4S = SIMD_I64X2_EXTEND_LOW_I32X4_S; I64x2ExtendLowI32x4U = SIMD_I64X2_EXTEND_LOW_I32X4_U; I64x2ExtendHighI32x4S = SIMD_I64X2_EXTEND_HIGH_I32X4_S;
    I64x2ExtendHighI32x4U = SIMD_I64X2_EXTEND_HIGH_I32X4_U; I16x8ExtAddPairwiseI8x16S = SIMD_I16X8_EXT_ADD_PAIRWISE_I8X16_S; I16x8ExtAddPairwiseI8x16U = SIMD_I16X8_EXT_ADD_PAIRWISE_I8X16_U;
    I32x4ExtAddPairwiseI16x8S = SIMD_I32X4_EXT_ADD_PAIRWISE_I16X8_S; I32x4ExtAddPairwiseI16x8U = SIMD_I32X4_EXT_ADD_PAIRWISE_I16X8_U; I8x16NarrowI16x8S = SIMD_I8X16_NARROW_I16X8_S;
    I8x16NarrowI16x8U = SIMD_I8X16_NARROW_I16X8_U; I16x8NarrowI32x4S = SIMD_I16X8_NARROW_I32X4_S; I16x8NarrowI32x4U = SIMD_I16X8_NARROW_I32X4_U;
    I16x8ExtMulLowI8x16S = SIMD_I16X8_EXT_MUL_LOW_I8X16_S; I16x8ExtMulLowI8x16U = SIMD_I16X8_EXT_MUL_LOW_I8X16_U; I16x8ExtMulHighI8x16S = SIMD_I16X8_EXT_MUL_HIGH_I8X16_S;
    I16x8ExtMulHighI8x16U = SIMD_I16X8_EXT_MUL_HIGH_I8X16_U; I32x4ExtMulLowI16x8S = SIMD_I32X4_EXT_MUL_LOW_I16X8_S; I32x4ExtMulLowI16x8U = SIMD_I32X4_EXT_MUL_LOW_I16X8_U;
    I32x4ExtMulHighI16x8S = SIMD_I32X4_EXT_MUL_HIGH_I16X8_S; I32x4ExtMulHighI16x8U = SIMD_I32X4_EXT_MUL_HIGH_I16X8_U; I64x2ExtMulLowI32x4S = SIMD_I64X2_EXT_MUL_LOW_I32X4_S;
    I64x2ExtMulLowI32x4U = SIMD_I64X2_EXT_MUL_LOW_I32X4_U; I64x2ExtMulHighI32x4S = SIMD_I64X2_EXT_MUL_HIGH_I32X4_S; I64x2ExtMulHighI32x4U = SIMD_I64X2_EXT_MUL_HIGH_I32X4_U;
    I32x4DotI16x8S = SIMD_I32X4_DOT_I16X8_S; I16x8RelaxedDotI8x16I7x16S = SIMD_I16X8_RELAXED_DOT_I8X16_I7X16_S; I32x4TruncSatF32x4S = SIMD_I32X4_TRUNC_SAT_F32X4_S;
    I32x4TruncSatF32x4U = SIMD_I32X4_TRUNC_SAT_F32X4_U; F32x4ConvertI32x4S = SIMD_F32X4_CONVERT_I32X4_S; F32x4ConvertI32x4U = SIMD_F32X4_CONVERT_I32X4_U;
    I32x4TruncSatF64x2SZero = SIMD_I32X4_TRUNC_SAT_F64X2_SZERO; I32x4TruncSatF64x2UZero = SIMD_I32X4_TRUNC_SAT_F64X2_UZERO; F64x2ConvertLowI32x4S = SIMD_F64X2_CONVERT_LOW_I32X4_S;
    F64x2ConvertLowI32x4U = SIMD_F64X2_CONVERT_LOW_I32X4_U; F32x4DemoteF64x2Zero = SIMD_F32X4_DEMOTE_F64X2_ZERO; F64x2PromoteLowF32x4 = SIMD_F64X2_PROMOTE_LOW_F32X4;
    I32x4RelaxedTruncF32x4S = SIMD_I32X4_RELAXED_TRUNC_F32X4_S; I32x4RelaxedTruncF32x4U = SIMD_I32X4_RELAXED_TRUNC_F32X4_U; I32x4RelaxedTruncF64x2SZero = SIMD_I32X4_RELAXED_TRUNC_F64X2_SZERO;
    I32x4RelaxedTruncF64x2UZero = SIMD_I32X4_RELAXED_TRUNC_F64X2_UZERO;
}

/// Each vector load tag is the runtime's `MemoryLoad` discriminant.
macro_rules! load_tags {
    ($($load:ident = $code:ident;)*) => {
        $(const _: () = assert!(MemoryLoad::$load as u32 == codes::$code);)*
    };
}
load_tags! {
    V128Load8x8S = SIMD_V128_LOAD8X8_S; V128Load8x8U = SIMD_V128_LOAD8X8_U;
    V128Load16x4S = SIMD_V128_LOAD16X4_S; V128Load16x4U = SIMD_V128_LOAD16X4_U;
    V128Load32x2S = SIMD_V128_LOAD32X2_S; V128Load32x2U = SIMD_V128_LOAD32X2_U;
    V128Load8Splat = SIMD_V128_LOAD8_SPLAT; V128Load16Splat = SIMD_V128_LOAD16_SPLAT;
    V128Load32Splat = SIMD_V128_LOAD32_SPLAT; V128Load64Splat = SIMD_V128_LOAD64_SPLAT;
    V128Load32Zero = SIMD_V128_LOAD32_ZERO; V128Load64Zero = SIMD_V128_LOAD64_ZERO;
}

/// The rows of the step at `instruction` if these rows compute it, else `None`: an operator in one of
/// the `SIMD_MORE_*` families, or a V128 splat, extend or zero load. `_pc` is unused: the rows need no position.
pub(crate) fn simd_more(_pc: usize, instruction: WideInstruction, world: &World<'_>) -> Option<Vec<Planned>> {
    match Step::of(instruction.op(), instruction.imm()) {
        Some(Step::Simd(Simd::Operator { selector })) => operator(instruction, selector, world),
        None if Family::of(instruction.op()) == Some(Family::MemoryLoad) => load(instruction, world),
        _ => None,
    }
}

/// The kind of the `SIMD_MORE_*` family `tag` is in, `None` for an operator these rows do not compute
/// (`wasm_families.rs` derives it from the list's family column).
fn kind_of(tag: u32) -> Option<u8> {
    codes::simd_more_kind(tag)
}

/// The row of an operator `kind_of` admits, its holes by the kind's shape: a scalar destination for
/// a test, a scalar count for a shift, a vector source alone for a unary, widening or conversion, and
/// two vector sources otherwise.
fn operator(instruction: WideInstruction, selector: u32, world: &World<'_>) -> Option<Vec<Planned>> {
    let (operator, _) = SimdOperator::from_selector(selector)?;
    let tag = operator as u32;
    let kind = kind_of(tag)?;
    let (a, b, c) = (instruction.a(), instruction.b(), instruction.c());
    let vector = |register: u16| vector_slot(world.registers, register);
    let planned = match kind {
        codes::SIMD_MORE_TEST => vector_row((kind, tag), [slot(a), vector(b)]),
        codes::SIMD_MORE_SHIFT => vector_row((kind, tag), [vector(a), vector(b), slot(c)]),
        codes::SIMD_MORE_UNARY | codes::SIMD_MORE_WIDEN | codes::SIMD_MORE_CONVERT => {
            vector_row((kind, tag), [vector(a), vector(b)])
        }
        _ => vector_row((kind, tag), [vector(a), vector(b), vector(c)]),
    };
    Some(vec![planned])
}

/// The row of a splatting, extending or zeroing V128 load: `a = load(c)` at the effective address in
/// `c`, of the form the tag names (`MemoryLoad`), as `Simd::Load` loads the plain V128.
fn load(instruction: WideInstruction, world: &World<'_>) -> Option<Vec<Planned>> {
    let tag = instruction.imm();
    codes::SIMD_MORE_LOAD_OPS.contains(&tag).then(|| {
        vec![vector_row((codes::SIMD_MORE_LOAD, tag), [vector_slot(world.registers, instruction.a()), slot(instruction.c())])]
    })
}

/// Whether `instruction` is a V128 splat, extend or zero load: a vector step with no `Step`, which
/// `uses_vectors` must count for the frame's vector area.
pub(crate) fn is_vector_load(instruction: WideInstruction) -> bool {
    Family::of(instruction.op()) == Some(Family::MemoryLoad) && codes::SIMD_MORE_LOAD_OPS.contains(&instruction.imm())
}
