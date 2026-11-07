//! Exact vector bits and admitted SIMD lane operations; no host SIMD dependency.

use super::{V128_BYTES, WasmType, WasmValue};
use crate::bytecode::{Op, Register};

mod integer;

const SELECTOR_LANE_BITS: u32 = u8::BITS;
const SELECTOR_LANE_MASK: u32 = u8::MAX as u32;

fn vector(value: WasmValue) -> u128 {
    let WasmValue::V128(bits) = value else {
        unreachable!("decoded SIMD operand")
    };
    bits
}

fn scalar(value: WasmValue) -> u64 {
    match value {
        WasmValue::I32(value) => u64::from(value as u32),
        WasmValue::F32(bits) => u64::from(bits),
        WasmValue::I64(value) => value as u64,
        WasmValue::F64(bits) => bits,
        _ => unreachable!("decoded SIMD scalar"),
    }
}

fn lane_mask(width: u32) -> u128 {
    u128::MAX >> (u128::BITS - width)
}
fn lane(bits: u128, width: u32, index: u8) -> u64 {
    ((bits >> (width * u32::from(index))) & lane_mask(width)) as u64
}
fn replace(bits: u128, width: u32, index: u8, value: u64) -> u128 {
    let shift = width * u32::from(index);
    let mask = lane_mask(width);
    (bits & !(mask << shift)) | ((u128::from(value) & mask) << shift)
}
fn splat(value: u64, width: u32) -> u128 {
    (0..u128::BITS / width).fold(0, |bits, index| replace(bits, width, index as u8, value))
}

fn map_lanes(
    left: u128,
    right: Option<u128>,
    width: u32,
    mut operation: impl FnMut(u64, Option<u64>) -> u64,
) -> WasmValue {
    let mut result = 0;
    for index in 0..u128::BITS / width {
        let index = index as u8;
        let value = operation(
            lane(left, width, index),
            right.map(|bits| lane(bits, width, index)),
        );
        result = replace(result, width, index, value);
    }
    WasmValue::V128(result)
}

macro_rules! float_lane_projection {
    ($binary_fn:ident, $unary_fn:ident, $pseudo_fn:ident, $float:ty, $bits:ty, $binary:ident, $unary:ident) => {
        fn $binary_fn(left: u128, right: u128, operator: super::float::$binary) -> WasmValue {
            map_lanes(left, Some(right), <$bits>::BITS, |left, right| {
                let value = operator.apply(
                    <$float>::from_bits(left as $bits),
                    <$float>::from_bits(right.unwrap() as $bits),
                );
                match value {
                    WasmValue::I32(comparison) => {
                        if comparison != 0 {
                            lane_mask(<$bits>::BITS) as u64
                        } else {
                            0
                        }
                    }
                    value => scalar(value),
                }
            })
        }
        fn $unary_fn(value: u128, operator: super::float::$unary) -> WasmValue {
            map_lanes(value, None, <$bits>::BITS, |bits, _| {
                scalar(operator.apply(<$float>::from_bits(bits as $bits)))
            })
        }
        fn $pseudo_fn(left: u128, right: u128, comparison: super::float::$binary) -> WasmValue {
            map_lanes(left, Some(right), <$bits>::BITS, |left, right| {
                let right = right.unwrap();
                // The comparison decides selection; the original bits own the result.
                let WasmValue::I32(select_right) = comparison.apply(
                    <$float>::from_bits(left as $bits),
                    <$float>::from_bits(right as $bits),
                ) else {
                    unreachable!("pseudo-extrema comparison")
                };
                if select_right != 0 { right } else { left }
            })
        }
    };
}
float_lane_projection!(
    f32_binary,
    f32_unary,
    f32_pseudo,
    f32,
    u32,
    F32BinaryOperator,
    F32UnaryOperator
);
float_lane_projection!(
    f64_binary,
    f64_unary,
    f64_pseudo,
    f64,
    u64,
    F64BinaryOperator,
    F64UnaryOperator
);

// Scalar conversion signatures determine lane interpretation and active lanes.
// Narrowing zeroes the unused upper lanes; widening reads only the low lanes.
fn convert(value: u128, op: super::conversion::ScalarConversionOperator) -> WasmValue {
    use super::ScalarBits;
    let width = |ty| match ty {
        WasmType::I32 | WasmType::F32 => u32::BITS,
        WasmType::F64 => u64::BITS,
        _ => unreachable!("admitted SIMD conversion signature"),
    };
    let source = op.source_type();
    let source_width = width(source);
    let result_width = width(op.result_type());
    let mut result = 0;
    for index in 0..u128::BITS / source_width.max(result_width) {
        let bits = lane(value, source_width, index as u8);
        let bits = if source_width == u32::BITS {
            ScalarBits::Bits32(bits as u32)
        } else {
            ScalarBits::Bits64(bits)
        };
        let input = source.decode(bits).expect("scalar lane signature");
        let output = op.apply(input).expect("nontrapping SIMD conversion");
        result = replace(result, result_width, index as u8, scalar(output));
    }
    WasmValue::V128(result)
}

/// An operator as an execution needs it (`SimdOperator::kernel`).
pub(crate) struct SimdKernel {
    pub(crate) left: WasmType,
    pub(crate) right: Option<WasmType>,
    /// How many lanes a lane immediate may name: one for an operator without one.
    lanes: u32,
    pub(crate) apply: fn(WasmValue, Option<WasmValue>, u8) -> WasmValue,
}

impl SimdKernel {
    /// The kernel and lane immediate a selector names: none for an unknown operator or a
    /// lane past its count.
    #[inline]
    pub(crate) fn of(selector: u32) -> Option<(&'static Self, u8)> {
        let kernel = SimdOperator::KERNELS.get((selector >> SELECTOR_LANE_BITS) as usize)?;
        let lane = (selector & SELECTOR_LANE_MASK) as u8;
        (u32::from(lane) < kernel.lanes).then_some((kernel, lane))
    }

    /// The type of the operator's result, read off its own semantics: a kernel is pure and
    /// total, so applied to zero operands it answers with a value of that type.
    pub(crate) fn result_type(&self, lane: u8) -> WasmType {
        self.apply_bits_typed(0, 0, lane).ty()
    }

    /// The operator applied to operands given as bits, its result as bits: a vector whole,
    /// a scalar in the low bits (`ScalarBits`), as compiled code holds them.
    pub(crate) fn apply_bits(&self, left: u128, right: u128, lane: u8) -> u128 {
        match self.apply_bits_typed(left, right, lane) {
            WasmValue::V128(bits) => bits,
            value => u128::from(scalar(value)),
        }
    }

    fn apply_bits_typed(&self, left: u128, right: u128, lane: u8) -> WasmValue {
        let value = |ty, bits: u128| match ty {
            WasmType::V128 => WasmValue::V128(bits),
            ty => WasmValue::from_bits(ty, bits as u64),
        };
        (self.apply)(value(self.left, left), self.right.map(|ty| value(ty, right)), lane)
    }
}

/// The lanes a lane immediate may name, from the operator's lane width when it has one.
const fn lane_count(widths: &[u32]) -> u32 {
    match widths {
        [width] => u128::BITS / *width,
        _ => 1,
    }
}

macro_rules! simd_operators {
    ($($name:ident [$($lane:ident: $width:expr)?]: $left:ident, $right:expr => |$a:ident, $b:ident, $l:ident| $body:expr;)* ) => {
        #[derive(Clone, Copy, Debug)]
        #[repr(u32)]
        pub(crate) enum SimdOperator { $($name,)* }
        impl SimdOperator {
            const ALL: &'static [Self] = &[$(Self::$name,)*];
            /// Each operator's signature and body by selector position, so an execution reads
            /// one table entry instead of matching the operator for each of them.
            const KERNELS: &'static [SimdKernel] = &[$(SimdKernel {
                left: WasmType::$left,
                right: $right,
                lanes: lane_count(&[$($width)?]),
                apply: |$a, $b, $l| { let _ = (&$a, &$b, &$l); $body },
            },)*];
            pub(crate) fn from_selector(selector: u32) -> Option<(Self, u8)> {
                let (_, lane) = SimdKernel::of(selector)?;
                Some((Self::ALL[(selector >> SELECTOR_LANE_BITS) as usize], lane))
            }
            pub(super) fn selector(self, lane: u8) -> u32 { ((self as u32) << SELECTOR_LANE_BITS) | u32::from(lane) }
            pub(super) fn from_wasm(op: &wasmparser::Operator<'_>) -> Option<(Self, u8)> {
                match op {
                    $(wasmparser::Operator::$name $({$lane})? => Some((Self::$name, [$(*$lane)?].first().copied().unwrap_or(0))),)*
                    _ => None,
                }
            }
            pub(crate) fn right_type(self) -> Option<WasmType> { Self::KERNELS[self as usize].right }
            pub(crate) fn apply(self, left: WasmValue, right: Option<WasmValue>, index: u8) -> WasmValue {
                match self {
                    $(Self::$name => { let ($a, $b, $l) = (left, right, index); let _ = (&$a, &$b, &$l); $body },)*
                }
            }
        }

    }
}

