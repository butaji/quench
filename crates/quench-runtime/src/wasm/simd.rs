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

macro_rules! simd_operators {
    ($($name:ident [$($lane:ident: $width:expr)?]: $left:ident, $right:expr => |$a:ident, $b:ident, $l:ident| $body:expr;)* ) => {
        #[derive(Clone, Copy, Debug)]
        #[repr(u32)]
        pub(crate) enum SimdOperator { $($name,)* }
        impl SimdOperator {
            const ALL: &'static [Self] = &[$(Self::$name,)*];
            pub(crate) fn from_selector(selector: u32) -> Option<(Self, u8)> {
                let op = *Self::ALL.get((selector >> SELECTOR_LANE_BITS) as usize)?;
                let lane = (selector & SELECTOR_LANE_MASK) as u8;
                let count = op.lane_width().map_or(1, |width| u128::BITS / width);
                (u32::from(lane) < count).then_some((op, lane))
            }
            fn lane_width(self) -> Option<u32> {
                match self { $(Self::$name => [$($width)?].first().copied(),)* }
            }
            pub(super) fn selector(self, lane: u8) -> u32 { ((self as u32) << SELECTOR_LANE_BITS) | u32::from(lane) }
            pub(super) fn from_wasm(op: &wasmparser::Operator<'_>) -> Option<(Self, u8)> {
                match op {
                    $(wasmparser::Operator::$name $({$lane})? => Some((Self::$name, [$(*$lane)?].first().copied().unwrap_or(0))),)*
                    _ => None,
                }
            }
            pub(crate) fn left_type(self) -> WasmType { match self { $(Self::$name => WasmType::$left,)* } }
            pub(crate) fn right_type(self) -> Option<WasmType> { match self { $(Self::$name => $right,)* } }
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
    let a = left.to_le_bytes();
    let b = right.to_le_bytes();
    u128::from_le_bytes(indices.map(|index| {
        if (index as usize) < V128_BYTES {
            a[index as usize]
        } else {
            b[index as usize - V128_BYTES]
        }
    }))
}
pub(crate) fn shuffle_indices_valid(indices: &[u8; V128_BYTES]) -> bool {
    indices
        .iter()
        .all(|&index| (index as usize) < V128_BYTES * 2)
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
