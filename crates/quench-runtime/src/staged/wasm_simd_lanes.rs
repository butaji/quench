//! The SIMD operators' semantics on bits, written once for both tiers: the interpreter (`wasm/simd.rs`
//! wraps it at the `WasmValue` boundary) and the b-jit's vector rows (`quench-bjit/stencils`), which
//! compile this file as a stencil. Vectors are `u128`, scalars the low bits of a `u128`, and nothing
//! here names `WasmValue`, `wasmparser`, an allocation or a table of function pointers: a row takes
//! `SimdOperator::from_tag(OP)` in a const block, so its body folds to the one operator it runs.
//!
//! The operator list (`simd_operator_list!`) is the one place an operator's tag, lane width, operand
//! and result types and body are written. The enum and its tag lookup are generated from it here, and
//! `wasm/simd.rs` generates the wasmparser mapping and the kernel table from the same list.

use crate::staged::machine::MachineStage;
use crate::staged::wasm_concrete::ConcreteWasm;
use crate::staged::wasm_float::{Float, float_binary};
use crate::wasm::conversion::ScalarConversionOperator as Conversion;
use crate::wasm::float::{F64BinaryOperator as FloatBinary, F64UnaryOperator as FloatUnary};

/// The Q15 rounding bias: half of the fraction's unit, 2^14.
const Q15_ROUNDING: i64 = 1 << 14;

/// `width`-bit lane mask, as a v128 value.
#[inline(always)]
pub(crate) const fn lane_mask(width: u32) -> u128 {
    u128::MAX >> (u128::BITS - width)
}

/// `width`-bit lane mask, as a scalar.
#[inline(always)]
const fn mask64(width: u32) -> u64 {
    u64::MAX >> (u64::BITS - width)
}

/// The lanes of a `width`-bit lane in a v128 (a power of two; no division).
#[inline(always)]
const fn lanes_of(width: u32) -> u32 {
    128 >> width.trailing_zeros()
}

/// Lane `index` of `bits`, `width` bits wide, zero-extended.
#[inline(always)]
pub(crate) fn lane(bits: u128, width: u32, index: u8) -> u64 {
    ((bits >> (width * u32::from(index))) & lane_mask(width)) as u64
}

/// `bits` with lane `index` set to the low `width` bits of `value`.
#[inline(always)]
pub(crate) fn replace(bits: u128, width: u32, index: u8, value: u64) -> u128 {
    let shift = width * u32::from(index);
    let mask = lane_mask(width);
    (bits & !(mask << shift)) | ((u128::from(value) & mask) << shift)
}

/// The `width`-bit lane `bits` holds, read as two's complement and sign-extended.
#[inline(always)]
pub(crate) fn signed(bits: u64, width: u32) -> i64 {
    let shift = u64::BITS - width;
    ((bits << shift) as i64) >> shift
}

/// The `width`-bit lane `bits` holds as an integer: signed when `sign`, else zero-extended.
#[inline(always)]
fn lane_as(bits: u64, width: u32, sign: bool) -> i64 {
    if sign { signed(bits, width) } else { bits as i64 }
}

/// All ones in a `width`-bit lane where `holds`, zero where not.
#[inline(always)]
fn when(holds: bool, width: u32) -> u64 {
    if holds { mask64(width) } else { 0 }
}

/// `value` clamped to the `width`-bit lane's range (signed or unsigned), as the lane's bits.
#[inline(always)]
fn saturate(value: i64, width: u32, sign: bool) -> u64 {
    let (low, high) = if sign {
        let bound = 1i64 << (width - 1);
        (-bound, bound - 1)
    } else {
        (0, mask64(width) as i64)
    };
    (if value < low {
        low
    } else if value > high {
        high
    } else {
        value
    }) as u64
}

/// Each lane of `bits` through `operation`, `width` bits wide.
#[inline(always)]
fn each(bits: u128, width: u32, operation: impl Fn(u64) -> u64) -> u128 {
    let mut result = 0;
    let mut index = 0;
    while index < lanes_of(width) {
        let at = index as u8;
        result = replace(result, width, at, operation(lane(bits, width, at)));
        index += 1;
    }
    result
}

/// Each lane pair of `left` and `right` through `operation`, `width` bits wide.
#[inline(always)]
fn zip(left: u128, right: u128, width: u32, operation: impl Fn(u64, u64) -> u64) -> u128 {
    let mut result = 0;
    let mut index = 0;
    while index < lanes_of(width) {
        let at = index as u8;
        result = replace(result, width, at, operation(lane(left, width, at), lane(right, width, at)));
        index += 1;
    }
    result
}

/// Each lane pair where `holds` is all ones, the rest zero.
#[inline(always)]
fn compare(left: u128, right: u128, width: u32, holds: impl Fn(u64, u64) -> bool) -> u128 {
    zip(left, right, width, |x, y| when(holds(x, y), width))
}

/// Every lane set to `value`'s low `width` bits.
#[inline(always)]
fn splat(value: u64, width: u32) -> u128 {
    each(0, width, |_| value)
}

/// Each lane negated (wrapping).
#[inline(always)]
fn negate(bits: u128, width: u32) -> u128 {
    each(bits, width, |x| 0u64.wrapping_sub(x))
}

/// Each lane's magnitude (wrapping at the minimum).
#[inline(always)]
fn absolute(bits: u128, width: u32) -> u128 {
    each(bits, width, |x| if signed(x, width) < 0 { 0u64.wrapping_sub(x) } else { x })
}

/// 1 where every lane is nonzero.
#[inline(always)]
fn all_true(bits: u128, width: u32) -> u128 {
    let mut all = true;
    let mut index = 0;
    while index < lanes_of(width) {
        all = all && lane(bits, width, index as u8) != 0;
        index += 1;
    }
    u128::from(all)
}

/// Each lane's sign bit gathered into the low bits, lane 0 lowest.
#[inline(always)]
fn bitmask(bits: u128, width: u32) -> u128 {
    let mut result: u128 = 0;
    let mut index = 0;
    while index < lanes_of(width) {
        result |= u128::from(lane(bits, width, index as u8) >> (width - 1)) << index;
        index += 1;
    }
    result
}