simd_operators! {
  V128Not []: V128, None => |a,b,l| WasmValue::V128(!vector(a));
  V128And []: V128, Some(WasmType::V128) => |a,b,l| WasmValue::V128(vector(a) & vector(b.unwrap()));
  V128AndNot []: V128, Some(WasmType::V128) => |a,b,l| WasmValue::V128(vector(a) & !vector(b.unwrap()));
  V128Or []: V128, Some(WasmType::V128) => |a,b,l| WasmValue::V128(vector(a) | vector(b.unwrap()));
  V128Xor []: V128, Some(WasmType::V128) => |a,b,l| WasmValue::V128(vector(a) ^ vector(b.unwrap()));
  V128AnyTrue []: V128, None => |a,b,l| WasmValue::I32(i32::from(vector(a) != 0));
  I8x16Swizzle []: V128, Some(WasmType::V128) => |a,b,l| WasmValue::V128(swizzle(vector(a), vector(b.unwrap())));
  I8x16Splat []: I32, None => |a,b,l| WasmValue::V128(splat(scalar(a), u8::BITS));
  I16x8Splat []: I32, None => |a,b,l| WasmValue::V128(splat(scalar(a), u16::BITS));
  I32x4Splat []: I32, None => |a,b,l| WasmValue::V128(splat(scalar(a), u32::BITS));
  I64x2Splat []: I64, None => |a,b,l| WasmValue::V128(splat(scalar(a), u64::BITS));
  F32x4Splat []: F32, None => |a,b,l| WasmValue::V128(splat(scalar(a), u32::BITS));
  F64x2Splat []: F64, None => |a,b,l| WasmValue::V128(splat(scalar(a), u64::BITS));
  I8x16ExtractLaneS [lane: u8::BITS]: V128, None => |a,b,l| WasmValue::I32(lane(vector(a), u8::BITS,l) as u8 as i8 as i32);
  I8x16ExtractLaneU [lane: u8::BITS]: V128, None => |a,b,l| WasmValue::I32(lane(vector(a), u8::BITS,l) as i32);
  I16x8ExtractLaneS [lane: u16::BITS]: V128, None => |a,b,l| WasmValue::I32(lane(vector(a), u16::BITS,l) as u16 as i16 as i32);
  I16x8ExtractLaneU [lane: u16::BITS]: V128, None => |a,b,l| WasmValue::I32(lane(vector(a), u16::BITS,l) as i32);
  I32x4ExtractLane [lane: u32::BITS]: V128, None => |a,b,l| WasmValue::I32(lane(vector(a), u32::BITS,l) as i32);
  I64x2ExtractLane [lane: u64::BITS]: V128, None => |a,b,l| WasmValue::I64(lane(vector(a), u64::BITS,l) as i64);
  F32x4ExtractLane [lane: u32::BITS]: V128, None => |a,b,l| WasmValue::F32(lane(vector(a), u32::BITS,l) as u32);
  F64x2ExtractLane [lane: u64::BITS]: V128, None => |a,b,l| WasmValue::F64(lane(vector(a), u64::BITS,l));
  I8x16ReplaceLane [lane: u8::BITS]: V128, Some(WasmType::I32) => |a,b,l| WasmValue::V128(replace(vector(a),u8::BITS,l,scalar(b.unwrap())));
  I16x8ReplaceLane [lane: u16::BITS]: V128, Some(WasmType::I32) => |a,b,l| WasmValue::V128(replace(vector(a),u16::BITS,l,scalar(b.unwrap())));
  I32x4ReplaceLane [lane: u32::BITS]: V128, Some(WasmType::I32) => |a,b,l| WasmValue::V128(replace(vector(a),u32::BITS,l,scalar(b.unwrap())));
  I64x2ReplaceLane [lane: u64::BITS]: V128, Some(WasmType::I64) => |a,b,l| WasmValue::V128(replace(vector(a),u64::BITS,l,scalar(b.unwrap())));
  F32x4ReplaceLane [lane: u32::BITS]: V128, Some(WasmType::F32) => |a,b,l| WasmValue::V128(replace(vector(a),u32::BITS,l,scalar(b.unwrap())));
  F64x2ReplaceLane [lane: u64::BITS]: V128, Some(WasmType::F64) => |a,b,l| WasmValue::V128(replace(vector(a),u64::BITS,l,scalar(b.unwrap())));
  F32x4Add []: V128, Some(WasmType::V128) => |a,b,l| f32_binary(vector(a),vector(b.unwrap()),super::float::F32BinaryOperator::Add);
  F32x4Sub []: V128, Some(WasmType::V128) => |a,b,l| f32_binary(vector(a),vector(b.unwrap()),super::float::F32BinaryOperator::Subtract);
  F32x4Mul []: V128, Some(WasmType::V128) => |a,b,l| f32_binary(vector(a),vector(b.unwrap()),super::float::F32BinaryOperator::Multiply);
  F32x4Div []: V128, Some(WasmType::V128) => |a,b,l| f32_binary(vector(a),vector(b.unwrap()),super::float::F32BinaryOperator::Divide);
  F32x4Min []: V128, Some(WasmType::V128) => |a,b,l| f32_binary(vector(a),vector(b.unwrap()),super::float::F32BinaryOperator::Minimum);
  F32x4Max []: V128, Some(WasmType::V128) => |a,b,l| f32_binary(vector(a),vector(b.unwrap()),super::float::F32BinaryOperator::Maximum);
  F32x4Eq []: V128, Some(WasmType::V128) => |a,b,l| f32_binary(vector(a),vector(b.unwrap()),super::float::F32BinaryOperator::Equal);
  F32x4Ne []: V128, Some(WasmType::V128) => |a,b,l| f32_binary(vector(a),vector(b.unwrap()),super::float::F32BinaryOperator::NotEqual);
  F32x4Lt []: V128, Some(WasmType::V128) => |a,b,l| f32_binary(vector(a),vector(b.unwrap()),super::float::F32BinaryOperator::Less);
  F32x4Gt []: V128, Some(WasmType::V128) => |a,b,l| f32_binary(vector(a),vector(b.unwrap()),super::float::F32BinaryOperator::Greater);
  F32x4Le []: V128, Some(WasmType::V128) => |a,b,l| f32_binary(vector(a),vector(b.unwrap()),super::float::F32BinaryOperator::LessEqual);
  F32x4Ge []: V128, Some(WasmType::V128) => |a,b,l| f32_binary(vector(a),vector(b.unwrap()),super::float::F32BinaryOperator::GreaterEqual);
  F32x4PMin []: V128, Some(WasmType::V128) => |a,b,l| f32_pseudo(vector(a),vector(b.unwrap()),super::float::F32BinaryOperator::Greater);
  F32x4PMax []: V128, Some(WasmType::V128) => |a,b,l| f32_pseudo(vector(a),vector(b.unwrap()),super::float::F32BinaryOperator::Less);
  F32x4Abs []: V128, None => |a,b,l| f32_unary(vector(a),super::float::F32UnaryOperator::Absolute);
  F32x4Neg []: V128, None => |a,b,l| f32_unary(vector(a),super::float::F32UnaryOperator::Negate);
  F32x4Sqrt []: V128, None => |a,b,l| f32_unary(vector(a),super::float::F32UnaryOperator::SquareRoot);
  F32x4Ceil []: V128, None => |a,b,l| f32_unary(vector(a),super::float::F32UnaryOperator::Ceiling);
  F32x4Floor []: V128, None => |a,b,l| f32_unary(vector(a),super::float::F32UnaryOperator::Floor);
  F32x4Trunc []: V128, None => |a,b,l| f32_unary(vector(a),super::float::F32UnaryOperator::Truncate);
  F32x4Nearest []: V128, None => |a,b,l| f32_unary(vector(a),super::float::F32UnaryOperator::Nearest);
  F64x2Add []: V128, Some(WasmType::V128) => |a,b,l| f64_binary(vector(a),vector(b.unwrap()),super::float::F64BinaryOperator::Add);
  F64x2Sub []: V128, Some(WasmType::V128) => |a,b,l| f64_binary(vector(a),vector(b.unwrap()),super::float::F64BinaryOperator::Subtract);
  F64x2Mul []: V128, Some(WasmType::V128) => |a,b,l| f64_binary(vector(a),vector(b.unwrap()),super::float::F64BinaryOperator::Multiply);
  F64x2Div []: V128, Some(WasmType::V128) => |a,b,l| f64_binary(vector(a),vector(b.unwrap()),super::float::F64BinaryOperator::Divide);
  F64x2Min []: V128, Some(WasmType::V128) => |a,b,l| f64_binary(vector(a),vector(b.unwrap()),super::float::F64BinaryOperator::Minimum);
  F64x2Max []: V128, Some(WasmType::V128) => |a,b,l| f64_binary(vector(a),vector(b.unwrap()),super::float::F64BinaryOperator::Maximum);
  F64x2Eq []: V128, Some(WasmType::V128) => |a,b,l| f64_binary(vector(a),vector(b.unwrap()),super::float::F64BinaryOperator::Equal);
  F64x2Ne []: V128, Some(WasmType::V128) => |a,b,l| f64_binary(vector(a),vector(b.unwrap()),super::float::F64BinaryOperator::NotEqual);
  F64x2Lt []: V128, Some(WasmType::V128) => |a,b,l| f64_binary(vector(a),vector(b.unwrap()),super::float::F64BinaryOperator::Less);
  F64x2Gt []: V128, Some(WasmType::V128) => |a,b,l| f64_binary(vector(a),vector(b.unwrap()),super::float::F64BinaryOperator::Greater);
  F64x2Le []: V128, Some(WasmType::V128) => |a,b,l| f64_binary(vector(a),vector(b.unwrap()),super::float::F64BinaryOperator::LessEqual);
  F64x2Ge []: V128, Some(WasmType::V128) => |a,b,l| f64_binary(vector(a),vector(b.unwrap()),super::float::F64BinaryOperator::GreaterEqual);
  F64x2PMin []: V128, Some(WasmType::V128) => |a,b,l| f64_pseudo(vector(a),vector(b.unwrap()),super::float::F64BinaryOperator::Greater);
  F64x2PMax []: V128, Some(WasmType::V128) => |a,b,l| f64_pseudo(vector(a),vector(b.unwrap()),super::float::F64BinaryOperator::Less);
  F64x2Abs []: V128, None => |a,b,l| f64_unary(vector(a),super::float::F64UnaryOperator::Absolute);
  F64x2Neg []: V128, None => |a,b,l| f64_unary(vector(a),super::float::F64UnaryOperator::Negate);
  F64x2Sqrt []: V128, None => |a,b,l| f64_unary(vector(a),super::float::F64UnaryOperator::SquareRoot);
  F64x2Ceil []: V128, None => |a,b,l| f64_unary(vector(a),super::float::F64UnaryOperator::Ceiling);
  F64x2Floor []: V128, None => |a,b,l| f64_unary(vector(a),super::float::F64UnaryOperator::Floor);
  F64x2Trunc []: V128, None => |a,b,l| f64_unary(vector(a),super::float::F64UnaryOperator::Truncate);
  F64x2Nearest []: V128, None => |a,b,l| f64_unary(vector(a),super::float::F64UnaryOperator::Nearest);
  I8x16Eq []: V128, Some(WasmType::V128) => |a,b,l| integer::binary(vector(a),vector(b.unwrap()),u8::BITS,super::integer::I64BinaryOperator::Equal);
  I8x16Ne []: V128, Some(WasmType::V128) => |a,b,l| integer::binary(vector(a),vector(b.unwrap()),u8::BITS,super::integer::I64BinaryOperator::NotEqual);
  I8x16LtS []: V128, Some(WasmType::V128) => |a,b,l| integer::binary(vector(a),vector(b.unwrap()),u8::BITS,super::integer::I64BinaryOperator::LessSigned);
  I8x16GtS []: V128, Some(WasmType::V128) => |a,b,l| integer::binary(vector(a),vector(b.unwrap()),u8::BITS,super::integer::I64BinaryOperator::GreaterSigned);
  I8x16LeS []: V128, Some(WasmType::V128) => |a,b,l| integer::binary(vector(a),vector(b.unwrap()),u8::BITS,super::integer::I64BinaryOperator::LessEqualSigned);
  I8x16GeS []: V128, Some(WasmType::V128) => |a,b,l| integer::binary(vector(a),vector(b.unwrap()),u8::BITS,super::integer::I64BinaryOperator::GreaterEqualSigned);
  I8x16LtU []: V128, Some(WasmType::V128) => |a,b,l| integer::binary(vector(a),vector(b.unwrap()),u8::BITS,super::integer::I64BinaryOperator::LessUnsigned);
  I8x16GtU []: V128, Some(WasmType::V128) => |a,b,l| integer::binary(vector(a),vector(b.unwrap()),u8::BITS,super::integer::I64BinaryOperator::GreaterUnsigned);
  I8x16LeU []: V128, Some(WasmType::V128) => |a,b,l| integer::binary(vector(a),vector(b.unwrap()),u8::BITS,super::integer::I64BinaryOperator::LessEqualUnsigned);
  I8x16GeU []: V128, Some(WasmType::V128) => |a,b,l| integer::binary(vector(a),vector(b.unwrap()),u8::BITS,super::integer::I64BinaryOperator::GreaterEqualUnsigned);
  I8x16Add []: V128, Some(WasmType::V128) => |a,b,l| integer::binary(vector(a),vector(b.unwrap()),u8::BITS,super::integer::I64BinaryOperator::Add);
  I8x16Sub []: V128, Some(WasmType::V128) => |a,b,l| integer::binary(vector(a),vector(b.unwrap()),u8::BITS,super::integer::I64BinaryOperator::Subtract);
  I8x16Abs []: V128, None => |a,b,l| integer::absolute(vector(a),u8::BITS);
  I8x16Neg []: V128, None => |a,b,l| integer::negate(vector(a),u8::BITS);
  I8x16AllTrue []: V128, None => |a,b,l| integer::all_true(vector(a),u8::BITS);
  I8x16Bitmask []: V128, None => |a,b,l| integer::bitmask(vector(a),u8::BITS);
  I8x16Shl []: V128, Some(WasmType::I32) => |a,b,l| integer::shift(vector(a),scalar(b.unwrap()) as u32,u8::BITS,super::integer::I64BinaryOperator::ShiftLeft);
  I8x16ShrS []: V128, Some(WasmType::I32) => |a,b,l| integer::shift(vector(a),scalar(b.unwrap()) as u32,u8::BITS,super::integer::I64BinaryOperator::ShiftRightSigned);
  I8x16ShrU []: V128, Some(WasmType::I32) => |a,b,l| integer::shift(vector(a),scalar(b.unwrap()) as u32,u8::BITS,super::integer::I64BinaryOperator::ShiftRightUnsigned);
  I8x16MinS []: V128, Some(WasmType::V128) => |a,b,l| integer::extrema(vector(a),vector(b.unwrap()),u8::BITS,super::integer::I64BinaryOperator::LessSigned);
  I8x16MinU []: V128, Some(WasmType::V128) => |a,b,l| integer::extrema(vector(a),vector(b.unwrap()),u8::BITS,super::integer::I64BinaryOperator::LessUnsigned);
  I8x16MaxS []: V128, Some(WasmType::V128) => |a,b,l| integer::extrema(vector(a),vector(b.unwrap()),u8::BITS,super::integer::I64BinaryOperator::GreaterSigned);
  I8x16MaxU []: V128, Some(WasmType::V128) => |a,b,l| integer::extrema(vector(a),vector(b.unwrap()),u8::BITS,super::integer::I64BinaryOperator::GreaterUnsigned);
  I8x16AddSatS []: V128, Some(WasmType::V128) => |a,b,l| integer::saturating(vector(a),vector(b.unwrap()),u8::BITS,integer::Signedness::Signed,super::integer::I64BinaryOperator::Add);
  I8x16AddSatU []: V128, Some(WasmType::V128) => |a,b,l| integer::saturating(vector(a),vector(b.unwrap()),u8::BITS,integer::Signedness::Unsigned,super::integer::I64BinaryOperator::Add);
  I8x16SubSatS []: V128, Some(WasmType::V128) => |a,b,l| integer::saturating(vector(a),vector(b.unwrap()),u8::BITS,integer::Signedness::Signed,super::integer::I64BinaryOperator::Subtract);
  I8x16SubSatU []: V128, Some(WasmType::V128) => |a,b,l| integer::saturating(vector(a),vector(b.unwrap()),u8::BITS,integer::Signedness::Unsigned,super::integer::I64BinaryOperator::Subtract);
  I8x16AvgrU []: V128, Some(WasmType::V128) => |a,b,l| integer::average(vector(a),vector(b.unwrap()),u8::BITS);
  I8x16NarrowI16x8S []: V128, Some(WasmType::V128) => |a,b,l| integer::narrow(vector(a),vector(b.unwrap()),u8::BITS,integer::Signedness::Signed);
  I8x16NarrowI16x8U []: V128, Some(WasmType::V128) => |a,b,l| integer::narrow(vector(a),vector(b.unwrap()),u8::BITS,integer::Signedness::Unsigned);
  I16x8Eq []: V128, Some(WasmType::V128) => |a,b,l| integer::binary(vector(a),vector(b.unwrap()),u16::BITS,super::integer::I64BinaryOperator::Equal);
  I16x8Ne []: V128, Some(WasmType::V128) => |a,b,l| integer::binary(vector(a),vector(b.unwrap()),u16::BITS,super::integer::I64BinaryOperator::NotEqual);
  I16x8LtS []: V128, Some(WasmType::V128) => |a,b,l| integer::binary(vector(a),vector(b.unwrap()),u16::BITS,super::integer::I64BinaryOperator::LessSigned);
  I16x8GtS []: V128, Some(WasmType::V128) => |a,b,l| integer::binary(vector(a),vector(b.unwrap()),u16::BITS,super::integer::I64BinaryOperator::GreaterSigned);
  I16x8LeS []: V128, Some(WasmType::V128) => |a,b,l| integer::binary(vector(a),vector(b.unwrap()),u16::BITS,super::integer::I64BinaryOperator::LessEqualSigned);
  I16x8GeS []: V128, Some(WasmType::V128) => |a,b,l| integer::binary(vector(a),vector(b.unwrap()),u16::BITS,super::integer::I64BinaryOperator::GreaterEqualSigned);
  I16x8LtU []: V128, Some(WasmType::V128) => |a,b,l| integer::binary(vector(a),vector(b.unwrap()),u16::BITS,super::integer::I64BinaryOperator::LessUnsigned);
  I16x8GtU []: V128, Some(WasmType::V128) => |a,b,l| integer::binary(vector(a),vector(b.unwrap()),u16::BITS,super::integer::I64BinaryOperator::GreaterUnsigned);
  I16x8LeU []: V128, Some(WasmType::V128) => |a,b,l| integer::binary(vector(a),vector(b.unwrap()),u16::BITS,super::integer::I64BinaryOperator::LessEqualUnsigned);
  I16x8GeU []: V128, Some(WasmType::V128) => |a,b,l| integer::binary(vector(a),vector(b.unwrap()),u16::BITS,super::integer::I64BinaryOperator::GreaterEqualUnsigned);
  I16x8Add []: V128, Some(WasmType::V128) => |a,b,l| integer::binary(vector(a),vector(b.unwrap()),u16::BITS,super::integer::I64BinaryOperator::Add);
  I16x8Sub []: V128, Some(WasmType::V128) => |a,b,l| integer::binary(vector(a),vector(b.unwrap()),u16::BITS,super::integer::I64BinaryOperator::Subtract);
  I16x8Mul []: V128, Some(WasmType::V128) => |a,b,l| integer::binary(vector(a),vector(b.unwrap()),u16::BITS,super::integer::I64BinaryOperator::Multiply);
  I16x8Abs []: V128, None => |a,b,l| integer::absolute(vector(a),u16::BITS);
  I16x8Neg []: V128, None => |a,b,l| integer::negate(vector(a),u16::BITS);
  I16x8AllTrue []: V128, None => |a,b,l| integer::all_true(vector(a),u16::BITS);
  I16x8Bitmask []: V128, None => |a,b,l| integer::bitmask(vector(a),u16::BITS);
  I16x8Shl []: V128, Some(WasmType::I32) => |a,b,l| integer::shift(vector(a),scalar(b.unwrap()) as u32,u16::BITS,super::integer::I64BinaryOperator::ShiftLeft);
  I16x8ShrS []: V128, Some(WasmType::I32) => |a,b,l| integer::shift(vector(a),scalar(b.unwrap()) as u32,u16::BITS,super::integer::I64BinaryOperator::ShiftRightSigned);
  I16x8ShrU []: V128, Some(WasmType::I32) => |a,b,l| integer::shift(vector(a),scalar(b.unwrap()) as u32,u16::BITS,super::integer::I64BinaryOperator::ShiftRightUnsigned);
  I16x8MinS []: V128, Some(WasmType::V128) => |a,b,l| integer::extrema(vector(a),vector(b.unwrap()),u16::BITS,super::integer::I64BinaryOperator::LessSigned);
  I16x8MinU []: V128, Some(WasmType::V128) => |a,b,l| integer::extrema(vector(a),vector(b.unwrap()),u16::BITS,super::integer::I64BinaryOperator::LessUnsigned);
  I16x8MaxS []: V128, Some(WasmType::V128) => |a,b,l| integer::extrema(vector(a),vector(b.unwrap()),u16::BITS,super::integer::I64BinaryOperator::GreaterSigned);
  I16x8MaxU []: V128, Some(WasmType::V128) => |a,b,l| integer::extrema(vector(a),vector(b.unwrap()),u16::BITS,super::integer::I64BinaryOperator::GreaterUnsigned);
  I16x8AddSatS []: V128, Some(WasmType::V128) => |a,b,l| integer::saturating(vector(a),vector(b.unwrap()),u16::BITS,integer::Signedness::Signed,super::integer::I64BinaryOperator::Add);
  I16x8AddSatU []: V128, Some(WasmType::V128) => |a,b,l| integer::saturating(vector(a),vector(b.unwrap()),u16::BITS,integer::Signedness::Unsigned,super::integer::I64BinaryOperator::Add);
  I16x8SubSatS []: V128, Some(WasmType::V128) => |a,b,l| integer::saturating(vector(a),vector(b.unwrap()),u16::BITS,integer::Signedness::Signed,super::integer::I64BinaryOperator::Subtract);
  I16x8SubSatU []: V128, Some(WasmType::V128) => |a,b,l| integer::saturating(vector(a),vector(b.unwrap()),u16::BITS,integer::Signedness::Unsigned,super::integer::I64BinaryOperator::Subtract);
  I16x8AvgrU []: V128, Some(WasmType::V128) => |a,b,l| integer::average(vector(a),vector(b.unwrap()),u16::BITS);
  I16x8ExtendLowI8x16S []: V128, None => |a,b,l| integer::extend(vector(a),u16::BITS,integer::Signedness::Signed,integer::Half::Low);
  I16x8ExtMulLowI8x16S []: V128, Some(WasmType::V128) => |a,b,l| integer::extend_multiply(vector(a),vector(b.unwrap()),u16::BITS,integer::Signedness::Signed,integer::Half::Low);
  I16x8ExtendLowI8x16U []: V128, None => |a,b,l| integer::extend(vector(a),u16::BITS,integer::Signedness::Unsigned,integer::Half::Low);
  I16x8ExtMulLowI8x16U []: V128, Some(WasmType::V128) => |a,b,l| integer::extend_multiply(vector(a),vector(b.unwrap()),u16::BITS,integer::Signedness::Unsigned,integer::Half::Low);
  I16x8ExtendHighI8x16S []: V128, None => |a,b,l| integer::extend(vector(a),u16::BITS,integer::Signedness::Signed,integer::Half::High);
  I16x8ExtMulHighI8x16S []: V128, Some(WasmType::V128) => |a,b,l| integer::extend_multiply(vector(a),vector(b.unwrap()),u16::BITS,integer::Signedness::Signed,integer::Half::High);
  I16x8ExtendHighI8x16U []: V128, None => |a,b,l| integer::extend(vector(a),u16::BITS,integer::Signedness::Unsigned,integer::Half::High);
  I16x8ExtMulHighI8x16U []: V128, Some(WasmType::V128) => |a,b,l| integer::extend_multiply(vector(a),vector(b.unwrap()),u16::BITS,integer::Signedness::Unsigned,integer::Half::High);
  I16x8ExtAddPairwiseI8x16S []: V128, None => |a,b,l| integer::pairwise_add(vector(a),u16::BITS,integer::Signedness::Signed);
  I16x8ExtAddPairwiseI8x16U []: V128, None => |a,b,l| integer::pairwise_add(vector(a),u16::BITS,integer::Signedness::Unsigned);
  I16x8NarrowI32x4S []: V128, Some(WasmType::V128) => |a,b,l| integer::narrow(vector(a),vector(b.unwrap()),u16::BITS,integer::Signedness::Signed);
  I16x8NarrowI32x4U []: V128, Some(WasmType::V128) => |a,b,l| integer::narrow(vector(a),vector(b.unwrap()),u16::BITS,integer::Signedness::Unsigned);
  I32x4Eq []: V128, Some(WasmType::V128) => |a,b,l| integer::binary(vector(a),vector(b.unwrap()),u32::BITS,super::integer::I64BinaryOperator::Equal);
  I32x4Ne []: V128, Some(WasmType::V128) => |a,b,l| integer::binary(vector(a),vector(b.unwrap()),u32::BITS,super::integer::I64BinaryOperator::NotEqual);
  I32x4LtS []: V128, Some(WasmType::V128) => |a,b,l| integer::binary(vector(a),vector(b.unwrap()),u32::BITS,super::integer::I64BinaryOperator::LessSigned);
  I32x4GtS []: V128, Some(WasmType::V128) => |a,b,l| integer::binary(vector(a),vector(b.unwrap()),u32::BITS,super::integer::I64BinaryOperator::GreaterSigned);
  I32x4LeS []: V128, Some(WasmType::V128) => |a,b,l| integer::binary(vector(a),vector(b.unwrap()),u32::BITS,super::integer::I64BinaryOperator::LessEqualSigned);
  I32x4GeS []: V128, Some(WasmType::V128) => |a,b,l| integer::binary(vector(a),vector(b.unwrap()),u32::BITS,super::integer::I64BinaryOperator::GreaterEqualSigned);
  I32x4LtU []: V128, Some(WasmType::V128) => |a,b,l| integer::binary(vector(a),vector(b.unwrap()),u32::BITS,super::integer::I64BinaryOperator::LessUnsigned);
  I32x4GtU []: V128, Some(WasmType::V128) => |a,b,l| integer::binary(vector(a),vector(b.unwrap()),u32::BITS,super::integer::I64BinaryOperator::GreaterUnsigned);
  I32x4LeU []: V128, Some(WasmType::V128) => |a,b,l| integer::binary(vector(a),vector(b.unwrap()),u32::BITS,super::integer::I64BinaryOperator::LessEqualUnsigned);
  I32x4GeU []: V128, Some(WasmType::V128) => |a,b,l| integer::binary(vector(a),vector(b.unwrap()),u32::BITS,super::integer::I64BinaryOperator::GreaterEqualUnsigned);
  I32x4Add []: V128, Some(WasmType::V128) => |a,b,l| integer::binary(vector(a),vector(b.unwrap()),u32::BITS,super::integer::I64BinaryOperator::Add);
  I32x4Sub []: V128, Some(WasmType::V128) => |a,b,l| integer::binary(vector(a),vector(b.unwrap()),u32::BITS,super::integer::I64BinaryOperator::Subtract);
  I32x4Mul []: V128, Some(WasmType::V128) => |a,b,l| integer::binary(vector(a),vector(b.unwrap()),u32::BITS,super::integer::I64BinaryOperator::Multiply);
  I32x4Abs []: V128, None => |a,b,l| integer::absolute(vector(a),u32::BITS);
  I32x4Neg []: V128, None => |a,b,l| integer::negate(vector(a),u32::BITS);
  I32x4AllTrue []: V128, None => |a,b,l| integer::all_true(vector(a),u32::BITS);
  I32x4Bitmask []: V128, None => |a,b,l| integer::bitmask(vector(a),u32::BITS);
  I32x4Shl []: V128, Some(WasmType::I32) => |a,b,l| integer::shift(vector(a),scalar(b.unwrap()) as u32,u32::BITS,super::integer::I64BinaryOperator::ShiftLeft);
  I32x4ShrS []: V128, Some(WasmType::I32) => |a,b,l| integer::shift(vector(a),scalar(b.unwrap()) as u32,u32::BITS,super::integer::I64BinaryOperator::ShiftRightSigned);
  I32x4ShrU []: V128, Some(WasmType::I32) => |a,b,l| integer::shift(vector(a),scalar(b.unwrap()) as u32,u32::BITS,super::integer::I64BinaryOperator::ShiftRightUnsigned);
  I32x4MinS []: V128, Some(WasmType::V128) => |a,b,l| integer::extrema(vector(a),vector(b.unwrap()),u32::BITS,super::integer::I64BinaryOperator::LessSigned);
  I32x4MinU []: V128, Some(WasmType::V128) => |a,b,l| integer::extrema(vector(a),vector(b.unwrap()),u32::BITS,super::integer::I64BinaryOperator::LessUnsigned);
  I32x4MaxS []: V128, Some(WasmType::V128) => |a,b,l| integer::extrema(vector(a),vector(b.unwrap()),u32::BITS,super::integer::I64BinaryOperator::GreaterSigned);
  I32x4MaxU []: V128, Some(WasmType::V128) => |a,b,l| integer::extrema(vector(a),vector(b.unwrap()),u32::BITS,super::integer::I64BinaryOperator::GreaterUnsigned);
  I32x4ExtendLowI16x8S []: V128, None => |a,b,l| integer::extend(vector(a),u32::BITS,integer::Signedness::Signed,integer::Half::Low);
  I32x4ExtMulLowI16x8S []: V128, Some(WasmType::V128) => |a,b,l| integer::extend_multiply(vector(a),vector(b.unwrap()),u32::BITS,integer::Signedness::Signed,integer::Half::Low);
  I32x4ExtendLowI16x8U []: V128, None => |a,b,l| integer::extend(vector(a),u32::BITS,integer::Signedness::Unsigned,integer::Half::Low);
  I32x4ExtMulLowI16x8U []: V128, Some(WasmType::V128) => |a,b,l| integer::extend_multiply(vector(a),vector(b.unwrap()),u32::BITS,integer::Signedness::Unsigned,integer::Half::Low);
  I32x4ExtendHighI16x8S []: V128, None => |a,b,l| integer::extend(vector(a),u32::BITS,integer::Signedness::Signed,integer::Half::High);
  I32x4ExtMulHighI16x8S []: V128, Some(WasmType::V128) => |a,b,l| integer::extend_multiply(vector(a),vector(b.unwrap()),u32::BITS,integer::Signedness::Signed,integer::Half::High);
  I32x4ExtendHighI16x8U []: V128, None => |a,b,l| integer::extend(vector(a),u32::BITS,integer::Signedness::Unsigned,integer::Half::High);
  I32x4ExtMulHighI16x8U []: V128, Some(WasmType::V128) => |a,b,l| integer::extend_multiply(vector(a),vector(b.unwrap()),u32::BITS,integer::Signedness::Unsigned,integer::Half::High);
  I32x4ExtAddPairwiseI16x8S []: V128, None => |a,b,l| integer::pairwise_add(vector(a),u32::BITS,integer::Signedness::Signed);
  I32x4ExtAddPairwiseI16x8U []: V128, None => |a,b,l| integer::pairwise_add(vector(a),u32::BITS,integer::Signedness::Unsigned);
  I64x2Eq []: V128, Some(WasmType::V128) => |a,b,l| integer::binary(vector(a),vector(b.unwrap()),u64::BITS,super::integer::I64BinaryOperator::Equal);
  I64x2Ne []: V128, Some(WasmType::V128) => |a,b,l| integer::binary(vector(a),vector(b.unwrap()),u64::BITS,super::integer::I64BinaryOperator::NotEqual);
  I64x2LtS []: V128, Some(WasmType::V128) => |a,b,l| integer::binary(vector(a),vector(b.unwrap()),u64::BITS,super::integer::I64BinaryOperator::LessSigned);
  I64x2GtS []: V128, Some(WasmType::V128) => |a,b,l| integer::binary(vector(a),vector(b.unwrap()),u64::BITS,super::integer::I64BinaryOperator::GreaterSigned);
  I64x2LeS []: V128, Some(WasmType::V128) => |a,b,l| integer::binary(vector(a),vector(b.unwrap()),u64::BITS,super::integer::I64BinaryOperator::LessEqualSigned);
  I64x2GeS []: V128, Some(WasmType::V128) => |a,b,l| integer::binary(vector(a),vector(b.unwrap()),u64::BITS,super::integer::I64BinaryOperator::GreaterEqualSigned);
  I64x2Add []: V128, Some(WasmType::V128) => |a,b,l| integer::binary(vector(a),vector(b.unwrap()),u64::BITS,super::integer::I64BinaryOperator::Add);
  I64x2Sub []: V128, Some(WasmType::V128) => |a,b,l| integer::binary(vector(a),vector(b.unwrap()),u64::BITS,super::integer::I64BinaryOperator::Subtract);
  I64x2Mul []: V128, Some(WasmType::V128) => |a,b,l| integer::binary(vector(a),vector(b.unwrap()),u64::BITS,super::integer::I64BinaryOperator::Multiply);
  I64x2Abs []: V128, None => |a,b,l| integer::absolute(vector(a),u64::BITS);
  I64x2Neg []: V128, None => |a,b,l| integer::negate(vector(a),u64::BITS);
  I64x2AllTrue []: V128, None => |a,b,l| integer::all_true(vector(a),u64::BITS);
  I64x2Bitmask []: V128, None => |a,b,l| integer::bitmask(vector(a),u64::BITS);
  I64x2Shl []: V128, Some(WasmType::I32) => |a,b,l| integer::shift(vector(a),scalar(b.unwrap()) as u32,u64::BITS,super::integer::I64BinaryOperator::ShiftLeft);
  I64x2ShrS []: V128, Some(WasmType::I32) => |a,b,l| integer::shift(vector(a),scalar(b.unwrap()) as u32,u64::BITS,super::integer::I64BinaryOperator::ShiftRightSigned);
  I64x2ShrU []: V128, Some(WasmType::I32) => |a,b,l| integer::shift(vector(a),scalar(b.unwrap()) as u32,u64::BITS,super::integer::I64BinaryOperator::ShiftRightUnsigned);
  I64x2ExtendLowI32x4S []: V128, None => |a,b,l| integer::extend(vector(a),u64::BITS,integer::Signedness::Signed,integer::Half::Low);
  I64x2ExtMulLowI32x4S []: V128, Some(WasmType::V128) => |a,b,l| integer::extend_multiply(vector(a),vector(b.unwrap()),u64::BITS,integer::Signedness::Signed,integer::Half::Low);
  I64x2ExtendLowI32x4U []: V128, None => |a,b,l| integer::extend(vector(a),u64::BITS,integer::Signedness::Unsigned,integer::Half::Low);
  I64x2ExtMulLowI32x4U []: V128, Some(WasmType::V128) => |a,b,l| integer::extend_multiply(vector(a),vector(b.unwrap()),u64::BITS,integer::Signedness::Unsigned,integer::Half::Low);
  I64x2ExtendHighI32x4S []: V128, None => |a,b,l| integer::extend(vector(a),u64::BITS,integer::Signedness::Signed,integer::Half::High);
  I64x2ExtMulHighI32x4S []: V128, Some(WasmType::V128) => |a,b,l| integer::extend_multiply(vector(a),vector(b.unwrap()),u64::BITS,integer::Signedness::Signed,integer::Half::High);
  I64x2ExtendHighI32x4U []: V128, None => |a,b,l| integer::extend(vector(a),u64::BITS,integer::Signedness::Unsigned,integer::Half::High);
  I64x2ExtMulHighI32x4U []: V128, Some(WasmType::V128) => |a,b,l| integer::extend_multiply(vector(a),vector(b.unwrap()),u64::BITS,integer::Signedness::Unsigned,integer::Half::High);
  I8x16Popcnt []: V128, None => |a,b,l| integer::population_count(vector(a));
  I16x8Q15MulrSatS []: V128, Some(WasmType::V128) => |a,b,l| integer::q15(vector(a),vector(b.unwrap()));
  I32x4DotI16x8S []: V128, Some(WasmType::V128) => |a,b,l| integer::dot(vector(a),vector(b.unwrap()));
  I32x4TruncSatF32x4S []: V128, None => |a,b,l| convert(vector(a), super::conversion::ScalarConversionOperator::SaturateF32SignedI32);
  I32x4TruncSatF32x4U []: V128, None => |a,b,l| convert(vector(a), super::conversion::ScalarConversionOperator::SaturateF32UnsignedI32);
  F32x4ConvertI32x4S []: V128, None => |a,b,l| convert(vector(a), super::conversion::ScalarConversionOperator::ConvertI32SignedF32);
  F32x4ConvertI32x4U []: V128, None => |a,b,l| convert(vector(a), super::conversion::ScalarConversionOperator::ConvertI32UnsignedF32);
  I32x4TruncSatF64x2SZero []: V128, None => |a,b,l| convert(vector(a), super::conversion::ScalarConversionOperator::SaturateF64SignedI32);
  I32x4TruncSatF64x2UZero []: V128, None => |a,b,l| convert(vector(a), super::conversion::ScalarConversionOperator::SaturateF64UnsignedI32);
  F64x2ConvertLowI32x4S []: V128, None => |a,b,l| convert(vector(a), super::conversion::ScalarConversionOperator::ConvertI32SignedF64);
  F64x2ConvertLowI32x4U []: V128, None => |a,b,l| convert(vector(a), super::conversion::ScalarConversionOperator::ConvertI32UnsignedF64);
  F32x4DemoteF64x2Zero []: V128, None => |a,b,l| convert(vector(a), super::conversion::ScalarConversionOperator::DemoteF64);
  F64x2PromoteLowF32x4 []: V128, None => |a,b,l| convert(vector(a), super::conversion::ScalarConversionOperator::PromoteF32);
  // Choose deterministic outcomes admitted by the relaxed SIMD contract.
  I8x16RelaxedSwizzle []: V128, Some(WasmType::V128) => |a,b,l| Self::I8x16Swizzle.apply(a,b,l);
  I32x4RelaxedTruncF32x4S []: V128, None => |a,b,l| Self::I32x4TruncSatF32x4S.apply(a,b,l);
  I32x4RelaxedTruncF32x4U []: V128, None => |a,b,l| Self::I32x4TruncSatF32x4U.apply(a,b,l);
  I32x4RelaxedTruncF64x2SZero []: V128, None => |a,b,l| Self::I32x4TruncSatF64x2SZero.apply(a,b,l);
  I32x4RelaxedTruncF64x2UZero []: V128, None => |a,b,l| Self::I32x4TruncSatF64x2UZero.apply(a,b,l);
  F32x4RelaxedMin []: V128, Some(WasmType::V128) => |a,b,l| Self::F32x4Min.apply(a,b,l);
  F32x4RelaxedMax []: V128, Some(WasmType::V128) => |a,b,l| Self::F32x4Max.apply(a,b,l);
  F64x2RelaxedMin []: V128, Some(WasmType::V128) => |a,b,l| Self::F64x2Min.apply(a,b,l);
  F64x2RelaxedMax []: V128, Some(WasmType::V128) => |a,b,l| Self::F64x2Max.apply(a,b,l);
  I16x8RelaxedQ15mulrS []: V128, Some(WasmType::V128) => |a,b,l| Self::I16x8Q15MulrSatS.apply(a,b,l);
  I16x8RelaxedDotI8x16I7x16S []: V128, Some(WasmType::V128) => |a,b,l| integer::relaxed_dot(vector(a),vector(b.unwrap()));
}

