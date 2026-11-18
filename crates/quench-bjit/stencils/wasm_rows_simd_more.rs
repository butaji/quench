//! The b-jit's vector rows for the operators `wasm_rows_simd.rs` does not cover (kinds `SIMD_MORE_*`,
//! 60..=69; planned by `vm/compiled/wasm_stitch_simd_more.rs`). Holes and frame layout are
//! `wasm_rows_simd.rs`'s. Semantics are the staged rules on words: integer lanes as `wasm/simd/integer.rs`
//! and `wasm/simd.rs`, float lanes through `float_binary` and `operate_float_unary`, conversions through
//! `staged::wasm_float::convert` over `ConstantFree` (no float constant).

use super::simd::{mask, signed as sign_extend};
use super::wasm_rows_convert::ConstantFree;
use super::{StencilMemory, read, trap, write};
use crate::staged::machine::MachineStage;
use crate::staged::wasm_concrete::ConcreteWasm;
use crate::staged::wasm_float::{Float, float_binary};
use crate::staged::wasm_memory::{Access, check_access};
use crate::wasm::WasmType;
use crate::wasm::conversion::ScalarConversionOperator as Conversion;
use crate::wasm::float::{F64BinaryOperator as FloatBinary, F64UnaryOperator as FloatUnary};
use crate::wasm_codes::*;
use crate::words::{HOLE_A, HOLE_B, Out, S, slot};

/// Two narrow lanes make one wide lane (pairwise, widening, dot forms); Q15's fraction bits and rounding bias.
const PAIR: u32 = 2;
const Q15_SHIFT: u32 = i16::BITS - 1;
const Q15_ROUNDING: i64 = 1 << (Q15_SHIFT - 1);
/// The orderings a compare accepts (`order`'s mask).
const LESS: u8 = 0b001;
const EQUAL: u8 = 0b010;
const GREATER: u8 = 0b100;

/// The vector in the two words at hole `H`'s offset, as one 128-bit value.
#[inline(always)]
unsafe fn vread<const H: u8>(s: &S) -> u128 {
    unsafe {
        let at = slot::<H>(s) as *const u64;
        u128::from(*at) | (u128::from(*at.add(1)) << u64::BITS)
    }
}

/// Writes `bits` to the two words at hole `H`'s offset.
#[inline(always)]
unsafe fn vwrite<const H: u8>(s: &S, bits: u128) {
    unsafe {
        let at = slot::<H>(s) as *mut u64;
        *at = bits as u64;
        *at.add(1) = (bits >> u64::BITS) as u64;
    }
}

/// Lane `index` of `bits`, `width` bits wide, zero-extended.
#[inline(always)]
fn lane(bits: u128, width: u32, index: u32) -> u64 {
    (bits >> (width * index)) as u64 & mask(width)
}

/// `bits` with lane `index` set to the low `width` bits of `value`.
#[inline(always)]
fn put(bits: u128, width: u32, index: u32, value: u64) -> u128 {
    let field = u128::from(mask(width)) << (width * index);
    (bits & !field) | ((u128::from(value) << (width * index)) & field)
}

/// A `width`-bit lane sign-extended when `signed`, else zero-extended.
#[inline(always)]
fn value_of(bits: u64, width: u32, signed: bool) -> i64 {
    if signed {
        sign_extend(bits, width)
    } else {
        bits as i64
    }
}

/// `value` clamped to the `width`-bit lane's range, signed or unsigned, as the lane's bits.
#[inline(always)]
fn saturate(value: i64, width: u32, signed: bool) -> u64 {
    let (minimum, maximum) = if signed { (-(1i64 << (width - 1)), (1i64 << (width - 1)) - 1) } else { (0, mask(width) as i64) };
    value.clamp(minimum, maximum) as u64
}

/// Each lane pair of `left` and `right` through `operation`, `width` bits wide.
#[inline(always)]
fn lanes2(left: u128, right: u128, width: u32, operation: impl Fn(u64, u64) -> u64) -> u128 {
    let mut result = 0;
    let mut index = 0;
    while index < u128::BITS / width {
        result = put(result, width, index, operation(lane(left, width, index), lane(right, width, index)));
        index += 1;
    }
    result
}