/// Each byte's population count.
#[inline(always)]
fn population_count(bits: u128) -> u128 {
    each(bits, 8, |x| u64::from(x.count_ones()))
}

/// Each lane shifted left by `count` modulo the lane width.
#[inline(always)]
fn shift_left(bits: u128, count: u128, width: u32) -> u128 {
    let amount = (count as u32) & (width - 1);
    each(bits, width, |x| x << amount)
}

/// Each lane shifted right, zero-filled, by `count` modulo the lane width.
#[inline(always)]
fn shift_right_unsigned(bits: u128, count: u128, width: u32) -> u128 {
    let amount = (count as u32) & (width - 1);
    each(bits, width, |x| x >> amount)
}

/// Each lane shifted right, sign-filled, by `count` modulo the lane width.
#[inline(always)]
fn shift_right_signed(bits: u128, count: u128, width: u32) -> u128 {
    let amount = (count as u32) & (width - 1);
    each(bits, width, |x| (signed(x, width) >> amount) as u64)
}

/// Each lane's minimum (`greater` false) or maximum (`greater` true) of the pair.
#[inline(always)]
fn extrema(left: u128, right: u128, width: u32, sign: bool, greater: bool) -> u128 {
    zip(left, right, width, |x, y| {
        let (a, b) = (lane_as(x, width, sign), lane_as(y, width, sign));
        if (if greater { a > b } else { a < b }) { x } else { y }
    })
}

/// Each lane's saturating sum or difference of the pair.
#[inline(always)]
fn saturating(left: u128, right: u128, width: u32, sign: bool, subtract: bool) -> u128 {
    zip(left, right, width, |x, y| {
        let (a, b) = (lane_as(x, width, sign), lane_as(y, width, sign));
        saturate(if subtract { a - b } else { a + b }, width, sign)
    })
}

/// Each lane's unsigned rounding average of the pair.
#[inline(always)]
fn average(left: u128, right: u128, width: u32) -> u128 {
    zip(left, right, width, |x, y| (x + y + 1) >> 1)
}

/// Q15 fixed-point multiply with rounding and saturation, per i16 lane.
#[inline(always)]
fn q15(left: u128, right: u128) -> u128 {
    zip(left, right, 16, |x, y| {
        let product = signed(x, 16) * signed(y, 16);
        saturate((product + Q15_ROUNDING) >> 15, 16, true)
    })
}

/// `left`'s then `right`'s lanes, each signed source lane saturated to `width` bits.
#[inline(always)]
fn narrow(left: u128, right: u128, width: u32, sign: bool) -> u128 {
    let source = width * 2;
    let count = lanes_of(source);
    let mut result = 0;
    let mut index = 0;
    while index < lanes_of(width) {
        let bits = if index < count { left } else { right };
        let value = signed(lane(bits, source, (index & (count - 1)) as u8), source);
        result = replace(result, width, index as u8, saturate(value, width, sign));
        index += 1;
    }
    result
}

/// The lanes of `bits`' low (or high) half, each widened to `width` bits.
#[inline(always)]
fn extend(bits: u128, width: u32, sign: bool, high: bool) -> u128 {
    let source = width / 2;
    let first = if high { lanes_of(width) } else { 0 };
    let mut result = 0;
    let mut index = 0;
    while index < lanes_of(width) {
        let value = lane_as(lane(bits, source, (first + index) as u8), source, sign);
        result = replace(result, width, index as u8, value as u64);
        index += 1;
    }
    result
}

/// Each wide lane is the product of one half's narrow lanes of `left` and `right`.
#[inline(always)]
fn extend_multiply(left: u128, right: u128, width: u32, sign: bool, high: bool) -> u128 {
    let source = width / 2;
    let first = if high { lanes_of(width) } else { 0 };
    let mut result = 0;
    let mut index = 0;
    while index < lanes_of(width) {
        let at = (first + index) as u8;
        let a = lane_as(lane(left, source, at), source, sign) as u64;
        let b = lane_as(lane(right, source, at), source, sign) as u64;
        result = replace(result, width, index as u8, a.wrapping_mul(b));
        index += 1;
    }
    result
}

/// Each lane is the sum of an adjacent narrow pair of `bits`, widened.
#[inline(always)]
fn pairwise_add(bits: u128, width: u32, sign: bool) -> u128 {
    let source = width / 2;
    let mut result = 0;
    let mut index = 0;
    while index < lanes_of(width) {
        let low = (2 * index) as u8;
        let a = lane_as(lane(bits, source, low), source, sign);
        let b = lane_as(lane(bits, source, low + 1), source, sign);
        result = replace(result, width, index as u8, a.wrapping_add(b) as u64);
        index += 1;
    }
    result
}

/// Each i32 lane is the wrapping sum of two i16 products (`i32x4.dot_i16x8_s`).
#[inline(always)]
fn dot(left: u128, right: u128) -> u128 {
    let product = |at: u8| (signed(lane(left, 16, at), 16) as i32).wrapping_mul(signed(lane(right, 16, at), 16) as i32);
    let mut result = 0;
    let mut index = 0;
    while index < 4 {
        let low = (2 * index) as u8;
        let sum = product(low).wrapping_add(product(low + 1));
        result = replace(result, 32, index as u8, sum as u32 as u64);
        index += 1;
    }
    result
}

/// Each i16 lane is the saturated sum of two signed i8 products (`i16x8.relaxed_dot_i8x16_i7x16_s`).
#[inline(always)]
fn relaxed_dot(left: u128, right: u128) -> u128 {
    let mut result = 0;
    let mut index = 0;
    while index < 8 {
        let mut sum: i64 = 0;
        let mut part = 0;
        while part < 2 {
            let at = (2 * index + part) as u8;
            sum += signed(lane(left, 8, at), 8) * signed(lane(right, 8, at), 8);
            part += 1;
        }
        result = replace(result, 16, index as u8, saturate(sum, 16, true));
        index += 1;
    }
    result
}