fn swizzle(bits: u128, indices: u128) -> u128 {
    let bytes = bits.to_le_bytes();
    u128::from_le_bytes(
        indices
            .to_le_bytes()
            .map(|index| bytes.get(index as usize).copied().unwrap_or(0)),
    )
}
pub(crate) fn shuffle(left: u128, right: u128, indices: [u8; V128_BYTES]) -> u128 {
    // Both operands' bytes in index order: validation keeps every index below their count
    // (`shuffle_indices_valid`), which the mask restates without a branch.
    let mut both = [0u8; SHUFFLE_SOURCE_BYTES];
    both[..V128_BYTES].copy_from_slice(&left.to_le_bytes());
    both[V128_BYTES..].copy_from_slice(&right.to_le_bytes());
    u128::from_le_bytes(indices.map(|index| both[usize::from(index) & (SHUFFLE_SOURCE_BYTES - 1)]))
}
/// The bytes a shuffle selects from: both operands'.
const SHUFFLE_SOURCE_BYTES: usize = V128_BYTES * 2;
const _: () = assert!(SHUFFLE_SOURCE_BYTES.is_power_of_two());
pub(crate) fn shuffle_indices_valid(indices: &[u8; V128_BYTES]) -> bool {
    indices
        .iter()
        .all(|&index| (index as usize) < SHUFFLE_SOURCE_BYTES)
}