/// Each lane of `value` through `operation`, `width` bits wide.
#[inline(always)]
fn lanes1(value: u128, width: u32, operation: impl Fn(u64) -> u64) -> u128 {
    let mut result = 0;
    let mut index = 0;
    while index < u128::BITS / width {
        result = put(result, width, index, operation(lane(value, width, index)));
        index += 1;
    }
    result
}

/// All ones in each lane where `accept` admits the lane pair's ordering (`compare`).
#[inline(always)]
fn order(left: u128, right: u128, width: u32, signed: bool, accept: u8) -> u128 {
    lanes2(left, right, width, |x, y| {
        let ordering = if signed { value_of(x, width, true).cmp(&value_of(y, width, true)) } else { x.cmp(&y) };
        let bit = [LESS, EQUAL, GREATER][(ordering as i8 + 1) as usize];
        if (accept & bit) != 0 { mask(width) } else { 0 }
    })
}

/// Each lane's minimum (or maximum) of the pair, `extrema`'s `binary_lane` compare.
#[inline(always)]
fn extrema(left: u128, right: u128, width: u32, signed: bool, greater: bool) -> u128 {
    lanes2(left, right, width, |x, y| {
        let (a, b) = (value_of(x, width, signed), value_of(y, width, signed));
        if (if greater { a > b } else { a < b }) { x } else { y }
    })
}

/// Each lane's saturating sum or difference (`saturating`).
#[inline(always)]
fn saturating(left: u128, right: u128, width: u32, signed: bool, subtract: bool) -> u128 {
    lanes2(left, right, width, |x, y| {
        let (a, b) = (value_of(x, width, signed), value_of(y, width, signed));
        saturate(if subtract { a - b } else { a + b }, width, signed)
    })
}

/// The bytes of `left` at `indices`; an index past the sixteen bytes reads zero (`swizzle`).
#[inline(always)]
fn swizzle(left: u128, indices: u128) -> u128 {
    let bytes = left.to_le_bytes();
    u128::from_le_bytes(indices.to_le_bytes().map(|index| bytes.get(usize::from(index)).copied().unwrap_or(0)))
}

/// Each lane's float min or max, with `float_binary`'s NaN rule (`float_binary`).
#[inline(always)]
fn float_extremum(left: u128, right: u128, width: u32, float: Float, operator: FloatBinary) -> u128 {
    // `float_binary`'s Minimum/Maximum rule (`wasm/float.rs`), written over bits: through
    // `float_binary` LLVM vectorizes the lanes with a constant-pool mask, which a stencil cannot
    // relocate, so the canonical NaN comes from an integer immediate instead.
    let minimum = matches!(operator, FloatBinary::Minimum);
    lanes2(left, right, width, |x, y| match float {
        Float::F32 => {
            let (a, b) = (f32::from_bits(x as u32), f32::from_bits(y as u32));
            u64::from(if a.is_nan() || b.is_nan() {
                core::hint::black_box(0x7fc0_0000_u32)
            } else if a == b {
                if minimum { (x | y) as u32 } else { (x & y) as u32 }
            } else if (a < b) == minimum {
                x as u32
            } else {
                y as u32
            })
        }
        Float::F64 => {
            let (a, b) = (f64::from_bits(x), f64::from_bits(y));
            if a.is_nan() || b.is_nan() {
                core::hint::black_box(0x7ff8_0000_0000_0000_u64)
            } else if a == b {
                if minimum { x | y } else { x & y }
            } else if (a < b) == minimum {
                x
            } else {
                y
            }
        }
    })
}

/// Each lane's pseudo-min or -max (`f32_pseudo`): the right lane where `comparison` holds, else the left;
/// the bits are the operands', not the comparison's.
#[inline(always)]
fn float_pick(left: u128, right: u128, width: u32, float: Float, comparison: FloatBinary) -> u128 {
    // The comparison as `float_binary` decides it (an ordered compare, false on NaN), written
    // directly: through `float_binary` the lanes vectorize with a constant-pool mask.
    let greater = matches!(comparison, FloatBinary::Greater);
    lanes2(left, right, width, |x, y| {
        let holds = match float {
            Float::F32 => {
                let (a, b) = (f32::from_bits(x as u32), f32::from_bits(y as u32));
                if greater { a > b } else { a < b }
            }
            Float::F64 => {
                let (a, b) = (f64::from_bits(x), f64::from_bits(y));
                if greater { a > b } else { a < b }
            }
        };
        if holds { y } else { x }
    })
}

