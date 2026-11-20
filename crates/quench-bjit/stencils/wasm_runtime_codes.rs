//! The Wasm codes only the lib's runtime reads (`quench-runtime`'s `wasm_codes_check.rs` and the Wasm
//! lowering): the SIMD operator tags, the table and target layouts the runtime checks, and the frame's
//! result word. The stencil crate and the build script do not include this file (`src/lib.rs` does), so
//! none of it compiles there.

use crate::wasm_families::*;
use crate::wasm_kinds::*;
use crate::wasm_layout::*;

#[macro_use]
#[path = "../../quench-runtime/src/staged/wasm_lists.rs"]
mod wasm_lists;

/// The SIMD operators, from the list (`wasm_lists.rs`): each operator's tag as the enum `SimdTag`
/// (the runtime's `SimdOperator` has the same order). The b-jit names its operators by these constants.
macro_rules! simd_tag_enum {
    ($($name:ident $([$lane:ident])? : $width:expr, $left:ident, $right:expr, $result:ident, $family:ident => |$a:ident, $b:ident, $l:ident| $body:expr;)*) => {
        /// A SIMD operator's tag: its position in the list.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[repr(u32)]
        pub enum SimdTag {
            $($name,)*
        }
    };
}
simd_operator_list!(simd_tag_enum);

pub const T_F64: u8 = AccessType::F64 as u8;

pub const FRAME_RESULT_WORD: usize = WASM_RESULT_WORD;

pub const TARGET_ENTRY_AT: usize = core::mem::offset_of!(TableTargetLayout, entry);

/// `TableWords`' layout as a row reads it: `#[repr(C)]`, the base then the length.
#[repr(C)]
struct TableWordsLayout {
    base: usize,
    length: u64,
}
pub const TABLE_WORDS_BASE_AT: usize = core::mem::offset_of!(TableWordsLayout, base);
pub const TABLE_WORDS_LENGTH_AT: usize = core::mem::offset_of!(TableWordsLayout, length);
/// `TableTarget::NO_ENTRY`: an element that is no module function's body.
pub const NO_ENTRY: u32 = u32::MAX;

/// The number of SIMD operators, which the runtime asserts equals `SimdOperator::ALL`.
pub const SIMD_OPERATOR_COUNT: u32 = SIMD_FAMILY_OF.len() as u32;

/// The row kind of an operator in a `SIMD_MORE_*` family, `None` for any other operator.
pub const fn simd_more_kind(tag: u32) -> Option<u8> {
    if (tag as usize) >= SIMD_FAMILY_OF.len() {
        return None;
    }
    let family = SIMD_FAMILY_OF[tag as usize];
    if family == SimdFamily::MoreBinary as u8 {
        Some(SIMD_MORE_BINARY)
    } else if family == SimdFamily::MoreCompare as u8 {
        Some(SIMD_MORE_COMPARE)
    } else if family == SimdFamily::MoreFloat as u8 {
        Some(SIMD_MORE_FLOAT)
    } else if family == SimdFamily::MoreShift as u8 {
        Some(SIMD_MORE_SHIFT)
    } else if family == SimdFamily::MoreTest as u8 {
        Some(SIMD_MORE_TEST)
    } else if family == SimdFamily::MoreUnary as u8 {
        Some(SIMD_MORE_UNARY)
    } else if family == SimdFamily::MoreWiden as u8 {
        Some(SIMD_MORE_WIDEN)
    } else if family == SimdFamily::MoreWide as u8 {
        Some(SIMD_MORE_WIDE)
    } else if family == SimdFamily::MoreConvert as u8 {
        Some(SIMD_MORE_CONVERT)
    } else {
        None
    }
}