#[derive(Clone, Copy)]
enum TernaryComposition {
    Bitselect,
    MultiplyAdd {
        negate: Option<SimdOperator>,
        multiply: SimdOperator,
        add: SimdOperator,
    },
    DotAdd,
}

macro_rules! ternary_compositions {
    ($($wasm:ident => $composition:expr;)+) => {
        impl TernaryComposition {
            fn from_wasm(op: &wasmparser::Operator<'_>) -> Option<Self> {
                use SimdOperator::*;
                match op { $(wasmparser::Operator::$wasm => Some($composition),)+ _ => None }
            }
        }
    }
}
ternary_compositions! {
    V128Bitselect => Self::Bitselect;
    I8x16RelaxedLaneselect => Self::Bitselect;
    I16x8RelaxedLaneselect => Self::Bitselect;
    I32x4RelaxedLaneselect => Self::Bitselect;
    I64x2RelaxedLaneselect => Self::Bitselect;
    F32x4RelaxedMadd => Self::MultiplyAdd { negate: None, multiply: F32x4Mul, add: F32x4Add };
    F32x4RelaxedNmadd => Self::MultiplyAdd { negate: Some(F32x4Neg), multiply: F32x4Mul, add: F32x4Add };
    F64x2RelaxedMadd => Self::MultiplyAdd { negate: None, multiply: F64x2Mul, add: F64x2Add };
    F64x2RelaxedNmadd => Self::MultiplyAdd { negate: Some(F64x2Neg), multiply: F64x2Mul, add: F64x2Add };
    I32x4RelaxedDotI8x16I7x16AddS => Self::DotAdd;
}