/// 1 where every lane of `value` is nonzero (`all_true`).
#[inline(always)]
fn all_true(value: u128, width: u32) -> u64 {
    u64::from((0..u128::BITS / width).all(|index| lane(value, width, index) != 0))
}

/// Each lane's sign bit gathered into the low bits, lane 0 lowest (`bitmask`).
#[inline(always)]
fn bitmask(value: u128, width: u32) -> u64 {
    (0..u128::BITS / width).fold(0, |bits, index| bits | ((lane(value, width, index) >> (width - 1)) << index))
}

/// The lanes of `value`'s low (or high) half, each widened to `width` bits (`extend`).
#[inline(always)]
fn extend(value: u128, width: u32, signed: bool, high: bool) -> u128 {
    let (source, first) = (width / PAIR, if high { u128::BITS / width } else { 0 });
    let mut result = 0;
    let mut index = 0;
    while index < u128::BITS / width {
        result = put(result, width, index, value_of(lane(value, source, first + index), source, signed) as u64);
        index += 1;
    }
    result
}

/// Each lane is the sum of an adjacent narrow pair of `value`, widened (`pairwise_add`).
#[inline(always)]
fn pairwise_add(value: u128, width: u32, signed: bool) -> u128 {
    let source = width / PAIR;
    let mut result = 0;
    let mut index = 0;
    while index < u128::BITS / width {
        let (a, b) = (lane(value, source, PAIR * index), lane(value, source, PAIR * index + 1));
        result = put(result, width, index, (value_of(a, source, signed) + value_of(b, source, signed)) as u64);
        index += 1;
    }
    result
}

/// `left`'s then `right`'s lanes, each saturated to `width` bits (`narrow`).
#[inline(always)]
fn narrow(left: u128, right: u128, width: u32, signed: bool) -> u128 {
    let (source, count) = (width * PAIR, u128::BITS / (width * PAIR));
    let mut result = 0;
    let mut index = 0;
    while index < u128::BITS / width {
        let bits = if index < count { left } else { right };
        result = put(result, width, index, saturate(value_of(lane(bits, source, index % count), source, true), width, signed));
        index += 1;
    }
    result
}

/// Each wide lane is the product of one half's narrow lanes of `left` and `right` (`extend_multiply`).
#[inline(always)]
fn extmul(left: u128, right: u128, width: u32, signed: bool, high: bool) -> u128 {
    let (source, first) = (width / PAIR, if high { u128::BITS / width } else { 0 });
    let mut result = 0;
    let mut index = 0;
    while index < u128::BITS / width {
        let a = value_of(lane(left, source, first + index), source, signed) as u64;
        let b = value_of(lane(right, source, first + index), source, signed) as u64;
        result = put(result, width, index, a.wrapping_mul(b));
        index += 1;
    }
    result
}

/// Each i32 lane is the wrapping sum of two i16 products (`dot`).
#[inline(always)]
fn dot(left: u128, right: u128) -> u128 {
    let product = |index: u32| value_of(lane(left, 16, index), 16, true) as i32 * value_of(lane(right, 16, index), 16, true) as i32;
    let mut result = 0;
    let mut index = 0;
    while index < u128::BITS / 32 {
        result = put(result, 32, index, product(PAIR * index).wrapping_add(product(PAIR * index + 1)) as u32 as u64);
        index += 1;
    }
    result
}

/// Each i16 lane is the saturated sum of two i8 products (`relaxed_dot`).
#[inline(always)]
fn relaxed_dot(left: u128, right: u128) -> u128 {
    let mut result = 0;
    let mut index = 0;
    while index < u128::BITS / 16 {
        let mut sum = 0;
        for source in [PAIR * index, PAIR * index + 1] {
            sum += value_of(lane(left, 8, source), 8, true) * value_of(lane(right, 8, source), 8, true);
        }
        result = put(result, 16, index, saturate(sum, 16, true));
        index += 1;
    }
    result
}

/// `value`'s lanes, `source` bits wide, converted by the scalar rule into `result`-bit lanes; the lanes
/// past the conversion's count stay zero.
#[inline(always)]
fn convert_lanes(value: u128, operator: Conversion, source: u32, result: u32) -> u128 {
    let mut out = 0;
    let mut index = 0;
    while index < u128::BITS / source.max(result) {
        out = put(out, result, index, MachineStage::convert(&mut ConstantFree, operator, lane(value, source, index)));
        index += 1;
    }
    out
}