pub const SIMD_V128_NOT: u32 = SimdTag::V128Not as u32;
pub const SIMD_V128_AND: u32 = SimdTag::V128And as u32;
pub const SIMD_V128_ANDNOT: u32 = SimdTag::V128AndNot as u32;
pub const SIMD_V128_OR: u32 = SimdTag::V128Or as u32;
pub const SIMD_V128_XOR: u32 = SimdTag::V128Xor as u32;
pub const SIMD_I8X16_SWIZZLE: u32 = SimdTag::I8x16Swizzle as u32;
pub const SIMD_I8X16_SPLAT: u32 = SimdTag::I8x16Splat as u32;
pub const SIMD_I16X8_SPLAT: u32 = SimdTag::I16x8Splat as u32;
pub const SIMD_I32X4_SPLAT: u32 = SimdTag::I32x4Splat as u32;
pub const SIMD_I64X2_SPLAT: u32 = SimdTag::I64x2Splat as u32;
pub const SIMD_F32X4_SPLAT: u32 = SimdTag::F32x4Splat as u32;
pub const SIMD_F64X2_SPLAT: u32 = SimdTag::F64x2Splat as u32;
pub const SIMD_I8X16_EXTRACT_S: u32 = SimdTag::I8x16ExtractLaneS as u32;
pub const SIMD_I8X16_EXTRACT_U: u32 = SimdTag::I8x16ExtractLaneU as u32;
pub const SIMD_I16X8_EXTRACT_S: u32 = SimdTag::I16x8ExtractLaneS as u32;
pub const SIMD_I16X8_EXTRACT_U: u32 = SimdTag::I16x8ExtractLaneU as u32;
pub const SIMD_I32X4_EXTRACT: u32 = SimdTag::I32x4ExtractLane as u32;
pub const SIMD_I64X2_EXTRACT: u32 = SimdTag::I64x2ExtractLane as u32;
pub const SIMD_F32X4_EXTRACT: u32 = SimdTag::F32x4ExtractLane as u32;
pub const SIMD_F64X2_EXTRACT: u32 = SimdTag::F64x2ExtractLane as u32;
pub const SIMD_I8X16_REPLACE: u32 = SimdTag::I8x16ReplaceLane as u32;
pub const SIMD_I16X8_REPLACE: u32 = SimdTag::I16x8ReplaceLane as u32;
pub const SIMD_I32X4_REPLACE: u32 = SimdTag::I32x4ReplaceLane as u32;
pub const SIMD_I64X2_REPLACE: u32 = SimdTag::I64x2ReplaceLane as u32;
pub const SIMD_F32X4_REPLACE: u32 = SimdTag::F32x4ReplaceLane as u32;
pub const SIMD_F64X2_REPLACE: u32 = SimdTag::F64x2ReplaceLane as u32;
pub const SIMD_F32X4_ADD: u32 = SimdTag::F32x4Add as u32;
pub const SIMD_F32X4_SUB: u32 = SimdTag::F32x4Sub as u32;
pub const SIMD_F32X4_MUL: u32 = SimdTag::F32x4Mul as u32;
pub const SIMD_F32X4_DIV: u32 = SimdTag::F32x4Div as u32;
pub const SIMD_F32X4_EQ: u32 = SimdTag::F32x4Eq as u32;
pub const SIMD_F32X4_NE: u32 = SimdTag::F32x4Ne as u32;
pub const SIMD_F32X4_LT: u32 = SimdTag::F32x4Lt as u32;
pub const SIMD_F32X4_GT: u32 = SimdTag::F32x4Gt as u32;
pub const SIMD_F32X4_LE: u32 = SimdTag::F32x4Le as u32;
pub const SIMD_F32X4_GE: u32 = SimdTag::F32x4Ge as u32;
pub const SIMD_F32X4_ABS: u32 = SimdTag::F32x4Abs as u32;
pub const SIMD_F32X4_NEG: u32 = SimdTag::F32x4Neg as u32;
pub const SIMD_F64X2_ADD: u32 = SimdTag::F64x2Add as u32;
pub const SIMD_F64X2_SUB: u32 = SimdTag::F64x2Sub as u32;
pub const SIMD_F64X2_MUL: u32 = SimdTag::F64x2Mul as u32;
pub const SIMD_F64X2_DIV: u32 = SimdTag::F64x2Div as u32;
pub const SIMD_F64X2_EQ: u32 = SimdTag::F64x2Eq as u32;
pub const SIMD_F64X2_NE: u32 = SimdTag::F64x2Ne as u32;
pub const SIMD_F64X2_LT: u32 = SimdTag::F64x2Lt as u32;
pub const SIMD_F64X2_GT: u32 = SimdTag::F64x2Gt as u32;
pub const SIMD_F64X2_LE: u32 = SimdTag::F64x2Le as u32;
pub const SIMD_F64X2_GE: u32 = SimdTag::F64x2Ge as u32;
pub const SIMD_F64X2_ABS: u32 = SimdTag::F64x2Abs as u32;
pub const SIMD_F64X2_NEG: u32 = SimdTag::F64x2Neg as u32;
pub const SIMD_I8X16_EQ: u32 = SimdTag::I8x16Eq as u32;
pub const SIMD_I8X16_NE: u32 = SimdTag::I8x16Ne as u32;
pub const SIMD_I8X16_LT_S: u32 = SimdTag::I8x16LtS as u32;
pub const SIMD_I8X16_GT_S: u32 = SimdTag::I8x16GtS as u32;
pub const SIMD_I8X16_LE_S: u32 = SimdTag::I8x16LeS as u32;
pub const SIMD_I8X16_GE_S: u32 = SimdTag::I8x16GeS as u32;
pub const SIMD_I8X16_LT_U: u32 = SimdTag::I8x16LtU as u32;
pub const SIMD_I8X16_GT_U: u32 = SimdTag::I8x16GtU as u32;
pub const SIMD_I8X16_LE_U: u32 = SimdTag::I8x16LeU as u32;
pub const SIMD_I8X16_GE_U: u32 = SimdTag::I8x16GeU as u32;
pub const SIMD_I8X16_ADD: u32 = SimdTag::I8x16Add as u32;
pub const SIMD_I8X16_SUB: u32 = SimdTag::I8x16Sub as u32;
pub const SIMD_I8X16_ABS: u32 = SimdTag::I8x16Abs as u32;
pub const SIMD_I8X16_NEG: u32 = SimdTag::I8x16Neg as u32;
pub const SIMD_I16X8_EQ: u32 = SimdTag::I16x8Eq as u32;
pub const SIMD_I16X8_NE: u32 = SimdTag::I16x8Ne as u32;
pub const SIMD_I16X8_ADD: u32 = SimdTag::I16x8Add as u32;
pub const SIMD_I16X8_SUB: u32 = SimdTag::I16x8Sub as u32;
pub const SIMD_I16X8_MUL: u32 = SimdTag::I16x8Mul as u32;
pub const SIMD_I32X4_EQ: u32 = SimdTag::I32x4Eq as u32;
pub const SIMD_I32X4_NE: u32 = SimdTag::I32x4Ne as u32;
pub const SIMD_I32X4_ADD: u32 = SimdTag::I32x4Add as u32;
pub const SIMD_I32X4_SUB: u32 = SimdTag::I32x4Sub as u32;
pub const SIMD_I32X4_MUL: u32 = SimdTag::I32x4Mul as u32;
pub const SIMD_I64X2_ADD: u32 = SimdTag::I64x2Add as u32;
pub const SIMD_I64X2_SUB: u32 = SimdTag::I64x2Sub as u32;
pub const SIMD_I64X2_MUL: u32 = SimdTag::I64x2Mul as u32;
pub const SIMD_I8X16_MIN_S: u32 = SimdTag::I8x16MinS as u32;
pub const SIMD_I8X16_MIN_U: u32 = SimdTag::I8x16MinU as u32;
pub const SIMD_I8X16_MAX_S: u32 = SimdTag::I8x16MaxS as u32;
pub const SIMD_I8X16_MAX_U: u32 = SimdTag::I8x16MaxU as u32;
pub const SIMD_I16X8_MIN_S: u32 = SimdTag::I16x8MinS as u32;
pub const SIMD_I16X8_MIN_U: u32 = SimdTag::I16x8MinU as u32;
pub const SIMD_I16X8_MAX_S: u32 = SimdTag::I16x8MaxS as u32;
pub const SIMD_I16X8_MAX_U: u32 = SimdTag::I16x8MaxU as u32;
pub const SIMD_I32X4_MIN_S: u32 = SimdTag::I32x4MinS as u32;
pub const SIMD_I32X4_MIN_U: u32 = SimdTag::I32x4MinU as u32;
pub const SIMD_I32X4_MAX_S: u32 = SimdTag::I32x4MaxS as u32;
pub const SIMD_I32X4_MAX_U: u32 = SimdTag::I32x4MaxU as u32;
pub const SIMD_I8X16_ADD_SAT_S: u32 = SimdTag::I8x16AddSatS as u32;
pub const SIMD_I8X16_ADD_SAT_U: u32 = SimdTag::I8x16AddSatU as u32;
pub const SIMD_I8X16_SUB_SAT_S: u32 = SimdTag::I8x16SubSatS as u32;
pub const SIMD_I8X16_SUB_SAT_U: u32 = SimdTag::I8x16SubSatU as u32;
pub const SIMD_I16X8_ADD_SAT_S: u32 = SimdTag::I16x8AddSatS as u32;
pub const SIMD_I16X8_ADD_SAT_U: u32 = SimdTag::I16x8AddSatU as u32;
pub const SIMD_I16X8_SUB_SAT_S: u32 = SimdTag::I16x8SubSatS as u32;
pub const SIMD_I16X8_SUB_SAT_U: u32 = SimdTag::I16x8SubSatU as u32;
pub const SIMD_I8X16_AVGR_U: u32 = SimdTag::I8x16AvgrU as u32;
pub const SIMD_I16X8_AVGR_U: u32 = SimdTag::I16x8AvgrU as u32;
pub const SIMD_I16X8_Q15_MULR_SAT_S: u32 = SimdTag::I16x8Q15MulrSatS as u32;
pub const SIMD_I16X8_RELAXED_Q15MULR_S: u32 = SimdTag::I16x8RelaxedQ15mulrS as u32;
pub const SIMD_I8X16_RELAXED_SWIZZLE: u32 = SimdTag::I8x16RelaxedSwizzle as u32;
pub const SIMD_I16X8_LT_S: u32 = SimdTag::I16x8LtS as u32;
pub const SIMD_I16X8_GT_S: u32 = SimdTag::I16x8GtS as u32;
pub const SIMD_I16X8_LE_S: u32 = SimdTag::I16x8LeS as u32;
pub const SIMD_I16X8_GE_S: u32 = SimdTag::I16x8GeS as u32;
pub const SIMD_I16X8_LT_U: u32 = SimdTag::I16x8LtU as u32;
pub const SIMD_I16X8_GT_U: u32 = SimdTag::I16x8GtU as u32;
pub const SIMD_I16X8_LE_U: u32 = SimdTag::I16x8LeU as u32;
pub const SIMD_I16X8_GE_U: u32 = SimdTag::I16x8GeU as u32;
pub const SIMD_I32X4_LT_S: u32 = SimdTag::I32x4LtS as u32;
pub const SIMD_I32X4_GT_S: u32 = SimdTag::I32x4GtS as u32;
pub const SIMD_I32X4_LE_S: u32 = SimdTag::I32x4LeS as u32;
pub const SIMD_I32X4_GE_S: u32 = SimdTag::I32x4GeS as u32;
pub const SIMD_I32X4_LT_U: u32 = SimdTag::I32x4LtU as u32;
pub const SIMD_I32X4_GT_U: u32 = SimdTag::I32x4GtU as u32;
pub const SIMD_I32X4_LE_U: u32 = SimdTag::I32x4LeU as u32;
pub const SIMD_I32X4_GE_U: u32 = SimdTag::I32x4GeU as u32;
pub const SIMD_I64X2_EQ: u32 = SimdTag::I64x2Eq as u32;
pub const SIMD_I64X2_NE: u32 = SimdTag::I64x2Ne as u32;
pub const SIMD_I64X2_LT_S: u32 = SimdTag::I64x2LtS as u32;
pub const SIMD_I64X2_GT_S: u32 = SimdTag::I64x2GtS as u32;
pub const SIMD_I64X2_LE_S: u32 = SimdTag::I64x2LeS as u32;
pub const SIMD_I64X2_GE_S: u32 = SimdTag::I64x2GeS as u32;
pub const SIMD_F32X4_MIN: u32 = SimdTag::F32x4Min as u32;
pub const SIMD_F32X4_MAX: u32 = SimdTag::F32x4Max as u32;
pub const SIMD_F32X4_PMIN: u32 = SimdTag::F32x4PMin as u32;
pub const SIMD_F32X4_PMAX: u32 = SimdTag::F32x4PMax as u32;
pub const SIMD_F32X4_RELAXED_MIN: u32 = SimdTag::F32x4RelaxedMin as u32;
pub const SIMD_F32X4_RELAXED_MAX: u32 = SimdTag::F32x4RelaxedMax as u32;
pub const SIMD_F64X2_MIN: u32 = SimdTag::F64x2Min as u32;
pub const SIMD_F64X2_MAX: u32 = SimdTag::F64x2Max as u32;
pub const SIMD_F64X2_PMIN: u32 = SimdTag::F64x2PMin as u32;
pub const SIMD_F64X2_PMAX: u32 = SimdTag::F64x2PMax as u32;
pub const SIMD_F64X2_RELAXED_MIN: u32 = SimdTag::F64x2RelaxedMin as u32;
pub const SIMD_F64X2_RELAXED_MAX: u32 = SimdTag::F64x2RelaxedMax as u32;
pub const SIMD_I8X16_SHL: u32 = SimdTag::I8x16Shl as u32;
pub const SIMD_I8X16_SHR_S: u32 = SimdTag::I8x16ShrS as u32;
pub const SIMD_I8X16_SHR_U: u32 = SimdTag::I8x16ShrU as u32;
pub const SIMD_I16X8_SHL: u32 = SimdTag::I16x8Shl as u32;
pub const SIMD_I16X8_SHR_S: u32 = SimdTag::I16x8ShrS as u32;
pub const SIMD_I16X8_SHR_U: u32 = SimdTag::I16x8ShrU as u32;
pub const SIMD_I32X4_SHL: u32 = SimdTag::I32x4Shl as u32;
pub const SIMD_I32X4_SHR_S: u32 = SimdTag::I32x4ShrS as u32;
pub const SIMD_I32X4_SHR_U: u32 = SimdTag::I32x4ShrU as u32;
pub const SIMD_I64X2_SHL: u32 = SimdTag::I64x2Shl as u32;
pub const SIMD_I64X2_SHR_S: u32 = SimdTag::I64x2ShrS as u32;
pub const SIMD_I64X2_SHR_U: u32 = SimdTag::I64x2ShrU as u32;
pub const SIMD_V128_ANY_TRUE: u32 = SimdTag::V128AnyTrue as u32;
pub const SIMD_I8X16_ALL_TRUE: u32 = SimdTag::I8x16AllTrue as u32;
pub const SIMD_I8X16_BITMASK: u32 = SimdTag::I8x16Bitmask as u32;
pub const SIMD_I16X8_ALL_TRUE: u32 = SimdTag::I16x8AllTrue as u32;
pub const SIMD_I16X8_BITMASK: u32 = SimdTag::I16x8Bitmask as u32;
pub const SIMD_I32X4_ALL_TRUE: u32 = SimdTag::I32x4AllTrue as u32;
pub const SIMD_I32X4_BITMASK: u32 = SimdTag::I32x4Bitmask as u32;
pub const SIMD_I64X2_ALL_TRUE: u32 = SimdTag::I64x2AllTrue as u32;
pub const SIMD_I64X2_BITMASK: u32 = SimdTag::I64x2Bitmask as u32;
pub const SIMD_I8X16_POPCNT: u32 = SimdTag::I8x16Popcnt as u32;
pub const SIMD_I16X8_ABS: u32 = SimdTag::I16x8Abs as u32;
pub const SIMD_I16X8_NEG: u32 = SimdTag::I16x8Neg as u32;
pub const SIMD_I32X4_ABS: u32 = SimdTag::I32x4Abs as u32;
pub const SIMD_I32X4_NEG: u32 = SimdTag::I32x4Neg as u32;
pub const SIMD_I64X2_ABS: u32 = SimdTag::I64x2Abs as u32;
pub const SIMD_I64X2_NEG: u32 = SimdTag::I64x2Neg as u32;
pub const SIMD_F32X4_SQRT: u32 = SimdTag::F32x4Sqrt as u32;
pub const SIMD_F32X4_CEIL: u32 = SimdTag::F32x4Ceil as u32;
pub const SIMD_F32X4_FLOOR: u32 = SimdTag::F32x4Floor as u32;
pub const SIMD_F32X4_TRUNC: u32 = SimdTag::F32x4Trunc as u32;
pub const SIMD_F32X4_NEAREST: u32 = SimdTag::F32x4Nearest as u32;
pub const SIMD_F64X2_SQRT: u32 = SimdTag::F64x2Sqrt as u32;
pub const SIMD_F64X2_CEIL: u32 = SimdTag::F64x2Ceil as u32;
pub const SIMD_F64X2_FLOOR: u32 = SimdTag::F64x2Floor as u32;
pub const SIMD_F64X2_TRUNC: u32 = SimdTag::F64x2Trunc as u32;
pub const SIMD_F64X2_NEAREST: u32 = SimdTag::F64x2Nearest as u32;
pub const SIMD_I16X8_EXTEND_LOW_I8X16_S: u32 = SimdTag::I16x8ExtendLowI8x16S as u32;
pub const SIMD_I16X8_EXTEND_LOW_I8X16_U: u32 = SimdTag::I16x8ExtendLowI8x16U as u32;
pub const SIMD_I16X8_EXTEND_HIGH_I8X16_S: u32 = SimdTag::I16x8ExtendHighI8x16S as u32;
pub const SIMD_I16X8_EXTEND_HIGH_I8X16_U: u32 = SimdTag::I16x8ExtendHighI8x16U as u32;
pub const SIMD_I32X4_EXTEND_LOW_I16X8_S: u32 = SimdTag::I32x4ExtendLowI16x8S as u32;
pub const SIMD_I32X4_EXTEND_LOW_I16X8_U: u32 = SimdTag::I32x4ExtendLowI16x8U as u32;
pub const SIMD_I32X4_EXTEND_HIGH_I16X8_S: u32 = SimdTag::I32x4ExtendHighI16x8S as u32;
pub const SIMD_I32X4_EXTEND_HIGH_I16X8_U: u32 = SimdTag::I32x4ExtendHighI16x8U as u32;
pub const SIMD_I64X2_EXTEND_LOW_I32X4_S: u32 = SimdTag::I64x2ExtendLowI32x4S as u32;
pub const SIMD_I64X2_EXTEND_LOW_I32X4_U: u32 = SimdTag::I64x2ExtendLowI32x4U as u32;
pub const SIMD_I64X2_EXTEND_HIGH_I32X4_S: u32 = SimdTag::I64x2ExtendHighI32x4S as u32;
pub const SIMD_I64X2_EXTEND_HIGH_I32X4_U: u32 = SimdTag::I64x2ExtendHighI32x4U as u32;
pub const SIMD_I16X8_EXT_ADD_PAIRWISE_I8X16_S: u32 = SimdTag::I16x8ExtAddPairwiseI8x16S as u32;
pub const SIMD_I16X8_EXT_ADD_PAIRWISE_I8X16_U: u32 = SimdTag::I16x8ExtAddPairwiseI8x16U as u32;
pub const SIMD_I32X4_EXT_ADD_PAIRWISE_I16X8_S: u32 = SimdTag::I32x4ExtAddPairwiseI16x8S as u32;
pub const SIMD_I32X4_EXT_ADD_PAIRWISE_I16X8_U: u32 = SimdTag::I32x4ExtAddPairwiseI16x8U as u32;
pub const SIMD_I8X16_NARROW_I16X8_S: u32 = SimdTag::I8x16NarrowI16x8S as u32;
pub const SIMD_I8X16_NARROW_I16X8_U: u32 = SimdTag::I8x16NarrowI16x8U as u32;
pub const SIMD_I16X8_NARROW_I32X4_S: u32 = SimdTag::I16x8NarrowI32x4S as u32;
pub const SIMD_I16X8_NARROW_I32X4_U: u32 = SimdTag::I16x8NarrowI32x4U as u32;
pub const SIMD_I16X8_EXT_MUL_LOW_I8X16_S: u32 = SimdTag::I16x8ExtMulLowI8x16S as u32;
pub const SIMD_I16X8_EXT_MUL_LOW_I8X16_U: u32 = SimdTag::I16x8ExtMulLowI8x16U as u32;
pub const SIMD_I16X8_EXT_MUL_HIGH_I8X16_S: u32 = SimdTag::I16x8ExtMulHighI8x16S as u32;
pub const SIMD_I16X8_EXT_MUL_HIGH_I8X16_U: u32 = SimdTag::I16x8ExtMulHighI8x16U as u32;
pub const SIMD_I32X4_EXT_MUL_LOW_I16X8_S: u32 = SimdTag::I32x4ExtMulLowI16x8S as u32;
pub const SIMD_I32X4_EXT_MUL_LOW_I16X8_U: u32 = SimdTag::I32x4ExtMulLowI16x8U as u32;
pub const SIMD_I32X4_EXT_MUL_HIGH_I16X8_S: u32 = SimdTag::I32x4ExtMulHighI16x8S as u32;
pub const SIMD_I32X4_EXT_MUL_HIGH_I16X8_U: u32 = SimdTag::I32x4ExtMulHighI16x8U as u32;
pub const SIMD_I64X2_EXT_MUL_LOW_I32X4_S: u32 = SimdTag::I64x2ExtMulLowI32x4S as u32;
pub const SIMD_I64X2_EXT_MUL_LOW_I32X4_U: u32 = SimdTag::I64x2ExtMulLowI32x4U as u32;
pub const SIMD_I64X2_EXT_MUL_HIGH_I32X4_S: u32 = SimdTag::I64x2ExtMulHighI32x4S as u32;
pub const SIMD_I64X2_EXT_MUL_HIGH_I32X4_U: u32 = SimdTag::I64x2ExtMulHighI32x4U as u32;
pub const SIMD_I32X4_DOT_I16X8_S: u32 = SimdTag::I32x4DotI16x8S as u32;
pub const SIMD_I16X8_RELAXED_DOT_I8X16_I7X16_S: u32 = SimdTag::I16x8RelaxedDotI8x16I7x16S as u32;
pub const SIMD_I32X4_TRUNC_SAT_F32X4_S: u32 = SimdTag::I32x4TruncSatF32x4S as u32;
pub const SIMD_I32X4_TRUNC_SAT_F32X4_U: u32 = SimdTag::I32x4TruncSatF32x4U as u32;
pub const SIMD_F32X4_CONVERT_I32X4_S: u32 = SimdTag::F32x4ConvertI32x4S as u32;
pub const SIMD_F32X4_CONVERT_I32X4_U: u32 = SimdTag::F32x4ConvertI32x4U as u32;
pub const SIMD_I32X4_TRUNC_SAT_F64X2_SZERO: u32 = SimdTag::I32x4TruncSatF64x2SZero as u32;
pub const SIMD_I32X4_TRUNC_SAT_F64X2_UZERO: u32 = SimdTag::I32x4TruncSatF64x2UZero as u32;
pub const SIMD_F64X2_CONVERT_LOW_I32X4_S: u32 = SimdTag::F64x2ConvertLowI32x4S as u32;
pub const SIMD_F64X2_CONVERT_LOW_I32X4_U: u32 = SimdTag::F64x2ConvertLowI32x4U as u32;
pub const SIMD_F32X4_DEMOTE_F64X2_ZERO: u32 = SimdTag::F32x4DemoteF64x2Zero as u32;
pub const SIMD_F64X2_PROMOTE_LOW_F32X4: u32 = SimdTag::F64x2PromoteLowF32x4 as u32;
pub const SIMD_I32X4_RELAXED_TRUNC_F32X4_S: u32 = SimdTag::I32x4RelaxedTruncF32x4S as u32;
pub const SIMD_I32X4_RELAXED_TRUNC_F32X4_U: u32 = SimdTag::I32x4RelaxedTruncF32x4U as u32;
pub const SIMD_I32X4_RELAXED_TRUNC_F64X2_SZERO: u32 = SimdTag::I32x4RelaxedTruncF64x2SZero as u32;
pub const SIMD_I32X4_RELAXED_TRUNC_F64X2_UZERO: u32 = SimdTag::I32x4RelaxedTruncF64x2UZero as u32;

pub const SIMD_V128_LOAD64_ZERO: u32 = MemoryLoadTag::V128Load64Zero as u32;