/// Whether `op` lowers through `Lowering::simd_operator`'s pop and push alone, so an aliased
/// operand may be read from its local register: lane operators and shuffles, not the
/// ternary compositions, which write an operand position.
pub(super) fn reads_aliases(op: &wasmparser::Operator<'_>) -> bool {
    SimdOperator::from_wasm(op).is_some() || matches!(op, wasmparser::Operator::I8x16Shuffle { .. })
}

impl super::Lowering<'_> {
    fn emit_simd(
        &mut self,
        op: SimdOperator,
        lane: u8,
        result: Register,
        left: Register,
        right: Register,
    ) -> Result<(), crate::Diagnostic> {
        self.emit(Op::WasmSimd, result, left, right, op.selector(lane))
    }
    pub(super) fn simd_operator(
        &mut self,
        operator: &wasmparser::Operator<'_>,
    ) -> Result<bool, crate::Diagnostic> {
        if self.ternary_simd_operator(operator)? {
            return Ok(true);
        }
        if let Some((op, lane)) = SimdOperator::from_wasm(operator) {
            if SimdOperator::from_selector(op.selector(lane)).is_none() {
                return Err(crate::Diagnostic::unsupported(
                    self.name,
                    "invalid SIMD lane",
                ));
            }
            if self.path == super::Reachability::Dead {
                return Ok(true);
            }
            let right = if op.right_type().is_some() {
                self.pop()?
            } else {
                0
            };
            let left = self.pop()?;
            let result = self.push()?;
            self.emit_simd(op, lane, result, left, right)?;
            self.produced(result);
            return Ok(true);
        }
        match operator {
            wasmparser::Operator::I8x16Shuffle { lanes } => {
                if !shuffle_indices_valid(lanes) {
                    return Err(crate::Diagnostic::unsupported(
                        self.name,
                        "invalid SIMD shuffle",
                    ));
                }
                if self.path == super::Reachability::Dead {
                    return Ok(true);
                }
                let right = self.pop()?;
                let left = self.pop()?;
                let result = self.push()?;
                let constant =
                    self.scalar_constant(WasmValue::V128(u128::from_le_bytes(*lanes)))?;
                self.emit(Op::WasmSimdShuffle, result, left, right, constant)?;
                self.produced(result);
            }
            _ => return Ok(false),
        }
        Ok(true)
    }
}