/// A conversion row's scalar conversion, source width and result width.
const fn conversion(operator: u32) -> (Conversion, u32, u32) {
    match operator {
        SIMD_I32X4_TRUNC_SAT_F32X4_S | SIMD_I32X4_RELAXED_TRUNC_F32X4_S => (Conversion::SaturateF32SignedI32, 32, 32),
        SIMD_I32X4_TRUNC_SAT_F32X4_U | SIMD_I32X4_RELAXED_TRUNC_F32X4_U => (Conversion::SaturateF32UnsignedI32, 32, 32),
        SIMD_F32X4_CONVERT_I32X4_S => (Conversion::ConvertI32SignedF32, 32, 32), SIMD_F32X4_CONVERT_I32X4_U => (Conversion::ConvertI32UnsignedF32, 32, 32),
        SIMD_I32X4_TRUNC_SAT_F64X2_SZERO | SIMD_I32X4_RELAXED_TRUNC_F64X2_SZERO => (Conversion::SaturateF64SignedI32, 64, 32),
        SIMD_I32X4_TRUNC_SAT_F64X2_UZERO | SIMD_I32X4_RELAXED_TRUNC_F64X2_UZERO => (Conversion::SaturateF64UnsignedI32, 64, 32),
        SIMD_F64X2_CONVERT_LOW_I32X4_S => (Conversion::ConvertI32SignedF64, 32, 64),
        SIMD_F64X2_CONVERT_LOW_I32X4_U => (Conversion::ConvertI32UnsignedF64, 32, 64), SIMD_F32X4_DEMOTE_F64X2_ZERO => (Conversion::DemoteF64, 64, 32),
        SIMD_F64X2_PROMOTE_LOW_F32X4 => (Conversion::PromoteF32, 32, 64),
        _ => (Conversion::WrapI64, 64, 32),
    }
}

const fn load_bytes(operator: u32) -> usize {
    match operator {
        SIMD_V128_LOAD8_SPLAT => 1,
        SIMD_V128_LOAD16_SPLAT => 2,
        SIMD_V128_LOAD32_SPLAT | SIMD_V128_LOAD32_ZERO => 4,
        _ => 8,
    }
}

/// The vector a load of `OP` makes from its bytes' word: splatted, widened or zeroed.
#[inline(always)]
fn load_value(operator: u32, word: u64) -> u128 {
    match operator {
        SIMD_V128_LOAD8X8_S => extend(u128::from(word), 16, true, false), SIMD_V128_LOAD8X8_U => extend(u128::from(word), 16, false, false),
        SIMD_V128_LOAD16X4_S => extend(u128::from(word), 32, true, false), SIMD_V128_LOAD16X4_U => extend(u128::from(word), 32, false, false),
        SIMD_V128_LOAD32X2_S => extend(u128::from(word), 64, true, false), SIMD_V128_LOAD32X2_U => extend(u128::from(word), 64, false, false),
        SIMD_V128_LOAD8_SPLAT => lanes1(0, 8, |_| word), SIMD_V128_LOAD16_SPLAT => lanes1(0, 16, |_| word), SIMD_V128_LOAD32_SPLAT => lanes1(0, 32, |_| word),
        SIMD_V128_LOAD64_SPLAT => lanes1(0, 64, |_| word), SIMD_V128_LOAD32_ZERO => u128::from(word as u32), SIMD_V128_LOAD64_ZERO => u128::from(word),
        _ => 0,
    }
}

/// `D = A op B` over the lanes of two vectors (`binary`: min/max, saturating, averages, Q15, swizzle;
/// `compare`: ordering; `float`: min, max, pseudo-min and -max; `wide`: narrow, extended multiply, dot).
macro_rules! lanewise {
    ($name:ident, $l:ident, $r:ident, $($arm:tt)*) => {
        #[inline(always)]
        pub(crate) unsafe fn $name<const OP: u32>(s: S) -> Out {
            unsafe {
                let ($l, $r) = (vread::<HOLE_A>(&s), vread::<HOLE_B>(&s));
                vwrite::<0>(&s, match OP { $($arm)* _ => 0 });
            }
            Out::Next(s)
        }
    };
}