/// `indices`' sixteen bytes select from `bits`; an index of 16 or more selects zero.
#[inline(always)]
pub(crate) fn swizzle(bits: u128, indices: u128) -> u128 {
    let mut result = 0;
    let mut index = 0;
    while index < 16 {
        let at = (indices >> (8 * index)) as u8;
        let byte = if at < 16 { (bits >> (8 * u32::from(at))) as u8 } else { 0 };
        result |= u128::from(byte) << (8 * index);
        index += 1;
    }
    result
}

/// `left` and `right`'s bytes, `left` first, selected by `indices` (each taken modulo 32).
#[inline(always)]
pub(crate) fn shuffle(left: u128, right: u128, indices: u128) -> u128 {
    let mut result = 0;
    let mut index = 0;
    while index < 16 {
        let at = ((indices >> (8 * index)) as u8) & 31;
        let source = if at < 16 { left } else { right };
        let byte = (source >> (8 * u32::from(at & 15))) as u8;
        result |= u128::from(byte) << (8 * index);
        index += 1;
    }
    result
}

/// The bit width of a float type's lanes.
#[inline(always)]
const fn float_width(float: Float) -> u32 {
    match float {
        Float::F32 => 32,
        Float::F64 => 64,
    }
}

/// Each lane's float operator (`float_binary`: the canonical NaN for an extremum with a NaN operand).
#[inline(always)]
fn float_lanes(left: u128, right: u128, float: Float, operator: FloatBinary) -> u128 {
    zip(left, right, float_width(float), |x, y| float_binary(&mut ConcreteWasm, operator, float, x, y))
}

/// Each lane's comparison, all ones where it holds.
#[inline(always)]
fn float_compare(left: u128, right: u128, float: Float, operator: FloatBinary) -> u128 {
    let width = float_width(float);
    zip(left, right, width, |x, y| when(float_binary(&mut ConcreteWasm, operator, float, x, y) != 0, width))
}

/// Each lane's pseudo-min or -max: the right lane where `comparison` holds, else the left; the bits
/// are the operands', not the comparison's.
#[inline(always)]
fn float_pseudo(left: u128, right: u128, float: Float, comparison: FloatBinary) -> u128 {
    zip(left, right, float_width(float), |x, y| {
        if float_binary(&mut ConcreteWasm, comparison, float, x, y) != 0 { y } else { x }
    })
}

/// Each lane's unary float operator.
#[inline(always)]
fn float_unary(bits: u128, float: Float, operator: FloatUnary) -> u128 {
    each(bits, float_width(float), |x| ConcreteWasm.operate_float_unary(operator, float, x))
}

/// Each source lane (`source` bits wide) converted by the scalar rule into a `result`-bit lane; the
/// lanes past the conversion's count stay zero.
#[inline(always)]
fn convert_lanes(bits: u128, operator: Conversion, source: u32, result: u32) -> u128 {
    let count = lanes_of(if source > result { source } else { result });
    let mut out = 0;
    let mut index = 0;
    while index < count {
        let at = index as u8;
        out = replace(out, result, at, ConcreteWasm.convert(operator, lane(bits, source, at)));
        index += 1;
    }
    out
}