impl super::Lowering<'_> {
    fn ternary_simd_operator(
        &mut self,
        op: &wasmparser::Operator<'_>,
    ) -> Result<bool, crate::Diagnostic> {
        let Some(composition) = TernaryComposition::from_wasm(op) else {
            return Ok(false);
        };
        if self.path == super::Reachability::Dead {
            return Ok(true);
        }
        let third = self.pop()?;
        let right = self.pop()?;
        let left = self.pop()?;
        let depth = self.depth;
        self.depth = third + 1;
        let first = self.push()?;
        match composition {
            TernaryComposition::Bitselect => {
                let second = self.push()?;
                self.emit_simd(SimdOperator::V128And, 0, first, left, third)?;
                self.emit_simd(SimdOperator::V128AndNot, 0, second, right, third)?;
                self.emit_simd(SimdOperator::V128Or, 0, left, first, second)?;
            }
            TernaryComposition::MultiplyAdd {
                negate,
                multiply,
                add,
            } => {
                let input = if let Some(negate) = negate {
                    self.emit_simd(negate, 0, first, left, 0)?;
                    first
                } else {
                    left
                };
                self.emit_simd(multiply, 0, first, input, right)?;
                self.emit_simd(add, 0, left, first, third)?;
            }
            TernaryComposition::DotAdd => {
                self.emit_simd(
                    SimdOperator::I16x8RelaxedDotI8x16I7x16S,
                    0,
                    first,
                    left,
                    right,
                )?;
                self.emit_simd(SimdOperator::I32x4ExtAddPairwiseI16x8S, 0, first, first, 0)?;
                self.emit_simd(SimdOperator::I32x4Add, 0, left, first, third)?;
            }
        }
        self.depth = depth;
        self.push()?;
        Ok(true)
    }
}