/// `D = op A` over the lanes of one vector (`unary`: popcount, abs, neg, float rounding; `widen`: extends).
macro_rules! unary_lanes {
    ($name:ident, $v:ident, $($arm:tt)*) => {
        #[inline(always)]
        pub(crate) unsafe fn $name<const OP: u32>(s: S) -> Out {
            unsafe {
                let $v = vread::<HOLE_A>(&s);
                vwrite::<0>(&s, match OP { $($arm)* _ => 0 });
            }
            Out::Next(s)
        }
    };
}

lanewise! { binary, left, right,
    SIMD_I8X16_MIN_S => extrema(left, right, 8, true, false), SIMD_I8X16_MIN_U => extrema(left, right, 8, false, false),
    SIMD_I8X16_MAX_S => extrema(left, right, 8, true, true), SIMD_I8X16_MAX_U => extrema(left, right, 8, false, true),
    SIMD_I16X8_MIN_S => extrema(left, right, 16, true, false), SIMD_I16X8_MIN_U => extrema(left, right, 16, false, false),
    SIMD_I16X8_MAX_S => extrema(left, right, 16, true, true), SIMD_I16X8_MAX_U => extrema(left, right, 16, false, true),
    SIMD_I32X4_MIN_S => extrema(left, right, 32, true, false), SIMD_I32X4_MIN_U => extrema(left, right, 32, false, false),
    SIMD_I32X4_MAX_S => extrema(left, right, 32, true, true), SIMD_I32X4_MAX_U => extrema(left, right, 32, false, true),
    SIMD_I8X16_ADD_SAT_S => saturating(left, right, 8, true, false), SIMD_I8X16_ADD_SAT_U => saturating(left, right, 8, false, false),
    SIMD_I8X16_SUB_SAT_S => saturating(left, right, 8, true, true), SIMD_I8X16_SUB_SAT_U => saturating(left, right, 8, false, true),
    SIMD_I16X8_ADD_SAT_S => saturating(left, right, 16, true, false), SIMD_I16X8_ADD_SAT_U => saturating(left, right, 16, false, false),
    SIMD_I16X8_SUB_SAT_S => saturating(left, right, 16, true, true), SIMD_I16X8_SUB_SAT_U => saturating(left, right, 16, false, true),
    SIMD_I8X16_AVGR_U => lanes2(left, right, 8, |x, y| (x + y).div_ceil(u64::from(PAIR))),
    SIMD_I16X8_AVGR_U => lanes2(left, right, 16, |x, y| (x + y).div_ceil(u64::from(PAIR))),
    SIMD_I16X8_Q15_MULR_SAT_S => lanes2(left, right, 16, |x, y| saturate((value_of(x, 16, true) * value_of(y, 16, true) + Q15_ROUNDING) >> Q15_SHIFT, 16, true)),
    SIMD_I16X8_RELAXED_Q15MULR_S => lanes2(left, right, 16, |x, y| saturate((value_of(x, 16, true) * value_of(y, 16, true) + Q15_ROUNDING) >> Q15_SHIFT, 16, true)),
    SIMD_I8X16_RELAXED_SWIZZLE => swizzle(left, right),
}
lanewise! { compare, left, right,
    SIMD_I16X8_LT_S => order(left, right, 16, true, LESS), SIMD_I16X8_GT_S => order(left, right, 16, true, GREATER),
    SIMD_I16X8_LE_S => order(left, right, 16, true, LESS | EQUAL), SIMD_I16X8_GE_S => order(left, right, 16, true, GREATER | EQUAL),
    SIMD_I16X8_LT_U => order(left, right, 16, false, LESS), SIMD_I16X8_GT_U => order(left, right, 16, false, GREATER),
    SIMD_I16X8_LE_U => order(left, right, 16, false, LESS | EQUAL), SIMD_I16X8_GE_U => order(left, right, 16, false, GREATER | EQUAL),
    SIMD_I32X4_LT_S => order(left, right, 32, true, LESS), SIMD_I32X4_GT_S => order(left, right, 32, true, GREATER),
    SIMD_I32X4_LE_S => order(left, right, 32, true, LESS | EQUAL), SIMD_I32X4_GE_S => order(left, right, 32, true, GREATER | EQUAL),
    SIMD_I32X4_LT_U => order(left, right, 32, false, LESS), SIMD_I32X4_GT_U => order(left, right, 32, false, GREATER),
    SIMD_I32X4_LE_U => order(left, right, 32, false, LESS | EQUAL), SIMD_I32X4_GE_U => order(left, right, 32, false, GREATER | EQUAL),
    SIMD_I64X2_EQ => order(left, right, 64, false, EQUAL), SIMD_I64X2_NE => order(left, right, 64, false, LESS | GREATER),
    SIMD_I64X2_LT_S => order(left, right, 64, true, LESS), SIMD_I64X2_GT_S => order(left, right, 64, true, GREATER),
    SIMD_I64X2_LE_S => order(left, right, 64, true, LESS | EQUAL), SIMD_I64X2_GE_S => order(left, right, 64, true, GREATER | EQUAL),
}
lanewise! { float, left, right,
    SIMD_F32X4_MIN => float_extremum(left, right, 32, Float::F32, FloatBinary::Minimum),
    SIMD_F32X4_MAX => float_extremum(left, right, 32, Float::F32, FloatBinary::Maximum),
    SIMD_F32X4_PMIN => float_pick(left, right, 32, Float::F32, FloatBinary::Greater),
    SIMD_F32X4_PMAX => float_pick(left, right, 32, Float::F32, FloatBinary::Less),
    SIMD_F32X4_RELAXED_MIN => float_extremum(left, right, 32, Float::F32, FloatBinary::Minimum),
    SIMD_F32X4_RELAXED_MAX => float_extremum(left, right, 32, Float::F32, FloatBinary::Maximum),
    SIMD_F64X2_MIN => float_extremum(left, right, 64, Float::F64, FloatBinary::Minimum),
    SIMD_F64X2_MAX => float_extremum(left, right, 64, Float::F64, FloatBinary::Maximum),
    SIMD_F64X2_PMIN => float_pick(left, right, 64, Float::F64, FloatBinary::Greater),
    SIMD_F64X2_PMAX => float_pick(left, right, 64, Float::F64, FloatBinary::Less),
    SIMD_F64X2_RELAXED_MIN => float_extremum(left, right, 64, Float::F64, FloatBinary::Minimum),
    SIMD_F64X2_RELAXED_MAX => float_extremum(left, right, 64, Float::F64, FloatBinary::Maximum),
}
lanewise! { wide, left, right,
    SIMD_I8X16_NARROW_I16X8_S => narrow(left, right, 8, true), SIMD_I8X16_NARROW_I16X8_U => narrow(left, right, 8, false),
    SIMD_I16X8_NARROW_I32X4_S => narrow(left, right, 16, true), SIMD_I16X8_NARROW_I32X4_U => narrow(left, right, 16, false),
    SIMD_I16X8_EXT_MUL_LOW_I8X16_S => extmul(left, right, 16, true, false), SIMD_I16X8_EXT_MUL_LOW_I8X16_U => extmul(left, right, 16, false, false),
    SIMD_I16X8_EXT_MUL_HIGH_I8X16_S => extmul(left, right, 16, true, true), SIMD_I16X8_EXT_MUL_HIGH_I8X16_U => extmul(left, right, 16, false, true),
    SIMD_I32X4_EXT_MUL_LOW_I16X8_S => extmul(left, right, 32, true, false), SIMD_I32X4_EXT_MUL_LOW_I16X8_U => extmul(left, right, 32, false, false),
    SIMD_I32X4_EXT_MUL_HIGH_I16X8_S => extmul(left, right, 32, true, true), SIMD_I32X4_EXT_MUL_HIGH_I16X8_U => extmul(left, right, 32, false, true),
    SIMD_I64X2_EXT_MUL_LOW_I32X4_S => extmul(left, right, 64, true, false), SIMD_I64X2_EXT_MUL_LOW_I32X4_U => extmul(left, right, 64, false, false),
    SIMD_I64X2_EXT_MUL_HIGH_I32X4_S => extmul(left, right, 64, true, true), SIMD_I64X2_EXT_MUL_HIGH_I32X4_U => extmul(left, right, 64, false, true),
    SIMD_I32X4_DOT_I16X8_S => dot(left, right), SIMD_I16X8_RELAXED_DOT_I8X16_I7X16_S => relaxed_dot(left, right),
}
unary_lanes! { unary, value,
    SIMD_I8X16_POPCNT => lanes1(value, 8, |x| u64::from(x.count_ones())),
    SIMD_I16X8_ABS => lanes1(value, 16, |x| if value_of(x, 16, true) < 0 { 0u64.wrapping_sub(x) } else { x }),
    SIMD_I16X8_NEG => lanes1(value, 16, |x| 0u64.wrapping_sub(x)),
    SIMD_I32X4_ABS => lanes1(value, 32, |x| if value_of(x, 32, true) < 0 { 0u64.wrapping_sub(x) } else { x }),
    SIMD_I32X4_NEG => lanes1(value, 32, |x| 0u64.wrapping_sub(x)),
    SIMD_I64X2_ABS => lanes1(value, 64, |x| if value_of(x, 64, true) < 0 { 0u64.wrapping_sub(x) } else { x }),
    SIMD_I64X2_NEG => lanes1(value, 64, |x| 0u64.wrapping_sub(x)),
    SIMD_F32X4_SQRT => lanes1(value, 32, |x| ConcreteWasm.operate_float_unary(FloatUnary::SquareRoot, Float::F32, x)),
    SIMD_F32X4_CEIL => lanes1(value, 32, |x| ConcreteWasm.operate_float_unary(FloatUnary::Ceiling, Float::F32, x)),
    SIMD_F32X4_FLOOR => lanes1(value, 32, |x| ConcreteWasm.operate_float_unary(FloatUnary::Floor, Float::F32, x)),
    SIMD_F32X4_TRUNC => lanes1(value, 32, |x| ConcreteWasm.operate_float_unary(FloatUnary::Truncate, Float::F32, x)),
    SIMD_F32X4_NEAREST => lanes1(value, 32, |x| ConcreteWasm.operate_float_unary(FloatUnary::Nearest, Float::F32, x)),
    SIMD_F64X2_SQRT => lanes1(value, 64, |x| ConcreteWasm.operate_float_unary(FloatUnary::SquareRoot, Float::F64, x)),
    SIMD_F64X2_CEIL => lanes1(value, 64, |x| ConcreteWasm.operate_float_unary(FloatUnary::Ceiling, Float::F64, x)),
    SIMD_F64X2_FLOOR => lanes1(value, 64, |x| ConcreteWasm.operate_float_unary(FloatUnary::Floor, Float::F64, x)),
    SIMD_F64X2_TRUNC => lanes1(value, 64, |x| ConcreteWasm.operate_float_unary(FloatUnary::Truncate, Float::F64, x)),
    SIMD_F64X2_NEAREST => lanes1(value, 64, |x| ConcreteWasm.operate_float_unary(FloatUnary::Nearest, Float::F64, x)),
}
unary_lanes! { widen, value,
    SIMD_I16X8_EXTEND_LOW_I8X16_S => extend(value, 16, true, false), SIMD_I16X8_EXTEND_LOW_I8X16_U => extend(value, 16, false, false),
    SIMD_I16X8_EXTEND_HIGH_I8X16_S => extend(value, 16, true, true), SIMD_I16X8_EXTEND_HIGH_I8X16_U => extend(value, 16, false, true),
    SIMD_I32X4_EXTEND_LOW_I16X8_S => extend(value, 32, true, false), SIMD_I32X4_EXTEND_LOW_I16X8_U => extend(value, 32, false, false),
    SIMD_I32X4_EXTEND_HIGH_I16X8_S => extend(value, 32, true, true), SIMD_I32X4_EXTEND_HIGH_I16X8_U => extend(value, 32, false, true),
    SIMD_I64X2_EXTEND_LOW_I32X4_S => extend(value, 64, true, false), SIMD_I64X2_EXTEND_LOW_I32X4_U => extend(value, 64, false, false),
    SIMD_I64X2_EXTEND_HIGH_I32X4_S => extend(value, 64, true, true), SIMD_I64X2_EXTEND_HIGH_I32X4_U => extend(value, 64, false, true),
    SIMD_I16X8_EXT_ADD_PAIRWISE_I8X16_S => pairwise_add(value, 16, true), SIMD_I16X8_EXT_ADD_PAIRWISE_I8X16_U => pairwise_add(value, 16, false),
    SIMD_I32X4_EXT_ADD_PAIRWISE_I16X8_S => pairwise_add(value, 32, true), SIMD_I32X4_EXT_ADD_PAIRWISE_I16X8_U => pairwise_add(value, 32, false),
}

