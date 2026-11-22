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
use crate::staged::wasm_float::{Float, float_binary, opaque};
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

/// All ones in a `width`-bit lane where `holds`, zero where not. Opaque in the stencil build, where
/// the backend would otherwise gather sixteen byte masks through constant-pool shuffles.
#[inline(always)]
fn when(holds: bool, width: u32) -> u64 {
    opaque(if holds { mask64(width) } else { 0 })
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
    zip(left, right, width, #[inline(always)] |x, y| when(holds(x, y), width))
}

/// Every lane set to `value`'s low `width` bits.
#[inline(always)]
fn splat(value: u64, width: u32) -> u128 {
    each(0, width, #[inline(always)] |_| value)
}

/// Each lane negated (wrapping).
#[inline(always)]
fn negate(bits: u128, width: u32) -> u128 {
    each(bits, width, #[inline(always)] |x| 0u64.wrapping_sub(x))
}

/// Each lane's magnitude (wrapping at the minimum).
#[inline(always)]
fn absolute(bits: u128, width: u32) -> u128 {
    each(bits, width, #[inline(always)] |x| if signed(x, width) < 0 { 0u64.wrapping_sub(x) } else { x })
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
    each(bits, 8, #[inline(always)] |x| u64::from(x.count_ones()))
}

/// Each lane shifted left by `count` modulo the lane width.
#[inline(always)]
fn shift_left(bits: u128, count: u128, width: u32) -> u128 {
    let amount = (count as u32) & (width - 1);
    each(bits, width, #[inline(always)] |x| x << amount)
}

/// Each lane shifted right, zero-filled, by `count` modulo the lane width.
#[inline(always)]
fn shift_right_unsigned(bits: u128, count: u128, width: u32) -> u128 {
    let amount = (count as u32) & (width - 1);
    each(bits, width, #[inline(always)] |x| x >> amount)
}

/// Each lane shifted right, sign-filled, by `count` modulo the lane width.
#[inline(always)]
fn shift_right_signed(bits: u128, count: u128, width: u32) -> u128 {
    let amount = (count as u32) & (width - 1);
    each(bits, width, #[inline(always)] |x| (signed(x, width) >> amount) as u64)
}

/// Each lane's minimum (`greater` false) or maximum (`greater` true) of the pair.
#[inline(always)]
fn extrema(left: u128, right: u128, width: u32, sign: bool, greater: bool) -> u128 {
    zip(left, right, width, #[inline(always)] |x, y| {
        let (a, b) = (lane_as(x, width, sign), lane_as(y, width, sign));
        let keeps = if greater { a > b } else { a < b };
        if keeps { x } else { y }
    })
}

/// Each lane's saturating sum or difference of the pair.
#[inline(always)]
fn saturating(left: u128, right: u128, width: u32, sign: bool, subtract: bool) -> u128 {
    zip(left, right, width, #[inline(always)] |x, y| {
        let (a, b) = (lane_as(x, width, sign), lane_as(y, width, sign));
        saturate(if subtract { a - b } else { a + b }, width, sign)
    })
}

/// Each lane's unsigned rounding average of the pair.
#[inline(always)]
fn average(left: u128, right: u128, width: u32) -> u128 {
    zip(left, right, width, #[inline(always)] |x, y| (x + y + 1) >> 1)
}

/// Q15 fixed-point multiply with rounding and saturation, per i16 lane.
#[inline(always)]
fn q15(left: u128, right: u128) -> u128 {
    zip(left, right, 16, #[inline(always)] |x, y| {
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
    zip(left, right, float_width(float), #[inline(always)] |x, y| float_binary(&mut ConcreteWasm, operator, float, x, y))
}

/// Each lane's comparison, all ones where it holds.
#[inline(always)]
fn float_compare(left: u128, right: u128, float: Float, operator: FloatBinary) -> u128 {
    let width = float_width(float);
    zip(left, right, width, #[inline(always)] |x, y| when(float_binary(&mut ConcreteWasm, operator, float, x, y) != 0, width))
}

/// Each lane's pseudo-min or -max: the right lane where `comparison` holds, else the left; the bits
/// are the operands', not the comparison's.
#[inline(always)]
fn float_pseudo(left: u128, right: u128, float: Float, comparison: FloatBinary) -> u128 {
    zip(left, right, float_width(float), #[inline(always)] |x, y| {
        if float_binary(&mut ConcreteWasm, comparison, float, x, y) != 0 { y } else { x }
    })
}

/// Each lane's unary float operator.
#[inline(always)]
fn float_unary(bits: u128, float: Float, operator: FloatUnary) -> u128 {
    each(bits, float_width(float), #[inline(always)] |x| ConcreteWasm.operate_float_unary(operator, float, x))
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

/// The operator list is `wasm_lists.rs`'s `simd_operator_list!`, which the runtime and the b-jit both expand;
/// this module expands it into the enum below.
#[path = "wasm_lists.rs"]
mod wasm_lists;
pub(crate) use wasm_lists::simd_operator_list;

/// The enum and its tag lookup, generated from the operator list: the discriminants are the list's
/// order, from zero.
macro_rules! define_simd_operators {
    ($($name:ident $([$lane:ident])? : $width:expr, $left:ident, $right:expr, $result:ident, $family:ident => |$a:ident, $b:ident, $l:ident| $body:expr;)*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[repr(u32)]
        pub enum SimdOperator { $($name,)* }

        impl SimdOperator {
            /// Every operator, by tag.
            pub const ALL: &'static [Self] = &[$(Self::$name,)*];

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