/// The lane operators an unboxed execution computes on bits directly, without the
/// `WasmValue` boundary of `SimdKernel`: each is exactly its operator's kernel, which
/// `bit_kernels_match_their_operators` checks lane by lane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BitKernel {
    And,
    AndNot,
    Or,
    Xor,
    I32x4Add,
    I32x4Sub,
    I64x2Add,
    I64x2Sub,
    I64x2Mul,
    /// The u64 products of the low two u32 lanes (`_mm_mul_epu32` past its shuffles).
    I64x2ExtMulLowI32x4U,
    /// A lane shift whose count the caller fixed; the kernel reduces it modulo the lane
    /// width, as the operator does.
    Shift(BitShift),
}

/// A lane shift by a fixed count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BitShift {
    I32x4Shl,
    I32x4ShrU,
    I32x4ShrS,
    I64x2Shl,
    I64x2ShrU,
    I64x2ShrS,
    /// `x << k | x >> (64 - k)` on each u64 lane, as `i64x2.shl`, `i64x2.shr_u` and
    /// `v128.or` compute it: a rotation by `k` modulo the lane width.
    I64x2Rotl,
}

/// The u64 lanes of `bits`, low first.
#[inline(always)]
fn u64_lanes(bits: u128) -> [u64; 2] {
    [bits as u64, (bits >> u64::BITS) as u64]
}
#[inline(always)]
fn from_u64_lanes([low, high]: [u64; 2]) -> u128 {
    u128::from(low) | (u128::from(high) << u64::BITS)
}
#[inline(always)]
fn u32_lanes(bits: u128) -> [u32; 4] {
    std::array::from_fn(|lane| (bits >> (u32::BITS * lane as u32)) as u32)
}
#[inline(always)]
fn from_u32_lanes(lanes: [u32; 4]) -> u128 {
    lanes
        .iter()
        .enumerate()
        .fold(0, |bits, (lane, &value)| bits | (u128::from(value) << (u32::BITS * lane as u32)))
}

impl BitKernel {
    /// The bit kernel of `selector`'s operator: one whose operands are vectors, or a shift
    /// whose count the caller knows (`count`).
    pub(crate) fn of(selector: u32, count: Option<i32>) -> Option<Self> {
        use SimdOperator as S;
        let (operator, _) = SimdOperator::from_selector(selector)?;
        let shift = |shift: BitShift| count.map(|_| Self::Shift(shift));
        match operator {
            S::V128And => Some(Self::And),
            S::V128AndNot => Some(Self::AndNot),
            S::V128Or => Some(Self::Or),
            S::V128Xor => Some(Self::Xor),
            S::I32x4Add => Some(Self::I32x4Add),
            S::I32x4Sub => Some(Self::I32x4Sub),
            S::I64x2Add => Some(Self::I64x2Add),
            S::I64x2Sub => Some(Self::I64x2Sub),
            S::I64x2Mul => Some(Self::I64x2Mul),
            S::I64x2ExtMulLowI32x4U => Some(Self::I64x2ExtMulLowI32x4U),
            S::I32x4Shl => shift(BitShift::I32x4Shl),
            S::I32x4ShrU => shift(BitShift::I32x4ShrU),
            S::I32x4ShrS => shift(BitShift::I32x4ShrS),
            S::I64x2Shl => shift(BitShift::I64x2Shl),
            S::I64x2ShrU => shift(BitShift::I64x2ShrU),
            S::I64x2ShrS => shift(BitShift::I64x2ShrS),
            _ => None,
        }
    }

    /// The operator on `left` and `right`: a vector, or for a shift the count in its low bits.
    #[inline(always)]
    pub(crate) fn apply(self, left: u128, right: u128) -> u128 {
        let pairs = |operation: fn(u64, u64) -> u64| {
            let (a, b) = (u64_lanes(left), u64_lanes(right));
            from_u64_lanes([operation(a[0], b[0]), operation(a[1], b[1])])
        };
        let quads = |operation: fn(u32, u32) -> u32| {
            let (a, b) = (u32_lanes(left), u32_lanes(right));
            from_u32_lanes(std::array::from_fn(|lane| operation(a[lane], b[lane])))
        };
        match self {
            Self::And => left & right,
            Self::AndNot => left & !right,
            Self::Or => left | right,
            Self::Xor => left ^ right,
            Self::I32x4Add => quads(u32::wrapping_add),
            Self::I32x4Sub => quads(u32::wrapping_sub),
            Self::I64x2Add => pairs(u64::wrapping_add),
            Self::I64x2Sub => pairs(u64::wrapping_sub),
            Self::I64x2Mul => pairs(u64::wrapping_mul),
            Self::I64x2ExtMulLowI32x4U => {
                let (a, b) = (u32_lanes(left), u32_lanes(right));
                from_u64_lanes([
                    u64::from(a[0]) * u64::from(b[0]),
                    u64::from(a[1]) * u64::from(b[1]),
                ])
            }
            Self::Shift(shift) => {
                let count = right as u32;
                let (narrow, wide) = (count % u32::BITS, count % u64::BITS);
                match shift {
                    BitShift::I32x4Shl => from_u32_lanes(u32_lanes(left).map(|lane| lane << narrow)),
                    BitShift::I32x4ShrU => from_u32_lanes(u32_lanes(left).map(|lane| lane >> narrow)),
                    BitShift::I32x4ShrS => {
                        from_u32_lanes(u32_lanes(left).map(|lane| ((lane as i32) >> narrow) as u32))
                    }
                    BitShift::I64x2Shl => from_u64_lanes(u64_lanes(left).map(|lane| lane << wide)),
                    BitShift::I64x2ShrU => from_u64_lanes(u64_lanes(left).map(|lane| lane >> wide)),
                    BitShift::I64x2ShrS => {
                        from_u64_lanes(u64_lanes(left).map(|lane| ((lane as i64) >> wide) as u64))
                    }
                    BitShift::I64x2Rotl => from_u64_lanes(u64_lanes(left).map(|lane| lane.rotate_left(wide))),
                }
            }
        }
    }
}