/// `D = A` shifted per lane by the scalar count in `HOLE_B`.
#[inline(always)]
pub(crate) unsafe fn shift<const OP: u32>(s: S) -> Out {
    unsafe {
        let (value, count) = (vread::<HOLE_A>(&s), read::<HOLE_B>(&s) as u32);
        let result = match OP {
            SIMD_I8X16_SHL => lanes1(value, 8, |x| x << (count % 8)), SIMD_I8X16_SHR_S => lanes1(value, 8, |x| (value_of(x, 8, true) >> (count % 8)) as u64),
            SIMD_I8X16_SHR_U => lanes1(value, 8, |x| x >> (count % 8)), SIMD_I16X8_SHL => lanes1(value, 16, |x| x << (count % 16)),
            SIMD_I16X8_SHR_S => lanes1(value, 16, |x| (value_of(x, 16, true) >> (count % 16)) as u64),
            SIMD_I16X8_SHR_U => lanes1(value, 16, |x| x >> (count % 16)), SIMD_I32X4_SHL => lanes1(value, 32, |x| x << (count % 32)),
            SIMD_I32X4_SHR_S => lanes1(value, 32, |x| (value_of(x, 32, true) >> (count % 32)) as u64),
            SIMD_I32X4_SHR_U => lanes1(value, 32, |x| x >> (count % 32)), SIMD_I64X2_SHL => lanes1(value, 64, |x| x << (count % 64)),
            SIMD_I64X2_SHR_S => lanes1(value, 64, |x| (value_of(x, 64, true) >> (count % 64)) as u64),
            SIMD_I64X2_SHR_U => lanes1(value, 64, |x| x >> (count % 64)),
            _ => 0,
        };
        vwrite::<0>(&s, result);
    }
    Out::Next(s)
}