/// The SIMD operators, one row each, in tag order: the name, the lane immediate marker (`[lane]`
/// for an operator that takes one), the lane width, the left and right operand types (`None` for a
/// unary operator), the result type, and the body over `a` (left), `b` (right) and `l` (lane). The
/// body's value is the result's bits: a vector whole, a scalar in the low bits.
///
/// The row order is the tag order: `SimdOperator as u32` is each tag `wasm_codes.rs` names.
/// `wasm/simd.rs` and the enum below read this list through `simd_operator_list!`.
macro_rules! simd_operator_list {
    ($callback:ident) => {
        $callback! {
            V128Not: 128, V128, None, V128 => |a, b, l| !a;
            V128And: 128, V128, Some(WasmType::V128), V128 => |a, b, l| a & b;
            V128AndNot: 128, V128, Some(WasmType::V128), V128 => |a, b, l| a & !b;
            V128Or: 128, V128, Some(WasmType::V128), V128 => |a, b, l| a | b;
            V128Xor: 128, V128, Some(WasmType::V128), V128 => |a, b, l| a ^ b;
            V128AnyTrue: 128, V128, None, I32 => |a, b, l| u128::from(a != 0);
            I8x16Swizzle: 8, V128, Some(WasmType::V128), V128 => |a, b, l| swizzle(a, b);
            I8x16Splat: 8, I32, None, V128 => |a, b, l| splat(a as u64, 8);
            I16x8Splat: 16, I32, None, V128 => |a, b, l| splat(a as u64, 16);
            I32x4Splat: 32, I32, None, V128 => |a, b, l| splat(a as u64, 32);
            I64x2Splat: 64, I64, None, V128 => |a, b, l| splat(a as u64, 64);
            F32x4Splat: 32, F32, None, V128 => |a, b, l| splat(a as u64, 32);
            F64x2Splat: 64, F64, None, V128 => |a, b, l| splat(a as u64, 64);
            I8x16ExtractLaneS [lane]: 8, V128, None, I32 => |a, b, l| u128::from(signed(lane(a, 8, l), 8) as u32);
            I8x16ExtractLaneU [lane]: 8, V128, None, I32 => |a, b, l| u128::from(lane(a, 8, l) as u32);
            I16x8ExtractLaneS [lane]: 16, V128, None, I32 => |a, b, l| u128::from(signed(lane(a, 16, l), 16) as u32);
            I16x8ExtractLaneU [lane]: 16, V128, None, I32 => |a, b, l| u128::from(lane(a, 16, l) as u32);
            I32x4ExtractLane [lane]: 32, V128, None, I32 => |a, b, l| u128::from(lane(a, 32, l) as u32);
            I64x2ExtractLane [lane]: 64, V128, None, I64 => |a, b, l| u128::from(lane(a, 64, l));
            F32x4ExtractLane [lane]: 32, V128, None, F32 => |a, b, l| u128::from(lane(a, 32, l));
            F64x2ExtractLane [lane]: 64, V128, None, F64 => |a, b, l| u128::from(lane(a, 64, l));
            I8x16ReplaceLane [lane]: 8, V128, Some(WasmType::I32), V128 => |a, b, l| replace(a, 8, l, b as u64);
            I16x8ReplaceLane [lane]: 16, V128, Some(WasmType::I32), V128 => |a, b, l| replace(a, 16, l, b as u64);
            I32x4ReplaceLane [lane]: 32, V128, Some(WasmType::I32), V128 => |a, b, l| replace(a, 32, l, b as u64);
            I64x2ReplaceLane [lane]: 64, V128, Some(WasmType::I64), V128 => |a, b, l| replace(a, 64, l, b as u64);
            F32x4ReplaceLane [lane]: 32, V128, Some(WasmType::F32), V128 => |a, b, l| replace(a, 32, l, b as u64);
            F64x2ReplaceLane [lane]: 64, V128, Some(WasmType::F64), V128 => |a, b, l| replace(a, 64, l, b as u64);
            F32x4Add: 32, V128, Some(WasmType::V128), V128 => |a, b, l| float_lanes(a, b, Float::F32, FloatBinary::Add);
            F32x4Sub: 32, V128, Some(WasmType::V128), V128 => |a, b, l| float_lanes(a, b, Float::F32, FloatBinary::Subtract);
            F32x4Mul: 32, V128, Some(WasmType::V128), V128 => |a, b, l| float_lanes(a, b, Float::F32, FloatBinary::Multiply);
            F32x4Div: 32, V128, Some(WasmType::V128), V128 => |a, b, l| float_lanes(a, b, Float::F32, FloatBinary::Divide);
            F32x4Min: 32, V128, Some(WasmType::V128), V128 => |a, b, l| float_lanes(a, b, Float::F32, FloatBinary::Minimum);
            F32x4Max: 32, V128, Some(WasmType::V128), V128 => |a, b, l| float_lanes(a, b, Float::F32, FloatBinary::Maximum);
            F32x4Eq: 32, V128, Some(WasmType::V128), V128 => |a, b, l| float_compare(a, b, Float::F32, FloatBinary::Equal);
            F32x4Ne: 32, V128, Some(WasmType::V128), V128 => |a, b, l| float_compare(a, b, Float::F32, FloatBinary::NotEqual);
            F32x4Lt: 32, V128, Some(WasmType::V128), V128 => |a, b, l| float_compare(a, b, Float::F32, FloatBinary::Less);
            F32x4Gt: 32, V128, Some(WasmType::V128), V128 => |a, b, l| float_compare(a, b, Float::F32, FloatBinary::Greater);
            F32x4Le: 32, V128, Some(WasmType::V128), V128 => |a, b, l| float_compare(a, b, Float::F32, FloatBinary::LessEqual);
            F32x4Ge: 32, V128, Some(WasmType::V128), V128 => |a, b, l| float_compare(a, b, Float::F32, FloatBinary::GreaterEqual);
            F32x4PMin: 32, V128, Some(WasmType::V128), V128 => |a, b, l| float_pseudo(a, b, Float::F32, FloatBinary::Greater);
            F32x4PMax: 32, V128, Some(WasmType::V128), V128 => |a, b, l| float_pseudo(a, b, Float::F32, FloatBinary::Less);
            F32x4Abs: 32, V128, None, V128 => |a, b, l| float_unary(a, Float::F32, FloatUnary::Absolute);
            F32x4Neg: 32, V128, None, V128 => |a, b, l| float_unary(a, Float::F32, FloatUnary::Negate);
            F32x4Sqrt: 32, V128, None, V128 => |a, b, l| float_unary(a, Float::F32, FloatUnary::SquareRoot);
            F32x4Ceil: 32, V128, None, V128 => |a, b, l| float_unary(a, Float::F32, FloatUnary::Ceiling);
            F32x4Floor: 32, V128, None, V128 => |a, b, l| float_unary(a, Float::F32, FloatUnary::Floor);
            F32x4Trunc: 32, V128, None, V128 => |a, b, l| float_unary(a, Float::F32, FloatUnary::Truncate);
            F32x4Nearest: 32, V128, None, V128 => |a, b, l| float_unary(a, Float::F32, FloatUnary::Nearest);
            F64x2Add: 64, V128, Some(WasmType::V128), V128 => |a, b, l| float_lanes(a, b, Float::F64, FloatBinary::Add);
            F64x2Sub: 64, V128, Some(WasmType::V128), V128 => |a, b, l| float_lanes(a, b, Float::F64, FloatBinary::Subtract);
            F64x2Mul: 64, V128, Some(WasmType::V128), V128 => |a, b, l| float_lanes(a, b, Float::F64, FloatBinary::Multiply);
            F64x2Div: 64, V128, Some(WasmType::V128), V128 => |a, b, l| float_lanes(a, b, Float::F64, FloatBinary::Divide);
            F64x2Min: 64, V128, Some(WasmType::V128), V128 => |a, b, l| float_lanes(a, b, Float::F64, FloatBinary::Minimum);
            F64x2Max: 64, V128, Some(WasmType::V128), V128 => |a, b, l| float_lanes(a, b, Float::F64, FloatBinary::Maximum);
            F64x2Eq: 64, V128, Some(WasmType::V128), V128 => |a, b, l| float_compare(a, b, Float::F64, FloatBinary::Equal);
            F64x2Ne: 64, V128, Some(WasmType::V128), V128 => |a, b, l| float_compare(a, b, Float::F64, FloatBinary::NotEqual);
            F64x2Lt: 64, V128, Some(WasmType::V128), V128 => |a, b, l| float_compare(a, b, Float::F64, FloatBinary::Less);
            F64x2Gt: 64, V128, Some(WasmType::V128), V128 => |a, b, l| float_compare(a, b, Float::F64, FloatBinary::Greater);
            F64x2Le: 64, V128, Some(WasmType::V128), V128 => |a, b, l| float_compare(a, b, Float::F64, FloatBinary::LessEqual);
            F64x2Ge: 64, V128, Some(WasmType::V128), V128 => |a, b, l| float_compare(a, b, Float::F64, FloatBinary::GreaterEqual);
            F64x2PMin: 64, V128, Some(WasmType::V128), V128 => |a, b, l| float_pseudo(a, b, Float::F64, FloatBinary::Greater);
            F64x2PMax: 64, V128, Some(WasmType::V128), V128 => |a, b, l| float_pseudo(a, b, Float::F64, FloatBinary::Less);
            F64x2Abs: 64, V128, None, V128 => |a, b, l| float_unary(a, Float::F64, FloatUnary::Absolute);
            F64x2Neg: 64, V128, None, V128 => |a, b, l| float_unary(a, Float::F64, FloatUnary::Negate);
            F64x2Sqrt: 64, V128, None, V128 => |a, b, l| float_unary(a, Float::F64, FloatUnary::SquareRoot);
            F64x2Ceil: 64, V128, None, V128 => |a, b, l| float_unary(a, Float::F64, FloatUnary::Ceiling);
            F64x2Floor: 64, V128, None, V128 => |a, b, l| float_unary(a, Float::F64, FloatUnary::Floor);
            F64x2Trunc: 64, V128, None, V128 => |a, b, l| float_unary(a, Float::F64, FloatUnary::Truncate);
            F64x2Nearest: 64, V128, None, V128 => |a, b, l| float_unary(a, Float::F64, FloatUnary::Nearest);
            I8x16Eq: 8, V128, Some(WasmType::V128), V128 => |a, b, l| compare(a, b, 8, |x, y| x == y);
            I8x16Ne: 8, V128, Some(WasmType::V128), V128 => |a, b, l| compare(a, b, 8, |x, y| x != y);
            I8x16LtS: 8, V128, Some(WasmType::V128), V128 => |a, b, l| compare(a, b, 8, |x, y| signed(x, 8) < signed(y, 8));
            I8x16GtS: 8, V128, Some(WasmType::V128), V128 => |a, b, l| compare(a, b, 8, |x, y| signed(x, 8) > signed(y, 8));
            I8x16LeS: 8, V128, Some(WasmType::V128), V128 => |a, b, l| compare(a, b, 8, |x, y| signed(x, 8) <= signed(y, 8));
            I8x16GeS: 8, V128, Some(WasmType::V128), V128 => |a, b, l| compare(a, b, 8, |x, y| signed(x, 8) >= signed(y, 8));
            I8x16LtU: 8, V128, Some(WasmType::V128), V128 => |a, b, l| compare(a, b, 8, |x, y| x < y);
            I8x16GtU: 8, V128, Some(WasmType::V128), V128 => |a, b, l| compare(a, b, 8, |x, y| x > y);
            I8x16LeU: 8, V128, Some(WasmType::V128), V128 => |a, b, l| compare(a, b, 8, |x, y| x <= y);
            I8x16GeU: 8, V128, Some(WasmType::V128), V128 => |a, b, l| compare(a, b, 8, |x, y| x >= y);
            I8x16Add: 8, V128, Some(WasmType::V128), V128 => |a, b, l| zip(a, b, 8, u64::wrapping_add);
            I8x16Sub: 8, V128, Some(WasmType::V128), V128 => |a, b, l| zip(a, b, 8, u64::wrapping_sub);
            I8x16Abs: 8, V128, None, V128 => |a, b, l| absolute(a, 8);
            I8x16Neg: 8, V128, None, V128 => |a, b, l| negate(a, 8);
            I8x16AllTrue: 8, V128, None, I32 => |a, b, l| all_true(a, 8);
            I8x16Bitmask: 8, V128, None, I32 => |a, b, l| bitmask(a, 8);
            I8x16Shl: 8, V128, Some(WasmType::I32), V128 => |a, b, l| shift_left(a, b, 8);
            I8x16ShrS: 8, V128, Some(WasmType::I32), V128 => |a, b, l| shift_right_signed(a, b, 8);
            I8x16ShrU: 8, V128, Some(WasmType::I32), V128 => |a, b, l| shift_right_unsigned(a, b, 8);
            I8x16MinS: 8, V128, Some(WasmType::V128), V128 => |a, b, l| extrema(a, b, 8, true, false);
            I8x16MinU: 8, V128, Some(WasmType::V128), V128 => |a, b, l| extrema(a, b, 8, false, false);
            I8x16MaxS: 8, V128, Some(WasmType::V128), V128 => |a, b, l| extrema(a, b, 8, true, true);
            I8x16MaxU: 8, V128, Some(WasmType::V128), V128 => |a, b, l| extrema(a, b, 8, false, true);
            I8x16AddSatS: 8, V128, Some(WasmType::V128), V128 => |a, b, l| saturating(a, b, 8, true, false);
            I8x16AddSatU: 8, V128, Some(WasmType::V128), V128 => |a, b, l| saturating(a, b, 8, false, false);
            I8x16SubSatS: 8, V128, Some(WasmType::V128), V128 => |a, b, l| saturating(a, b, 8, true, true);
            I8x16SubSatU: 8, V128, Some(WasmType::V128), V128 => |a, b, l| saturating(a, b, 8, false, true);
            I8x16AvgrU: 8, V128, Some(WasmType::V128), V128 => |a, b, l| average(a, b, 8);
            I8x16NarrowI16x8S: 8, V128, Some(WasmType::V128), V128 => |a, b, l| narrow(a, b, 8, true);
            I8x16NarrowI16x8U: 8, V128, Some(WasmType::V128), V128 => |a, b, l| narrow(a, b, 8, false);
            I16x8Eq: 16, V128, Some(WasmType::V128), V128 => |a, b, l| compare(a, b, 16, |x, y| x == y);
            I16x8Ne: 16, V128, Some(WasmType::V128), V128 => |a, b, l| compare(a, b, 16, |x, y| x != y);
            I16x8LtS: 16, V128, Some(WasmType::V128), V128 => |a, b, l| compare(a, b, 16, |x, y| signed(x, 16) < signed(y, 16));
            I16x8GtS: 16, V128, Some(WasmType::V128), V128 => |a, b, l| compare(a, b, 16, |x, y| signed(x, 16) > signed(y, 16));
            I16x8LeS: 16, V128, Some(WasmType::V128), V128 => |a, b, l| compare(a, b, 16, |x, y| signed(x, 16) <= signed(y, 16));
            I16x8GeS: 16, V128, Some(WasmType::V128), V128 => |a, b, l| compare(a, b, 16, |x, y| signed(x, 16) >= signed(y, 16));
            I16x8LtU: 16, V128, Some(WasmType::V128), V128 => |a, b, l| compare(a, b, 16, |x, y| x < y);
            I16x8GtU: 16, V128, Some(WasmType::V128), V128 => |a, b, l| compare(a, b, 16, |x, y| x > y);
            I16x8LeU: 16, V128, Some(WasmType::V128), V128 => |a, b, l| compare(a, b, 16, |x, y| x <= y);
            I16x8GeU: 16, V128, Some(WasmType::V128), V128 => |a, b, l| compare(a, b, 16, |x, y| x >= y);
            I16x8Add: 16, V128, Some(WasmType::V128), V128 => |a, b, l| zip(a, b, 16, u64::wrapping_add);
            I16x8Sub: 16, V128, Some(WasmType::V128), V128 => |a, b, l| zip(a, b, 16, u64::wrapping_sub);
            I16x8Mul: 16, V128, Some(WasmType::V128), V128 => |a, b, l| zip(a, b, 16, u64::wrapping_mul);
            I16x8Abs: 16, V128, None, V128 => |a, b, l| absolute(a, 16);
            I16x8Neg: 16, V128, None, V128 => |a, b, l| negate(a, 16);
            I16x8AllTrue: 16, V128, None, I32 => |a, b, l| all_true(a, 16);
            I16x8Bitmask: 16, V128, None, I32 => |a, b, l| bitmask(a, 16);
            I16x8Shl: 16, V128, Some(WasmType::I32), V128 => |a, b, l| shift_left(a, b, 16);
            I16x8ShrS: 16, V128, Some(WasmType::I32), V128 => |a, b, l| shift_right_signed(a, b, 16);
            I16x8ShrU: 16, V128, Some(WasmType::I32), V128 => |a, b, l| shift_right_unsigned(a, b, 16);
            I16x8MinS: 16, V128, Some(WasmType::V128), V128 => |a, b, l| extrema(a, b, 16, true, false);
            I16x8MinU: 16, V128, Some(WasmType::V128), V128 => |a, b, l| extrema(a, b, 16, false, false);
            I16x8MaxS: 16, V128, Some(WasmType::V128), V128 => |a, b, l| extrema(a, b, 16, true, true);
            I16x8MaxU: 16, V128, Some(WasmType::V128), V128 => |a, b, l| extrema(a, b, 16, false, true);
            I16x8AddSatS: 16, V128, Some(WasmType::V128), V128 => |a, b, l| saturating(a, b, 16, true, false);
            I16x8AddSatU: 16, V128, Some(WasmType::V128), V128 => |a, b, l| saturating(a, b, 16, false, false);
            I16x8SubSatS: 16, V128, Some(WasmType::V128), V128 => |a, b, l| saturating(a, b, 16, true, true);
            I16x8SubSatU: 16, V128, Some(WasmType::V128), V128 => |a, b, l| saturating(a, b, 16, false, true);
            I16x8AvgrU: 16, V128, Some(WasmType::V128), V128 => |a, b, l| average(a, b, 16);
            I16x8ExtendLowI8x16S: 16, V128, None, V128 => |a, b, l| extend(a, 16, true, false);
            I16x8ExtMulLowI8x16S: 16, V128, Some(WasmType::V128), V128 => |a, b, l| extend_multiply(a, b, 16, true, false);
            I16x8ExtendLowI8x16U: 16, V128, None, V128 => |a, b, l| extend(a, 16, false, false);
            I16x8ExtMulLowI8x16U: 16, V128, Some(WasmType::V128), V128 => |a, b, l| extend_multiply(a, b, 16, false, false);
            I16x8ExtendHighI8x16S: 16, V128, None, V128 => |a, b, l| extend(a, 16, true, true);
            I16x8ExtMulHighI8x16S: 16, V128, Some(WasmType::V128), V128 => |a, b, l| extend_multiply(a, b, 16, true, true);
            I16x8ExtendHighI8x16U: 16, V128, None, V128 => |a, b, l| extend(a, 16, false, true);
            I16x8ExtMulHighI8x16U: 16, V128, Some(WasmType::V128), V128 => |a, b, l| extend_multiply(a, b, 16, false, true);
            I16x8ExtAddPairwiseI8x16S: 16, V128, None, V128 => |a, b, l| pairwise_add(a, 16, true);
            I16x8ExtAddPairwiseI8x16U: 16, V128, None, V128 => |a, b, l| pairwise_add(a, 16, false);
            I16x8NarrowI32x4S: 16, V128, Some(WasmType::V128), V128 => |a, b, l| narrow(a, b, 16, true);
            I16x8NarrowI32x4U: 16, V128, Some(WasmType::V128), V128 => |a, b, l| narrow(a, b, 16, false);
            I32x4Eq: 32, V128, Some(WasmType::V128), V128 => |a, b, l| compare(a, b, 32, |x, y| x == y);
            I32x4Ne: 32, V128, Some(WasmType::V128), V128 => |a, b, l| compare(a, b, 32, |x, y| x != y);
            I32x4LtS: 32, V128, Some(WasmType::V128), V128 => |a, b, l| compare(a, b, 32, |x, y| signed(x, 32) < signed(y, 32));
            I32x4GtS: 32, V128, Some(WasmType::V128), V128 => |a, b, l| compare(a, b, 32, |x, y| signed(x, 32) > signed(y, 32));
            I32x4LeS: 32, V128, Some(WasmType::V128), V128 => |a, b, l| compare(a, b, 32, |x, y| signed(x, 32) <= signed(y, 32));
            I32x4GeS: 32, V128, Some(WasmType::V128), V128 => |a, b, l| compare(a, b, 32, |x, y| signed(x, 32) >= signed(y, 32));
            I32x4LtU: 32, V128, Some(WasmType::V128), V128 => |a, b, l| compare(a, b, 32, |x, y| x < y);
            I32x4GtU: 32, V128, Some(WasmType::V128), V128 => |a, b, l| compare(a, b, 32, |x, y| x > y);
            I32x4LeU: 32, V128, Some(WasmType::V128), V128 => |a, b, l| compare(a, b, 32, |x, y| x <= y);
            I32x4GeU: 32, V128, Some(WasmType::V128), V128 => |a, b, l| compare(a, b, 32, |x, y| x >= y);
            I32x4Add: 32, V128, Some(WasmType::V128), V128 => |a, b, l| zip(a, b, 32, u64::wrapping_add);
            I32x4Sub: 32, V128, Some(WasmType::V128), V128 => |a, b, l| zip(a, b, 32, u64::wrapping_sub);
            I32x4Mul: 32, V128, Some(WasmType::V128), V128 => |a, b, l| zip(a, b, 32, u64::wrapping_mul);
            I32x4Abs: 32, V128, None, V128 => |a, b, l| absolute(a, 32);
            I32x4Neg: 32, V128, None, V128 => |a, b, l| negate(a, 32);
            I32x4AllTrue: 32, V128, None, I32 => |a, b, l| all_true(a, 32);
            I32x4Bitmask: 32, V128, None, I32 => |a, b, l| bitmask(a, 32);
            I32x4Shl: 32, V128, Some(WasmType::I32), V128 => |a, b, l| shift_left(a, b, 32);
            I32x4ShrS: 32, V128, Some(WasmType::I32), V128 => |a, b, l| shift_right_signed(a, b, 32);
            I32x4ShrU: 32, V128, Some(WasmType::I32), V128 => |a, b, l| shift_right_unsigned(a, b, 32);
            I32x4MinS: 32, V128, Some(WasmType::V128), V128 => |a, b, l| extrema(a, b, 32, true, false);
            I32x4MinU: 32, V128, Some(WasmType::V128), V128 => |a, b, l| extrema(a, b, 32, false, false);
            I32x4MaxS: 32, V128, Some(WasmType::V128), V128 => |a, b, l| extrema(a, b, 32, true, true);
            I32x4MaxU: 32, V128, Some(WasmType::V128), V128 => |a, b, l| extrema(a, b, 32, false, true);
            I32x4ExtendLowI16x8S: 32, V128, None, V128 => |a, b, l| extend(a, 32, true, false);
            I32x4ExtMulLowI16x8S: 32, V128, Some(WasmType::V128), V128 => |a, b, l| extend_multiply(a, b, 32, true, false);
            I32x4ExtendLowI16x8U: 32, V128, None, V128 => |a, b, l| extend(a, 32, false, false);
            I32x4ExtMulLowI16x8U: 32, V128, Some(WasmType::V128), V128 => |a, b, l| extend_multiply(a, b, 32, false, false);
            I32x4ExtendHighI16x8S: 32, V128, None, V128 => |a, b, l| extend(a, 32, true, true);
            I32x4ExtMulHighI16x8S: 32, V128, Some(WasmType::V128), V128 => |a, b, l| extend_multiply(a, b, 32, true, true);
            I32x4ExtendHighI16x8U: 32, V128, None, V128 => |a, b, l| extend(a, 32, false, true);
            I32x4ExtMulHighI16x8U: 32, V128, Some(WasmType::V128), V128 => |a, b, l| extend_multiply(a, b, 32, false, true);
            I32x4ExtAddPairwiseI16x8S: 32, V128, None, V128 => |a, b, l| pairwise_add(a, 32, true);
            I32x4ExtAddPairwiseI16x8U: 32, V128, None, V128 => |a, b, l| pairwise_add(a, 32, false);
            I64x2Eq: 64, V128, Some(WasmType::V128), V128 => |a, b, l| compare(a, b, 64, |x, y| x == y);
            I64x2Ne: 64, V128, Some(WasmType::V128), V128 => |a, b, l| compare(a, b, 64, |x, y| x != y);
            I64x2LtS: 64, V128, Some(WasmType::V128), V128 => |a, b, l| compare(a, b, 64, |x, y| signed(x, 64) < signed(y, 64));
            I64x2GtS: 64, V128, Some(WasmType::V128), V128 => |a, b, l| compare(a, b, 64, |x, y| signed(x, 64) > signed(y, 64));
            I64x2LeS: 64, V128, Some(WasmType::V128), V128 => |a, b, l| compare(a, b, 64, |x, y| signed(x, 64) <= signed(y, 64));
            I64x2GeS: 64, V128, Some(WasmType::V128), V128 => |a, b, l| compare(a, b, 64, |x, y| signed(x, 64) >= signed(y, 64));
            I64x2Add: 64, V128, Some(WasmType::V128), V128 => |a, b, l| zip(a, b, 64, u64::wrapping_add);
            I64x2Sub: 64, V128, Some(WasmType::V128), V128 => |a, b, l| zip(a, b, 64, u64::wrapping_sub);
            I64x2Mul: 64, V128, Some(WasmType::V128), V128 => |a, b, l| zip(a, b, 64, u64::wrapping_mul);
            I64x2Abs: 64, V128, None, V128 => |a, b, l| absolute(a, 64);
            I64x2Neg: 64, V128, None, V128 => |a, b, l| negate(a, 64);
            I64x2AllTrue: 64, V128, None, I32 => |a, b, l| all_true(a, 64);
            I64x2Bitmask: 64, V128, None, I32 => |a, b, l| bitmask(a, 64);
            I64x2Shl: 64, V128, Some(WasmType::I32), V128 => |a, b, l| shift_left(a, b, 64);
            I64x2ShrS: 64, V128, Some(WasmType::I32), V128 => |a, b, l| shift_right_signed(a, b, 64);
            I64x2ShrU: 64, V128, Some(WasmType::I32), V128 => |a, b, l| shift_right_unsigned(a, b, 64);
            I64x2ExtendLowI32x4S: 64, V128, None, V128 => |a, b, l| extend(a, 64, true, false);
            I64x2ExtMulLowI32x4S: 64, V128, Some(WasmType::V128), V128 => |a, b, l| extend_multiply(a, b, 64, true, false);
            I64x2ExtendLowI32x4U: 64, V128, None, V128 => |a, b, l| extend(a, 64, false, false);
            I64x2ExtMulLowI32x4U: 64, V128, Some(WasmType::V128), V128 => |a, b, l| extend_multiply(a, b, 64, false, false);
            I64x2ExtendHighI32x4S: 64, V128, None, V128 => |a, b, l| extend(a, 64, true, true);
            I64x2ExtMulHighI32x4S: 64, V128, Some(WasmType::V128), V128 => |a, b, l| extend_multiply(a, b, 64, true, true);
            I64x2ExtendHighI32x4U: 64, V128, None, V128 => |a, b, l| extend(a, 64, false, true);
            I64x2ExtMulHighI32x4U: 64, V128, Some(WasmType::V128), V128 => |a, b, l| extend_multiply(a, b, 64, false, true);
            I8x16Popcnt: 8, V128, None, V128 => |a, b, l| population_count(a);
            I16x8Q15MulrSatS: 16, V128, Some(WasmType::V128), V128 => |a, b, l| q15(a, b);
            I32x4DotI16x8S: 32, V128, Some(WasmType::V128), V128 => |a, b, l| dot(a, b);
            I32x4TruncSatF32x4S: 32, V128, None, V128 => |a, b, l| convert_lanes(a, Conversion::SaturateF32SignedI32, 32, 32);
            I32x4TruncSatF32x4U: 32, V128, None, V128 => |a, b, l| convert_lanes(a, Conversion::SaturateF32UnsignedI32, 32, 32);
            F32x4ConvertI32x4S: 32, V128, None, V128 => |a, b, l| convert_lanes(a, Conversion::ConvertI32SignedF32, 32, 32);
            F32x4ConvertI32x4U: 32, V128, None, V128 => |a, b, l| convert_lanes(a, Conversion::ConvertI32UnsignedF32, 32, 32);
            I32x4TruncSatF64x2SZero: 64, V128, None, V128 => |a, b, l| convert_lanes(a, Conversion::SaturateF64SignedI32, 64, 32);
            I32x4TruncSatF64x2UZero: 64, V128, None, V128 => |a, b, l| convert_lanes(a, Conversion::SaturateF64UnsignedI32, 64, 32);
            F64x2ConvertLowI32x4S: 32, V128, None, V128 => |a, b, l| convert_lanes(a, Conversion::ConvertI32SignedF64, 32, 64);
            F64x2ConvertLowI32x4U: 32, V128, None, V128 => |a, b, l| convert_lanes(a, Conversion::ConvertI32UnsignedF64, 32, 64);
            F32x4DemoteF64x2Zero: 64, V128, None, V128 => |a, b, l| convert_lanes(a, Conversion::DemoteF64, 64, 32);
            F64x2PromoteLowF32x4: 32, V128, None, V128 => |a, b, l| convert_lanes(a, Conversion::PromoteF32, 32, 64);
            I8x16RelaxedSwizzle: 8, V128, Some(WasmType::V128), V128 => |a, b, l| swizzle(a, b);
            I32x4RelaxedTruncF32x4S: 32, V128, None, V128 => |a, b, l| convert_lanes(a, Conversion::SaturateF32SignedI32, 32, 32);
            I32x4RelaxedTruncF32x4U: 32, V128, None, V128 => |a, b, l| convert_lanes(a, Conversion::SaturateF32UnsignedI32, 32, 32);
            I32x4RelaxedTruncF64x2SZero: 64, V128, None, V128 => |a, b, l| convert_lanes(a, Conversion::SaturateF64SignedI32, 64, 32);
            I32x4RelaxedTruncF64x2UZero: 64, V128, None, V128 => |a, b, l| convert_lanes(a, Conversion::SaturateF64UnsignedI32, 64, 32);
            F32x4RelaxedMin: 32, V128, Some(WasmType::V128), V128 => |a, b, l| float_lanes(a, b, Float::F32, FloatBinary::Minimum);
            F32x4RelaxedMax: 32, V128, Some(WasmType::V128), V128 => |a, b, l| float_lanes(a, b, Float::F32, FloatBinary::Maximum);
            F64x2RelaxedMin: 64, V128, Some(WasmType::V128), V128 => |a, b, l| float_lanes(a, b, Float::F64, FloatBinary::Minimum);
            F64x2RelaxedMax: 64, V128, Some(WasmType::V128), V128 => |a, b, l| float_lanes(a, b, Float::F64, FloatBinary::Maximum);
            I16x8RelaxedQ15mulrS: 16, V128, Some(WasmType::V128), V128 => |a, b, l| q15(a, b);
            I16x8RelaxedDotI8x16I7x16S: 16, V128, Some(WasmType::V128), V128 => |a, b, l| relaxed_dot(a, b);
        }
    };
}
pub(crate) use simd_operator_list;