/// A shuffle's lane indices in the coarsest lanes they move whole: aligned u32 lanes of the
/// two operands, or bytes. Each form selects exactly what `shuffle` selects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ShuffleForm {
    /// Per result u32 lane, the operands' u32 lane it copies (right's from `WORD_LANES`).
    Words([u8; WORD_LANES]),
    Bytes([u8; V128_BYTES]),
}

/// A vector's u32 lanes.
const WORD_LANES: usize = V128_BYTES / size_of::<u32>();
/// The u32 lanes a shuffle selects from: both operands'.
const SHUFFLE_SOURCE_WORDS: usize = WORD_LANES * 2;

/// `i64x2.extmul_low_i32x4_u` of two vectors each shuffled from one operand by u32 lanes
/// (`_mm_mul_epu32`): per result u64 lane, the product of the operands' u32 lanes it names.
#[inline(always)]
pub(crate) fn multiply_words(left: u128, right: u128, left_words: [u8; 2], right_words: [u8; 2]) -> u128 {
    let (left, right) = (u32_lanes(left), u32_lanes(right));
    let product = |lane: usize| {
        u64::from(left[usize::from(left_words[lane]) % WORD_LANES])
            * u64::from(right[usize::from(right_words[lane]) % WORD_LANES])
    };
    from_u64_lanes([product(0), product(1)])
}

impl ShuffleForm {
    pub(crate) fn of(indices: [u8; V128_BYTES]) -> Self {
        let width = size_of::<u32>();
        let words: Option<Vec<u8>> = indices
            .chunks_exact(width)
            .map(|word| {
                let first = word[0] & (SHUFFLE_SOURCE_BYTES - 1) as u8;
                (first as usize % width == 0
                    && word.iter().zip(first..).all(|(&index, expected)| {
                        index & (SHUFFLE_SOURCE_BYTES - 1) as u8 == expected
                    }))
                .then_some(first / width as u8)
            })
            .collect();
        match words {
            Some(words) => Self::Words(words.try_into().expect("four words")),
            None => Self::Bytes(indices),
        }
    }

    #[inline(always)]
    pub(crate) fn apply(self, left: u128, right: u128) -> u128 {
        match self {
            Self::Words(words) => {
                let (left, right) = (u32_lanes(left), u32_lanes(right));
                let sources = [left[0], left[1], left[2], left[3], right[0], right[1], right[2], right[3]];
                let pick = |lane: usize| u128::from(sources[usize::from(words[lane]) % SHUFFLE_SOURCE_WORDS]);
                pick(0) | (pick(1) << u32::BITS) | (pick(2) << (2 * u32::BITS)) | (pick(3) << (3 * u32::BITS))
            }
            Self::Bytes(indices) => shuffle(left, right, indices),
        }
    }
}

#[cfg(test)]
mod bit_kernel_tests {
    use super::{BitKernel, ShuffleForm, SimdKernel, SimdOperator};

    #[test]
    fn shuffle_forms_select_what_the_shuffle_selects() {
        let (left, right) = (0x0f0e_0d0c_0b0a_0908_0706_0504_0302_0100_u128, 0x1f1e_1d1c_1b1a_1918_1716_1514_1312_1110_u128);
        let words = |lanes: [u8; 4]| -> [u8; 16] {
            std::array::from_fn(|byte| lanes[byte / 4] * 4 + (byte % 4) as u8)
        };
        // u32 lane picks from both operands (`_mm_mul_epu32`'s, a rotation of u64 lanes),
        // indices past the operands' bytes, which validation excludes but the mask reduces,
        // and byte patterns no word form covers.
        let cases = [
            words([0, 2, 0, 2]),
            words([2, 3, 4, 5]),
            words([7, 6, 1, 0]),
            std::array::from_fn(|byte| byte as u8 + 32),
            std::array::from_fn(|byte| (byte as u8 * 7) % 32),
            [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
        ];
        for indices in cases {
            assert_eq!(ShuffleForm::of(indices).apply(left, right), super::shuffle(left, right, indices), "{indices:?}");
        }
        assert!(matches!(ShuffleForm::of(words([0, 2, 0, 2])), ShuffleForm::Words(_)));
        assert!(matches!(ShuffleForm::of(cases[5]), ShuffleForm::Bytes(_)));
    }

    /// The operator `name` applied to bits, through its kernel.
    fn operator(name: SimdOperator, left: u128, right: u128) -> u128 {
        let (kernel, lane) = SimdKernel::of(name.selector(0)).expect("operator kernel");
        kernel.apply_bits(left, right, lane)
    }

    #[test]
    fn fused_kernels_match_their_compositions() {
        use super::{BitShift, multiply_words};
        for &x in &EDGES {
            // A rotation by k, including by 0 and by the lane width, which wrap to the identity.
            for k in [0u32, 1, 16, 24, 32, 63, 64, 65, 127] {
                let rotated = BitKernel::Shift(BitShift::I64x2Rotl).apply(x, u128::from(k));
                let composed = operator(SimdOperator::V128Or, operator(SimdOperator::I64x2Shl, x, u128::from(k)), operator(SimdOperator::I64x2ShrU, x, u128::from(64u32.wrapping_sub(k))));
                assert_eq!(rotated, composed, "rotl {x:#x} by {k}");
            }
            for &y in &EDGES {
                for (left, right) in [([0u8, 2], [0u8, 2]), ([1, 3], [2, 0]), ([4, 6], [7, 5])] {
                    let shuffle = |value: u128, words: [u8; 2]| {
                        ShuffleForm::of(std::array::from_fn(|byte| words[(byte / 4) % 2] * 4 + (byte % 4) as u8)).apply(value, value)
                    };
                    let composed = operator(SimdOperator::I64x2ExtMulLowI32x4U, shuffle(x, left), shuffle(y, right));
                    assert_eq!(multiply_words(x, y, left, right), composed, "{x:#x} {y:#x} {left:?} {right:?}");
                }
            }
        }
    }

    /// Vectors whose lanes reach every carry, sign and width boundary.
    const EDGES: [u128; 8] = [
        0,
        u128::MAX,
        0x8000_0000_0000_0000_8000_0000_0000_0000,
        0x7fff_ffff_ffff_ffff_7fff_ffff_ffff_ffff,
        0xffff_ffff_0000_0001_8000_0000_7fff_ffff,
        0x0123_4567_89ab_cdef_fedc_ba98_7654_3210,
        0x0000_0001_ffff_ffff_0000_0000_ffff_fffe,
        0xdead_beef_0bad_f00d_8000_0001_0000_0003,
    ];
    /// Shift counts at and past each lane width, which the operators reduce modulo it.
    const COUNTS: [i32; 12] = [0, 1, 7, 31, 32, 33, 63, 64, 65, 127, -1, i32::MIN];

    #[test]
    fn bit_kernels_match_their_operators() {
        for (index, _) in SimdOperator::ALL.iter().enumerate() {
            let selector = (index as u32) << super::SELECTOR_LANE_BITS;
            let (kernel, lane) = SimdKernel::of(selector).expect("operator kernel");
            for count in [None, Some(0)] {
                let Some(bits) = BitKernel::of(selector, count) else {
                    continue;
                };
                for &left in &EDGES {
                    let rights: Vec<u128> = match bits {
                        BitKernel::Shift(_) => COUNTS.iter().map(|&count| u128::from(count as u32)).collect(),
                        _ => EDGES.to_vec(),
                    };
                    for right in rights {
                        assert_eq!(
                            bits.apply(left, right),
                            kernel.apply_bits(left, right, lane),
                            "{bits:?} {left:#x} {right:#x}"
                        );
                    }
                }
            }
        }
    }
}