/// `D` (a scalar word) = the any, all or bitmask test of vector `A`.
#[inline(always)]
pub(crate) unsafe fn test<const OP: u32>(s: S) -> Out {
    unsafe {
        let value = vread::<HOLE_A>(&s);
        let word = match OP {
            SIMD_V128_ANY_TRUE => u64::from(value != 0), SIMD_I8X16_ALL_TRUE => all_true(value, 8), SIMD_I8X16_BITMASK => bitmask(value, 8),
            SIMD_I16X8_ALL_TRUE => all_true(value, 16), SIMD_I16X8_BITMASK => bitmask(value, 16), SIMD_I32X4_ALL_TRUE => all_true(value, 32),
            SIMD_I32X4_BITMASK => bitmask(value, 32), SIMD_I64X2_ALL_TRUE => all_true(value, 64), SIMD_I64X2_BITMASK => bitmask(value, 64),
            _ => 0,
        };
        write::<0>(&s, word);
    }
    Out::Next(s)
}

/// `D = op A` converting lanes between int and float widths, trapping or saturating.
#[inline(always)]
pub(crate) unsafe fn convert<const OP: u32>(s: S) -> Out {
    unsafe {
        let (operator, source, result) = const { conversion(OP) };
        vwrite::<0>(&s, convert_lanes(vread::<HOLE_A>(&s), operator, source, result));
    }
    Out::Next(s)
}

/// `D = v128.load` of the form `OP` at the 64-bit address in `HOLE_A`; traps out of bounds, writing nothing.
#[inline(always)]
pub(crate) unsafe fn load<const OP: u32>(s: S) -> Out {
    let index = unsafe { read::<HOLE_A>(&s) };
    let bytes = const { load_bytes(OP) };
    let mut memory = StencilMemory::of(&s);
    match check_access(&mut memory, Access { ty: WasmType::V128, bytes, signed: false }, index, 0, false) {
        Ok(()) => {
            let mut raw = [0u8; 8];
            let word = unsafe {
                core::ptr::copy_nonoverlapping(memory.at(index, 0), raw.as_mut_ptr(), bytes);
                u64::from_le_bytes(raw)
            };
            unsafe { vwrite::<0>(&s, load_value(OP, word)) };
            Out::Next(s)
        }
        Err(error) => trap(s, error),
    }
}