/// The enum and its tag lookup, generated from the operator list: the discriminants are the list's
/// order, from zero.
macro_rules! define_simd_operators {
    ($($name:ident $([$lane:ident])? : $width:expr, $left:ident, $right:expr, $result:ident => |$a:ident, $b:ident, $l:ident| $body:expr;)*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[repr(u32)]
        pub(crate) enum SimdOperator { $($name,)* }

        impl SimdOperator {
            /// Every operator, by tag.
            pub(crate) const ALL: &'static [Self] = &[$(Self::$name,)*];

            /// The operator whose tag is `tag`, in a const context (a row's OP folds to it).
            #[inline(always)]
            pub(crate) const fn from_tag(tag: u32) -> Option<Self> {
                let mut index = 0;
                while index < Self::ALL.len() {
                    if Self::ALL[index] as u32 == tag {
                        return Some(Self::ALL[index]);
                    }
                    index += 1;
                }
                None
            }

            /// The operator on `left` and `right` (a scalar's bits in the low bits), with `lane` its
            /// lane immediate (zero for an operator without one): the result's bits.
            #[inline(always)]
            pub(crate) fn bits(self, left: u128, right: u128, lane: u8) -> u128 {
                match self {
                    $(Self::$name => {
                        let ($a, $b, $l) = (left, right, lane);
                        let _ = (&$a, &$b, &$l);
                        $body
                    },)*
                }
            }
        }
    };
}
simd_operator_list!(define_simd_operators);
