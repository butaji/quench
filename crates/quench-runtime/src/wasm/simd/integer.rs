//! Integer lane projections and SIMD-specific saturation/grouping rules.

use super::{WasmValue, lane, lane_mask, map_lanes, replace};
use crate::wasm::integer::{I64BinaryOperator as Binary, I64UnaryOperator as Unary};

const PAIR_LANES: u32 = 2;
const Q15_FRACTION_BITS: u32 = i16::BITS - 1;
const Q15_ROUNDING_BIAS: i128 = 1 << (Q15_FRACTION_BITS - 1);

#[derive(Clone, Copy)]
pub(super) enum Signedness {
    Signed,
    Unsigned,
}
#[derive(Clone, Copy)]
pub(super) enum Half {
    Low,
    High,
}

fn signed(bits: u64, width: u32) -> i64 {
    let shift = u64::BITS - width;
    ((bits << shift) as i64) >> shift
}
fn interpret(bits: u64, width: u32, sign: Signedness) -> i128 {
    match sign {
        Signedness::Signed => i128::from(signed(bits, width)),
        Signedness::Unsigned => i128::from(bits),
    }
}
fn saturate(value: i128, width: u32, sign: Signedness) -> u64 {
    let (minimum, maximum) = match sign {
        Signedness::Signed => {
            let bound = 1_i128 << (width - 1);
            (-bound, bound - 1)
        }
        Signedness::Unsigned => (0, lane_mask(width) as i128),
    };
    value.clamp(minimum, maximum) as u64
}
fn numeric(value: WasmValue, width: u32) -> u64 {
    match value {
        WasmValue::I64(value) => value as u64,
        WasmValue::I32(value) => {
            if value != 0 {
                lane_mask(width) as u64
            } else {
                0
            }
        }
        _ => unreachable!("integer scalar projection"),
    }
}
fn binary_lane(left: u64, right: u64, width: u32, op: Binary) -> u64 {
    let sign = matches!(
        op,
        Binary::LessSigned
            | Binary::GreaterSigned
            | Binary::LessEqualSigned
            | Binary::GreaterEqualSigned
            | Binary::ShiftRightSigned
    );
    let left = if sign {
        signed(left, width)
    } else {
        left as i64
    };
    let right = if sign && !matches!(op, Binary::ShiftRightSigned) {
        signed(right, width)
    } else {
        right as i64
    };
    numeric(
        op.apply(left, right)
            .expect("admitted nontrapping integer lane operation"),
        width,
    )
}
pub(super) fn binary(left: u128, right: u128, width: u32, op: Binary) -> WasmValue {
    map_lanes(left, Some(right), width, |a, b| {
        binary_lane(a, b.unwrap(), width, op)
    })
}
pub(super) fn shift(value: u128, count: u32, width: u32, op: Binary) -> WasmValue {
    let count = u64::from(count % width);
    map_lanes(value, None, width, |bits, _| {
        binary_lane(bits, count, width, op)
    })
}
pub(super) fn negate(value: u128, width: u32) -> WasmValue {
    map_lanes(value, None, width, |bits, _| {
        binary_lane(0, bits, width, Binary::Subtract)
    })
}
pub(super) fn absolute(value: u128, width: u32) -> WasmValue {
    map_lanes(value, None, width, |bits, _| {
        if signed(bits, width) < 0 {
            binary_lane(0, bits, width, Binary::Subtract)
        } else {
            bits
        }
    })
}
pub(super) fn population_count(value: u128) -> WasmValue {
    map_lanes(value, None, u8::BITS, |bits, _| {
        numeric(Unary::PopulationCount.apply(bits as i64), u8::BITS)
    })
}
pub(super) fn all_true(value: u128, width: u32) -> WasmValue {
    WasmValue::I32(i32::from(
        (0..u128::BITS / width).all(|index| lane(value, width, index as u8) != 0),
    ))
}
pub(super) fn bitmask(value: u128, width: u32) -> WasmValue {
    let bits = (0..u128::BITS / width).fold(0_u32, |bits, index| {
        bits | (((lane(value, width, index as u8) >> (width - 1)) as u32) << index)
    });
    WasmValue::I32(bits as i32)
}
pub(super) fn extrema(left: u128, right: u128, width: u32, compare: Binary) -> WasmValue {
    map_lanes(left, Some(right), width, |a, b| {
        let b = b.unwrap();
        if binary_lane(a, b, width, compare) != 0 {
            a
        } else {
            b
        }
    })
}
pub(super) fn saturating(
    left: u128,
    right: u128,
    width: u32,
    sign: Signedness,
    op: Binary,
) -> WasmValue {
    map_lanes(left, Some(right), width, |a, b| {
        let a = interpret(a, width, sign);
        let b = interpret(b.unwrap(), width, sign);
        let value = match op {
            Binary::Add => a + b,
            Binary::Subtract => a - b,
            _ => unreachable!("saturating add/subtract"),
        };
        saturate(value, width, sign)
    })
}
pub(super) fn average(left: u128, right: u128, width: u32) -> WasmValue {
    map_lanes(left, Some(right), width, |a, b| {
        (a + b.unwrap()).div_ceil(u64::from(PAIR_LANES))
    })
}
pub(super) fn q15(left: u128, right: u128) -> WasmValue {
    map_lanes(left, Some(right), i16::BITS, |a, b| {
        let product = interpret(a, i16::BITS, Signedness::Signed)
            * interpret(b.unwrap(), i16::BITS, Signedness::Signed);
        saturate(
            (product + Q15_ROUNDING_BIAS) >> Q15_FRACTION_BITS,
            i16::BITS,
            Signedness::Signed,
        )
    })
}
fn half_start(width: u32, half: Half) -> u32 {
    match half {
        Half::Low => 0,
        Half::High => u128::BITS / width,
    }
}
pub(super) fn extend(value: u128, width: u32, sign: Signedness, half: Half) -> WasmValue {
    let source_width = width / PAIR_LANES;
    let start = half_start(width, half);
    let mut result = 0;
    for index in 0..u128::BITS / width {
        let value = interpret(
            lane(value, source_width, (start + index) as u8),
            source_width,
            sign,
        ) as u64;
        result = replace(result, width, index as u8, value);
    }
    WasmValue::V128(result)
}
pub(super) fn extend_multiply(
    left: u128,
    right: u128,
    width: u32,
    sign: Signedness,
    half: Half,
) -> WasmValue {
    let source_width = width / PAIR_LANES;
    let start = half_start(width, half);
    let mut result = 0;
    for index in 0..u128::BITS / width {
        let a = interpret(
            lane(left, source_width, (start + index) as u8),
            source_width,
            sign,
        ) as u64;
        let b = interpret(
            lane(right, source_width, (start + index) as u8),
            source_width,
            sign,
        ) as u64;
        result = replace(
            result,
            width,
            index as u8,
            binary_lane(a, b, width, Binary::Multiply),
        );
    }
    WasmValue::V128(result)
}
pub(super) fn pairwise_add(value: u128, width: u32, sign: Signedness) -> WasmValue {
    let source_width = width / PAIR_LANES;
    let mut result = 0;
    for index in 0..u128::BITS / width {
        let source = index * PAIR_LANES;
        let a = interpret(lane(value, source_width, source as u8), source_width, sign) as u64;
        let b = interpret(
            lane(value, source_width, (source + 1) as u8),
            source_width,
            sign,
        ) as u64;
        result = replace(
            result,
            width,
            index as u8,
            binary_lane(a, b, width, Binary::Add),
        );
    }
    WasmValue::V128(result)
}
pub(super) fn dot(left: u128, right: u128) -> WasmValue {
    let mut result = 0;
    for index in 0..u128::BITS / u32::BITS {
        let mut sum = 0;
        for part in 0..PAIR_LANES {
            let source = (index * PAIR_LANES + part) as u8;
            let a = signed(lane(left, u16::BITS, source), u16::BITS) as u64;
            let b = signed(lane(right, u16::BITS, source), u16::BITS) as u64;
            let product = binary_lane(a, b, u64::BITS, Binary::Multiply);
            sum = binary_lane(sum, product, u64::BITS, Binary::Add);
        }
        result = replace(result, u32::BITS, index as u8, sum);
    }
    WasmValue::V128(result)
}
pub(super) fn narrow(left: u128, right: u128, width: u32, sign: Signedness) -> WasmValue {
    let source_width = width * PAIR_LANES;
    let source_count = u128::BITS / source_width;
    let mut result = 0;
    for index in 0..u128::BITS / width {
        let source = if index < source_count { left } else { right };
        // Both narrowing forms interpret source lanes as signed; only the target differs.
        let value = interpret(
            lane(source, source_width, (index % source_count) as u8),
            source_width,
            Signedness::Signed,
        );
        result = replace(result, width, index as u8, saturate(value, width, sign));
    }
    WasmValue::V128(result)
}

/// Deterministic relaxed dot: signed byte products with signed pair saturation.
pub(super) fn relaxed_dot(left: u128, right: u128) -> WasmValue {
    let mut result = 0;
    for index in 0..u128::BITS / u16::BITS {
        let mut sum = 0_i128;
        for part in 0..PAIR_LANES {
            let source = (index * PAIR_LANES + part) as u8;
            sum += interpret(lane(left, u8::BITS, source), u8::BITS, Signedness::Signed)
                * interpret(lane(right, u8::BITS, source), u8::BITS, Signedness::Signed);
        }
        result = replace(
            result,
            u16::BITS,
            index as u8,
            saturate(sum, u16::BITS, Signedness::Signed),
        );
    }
    WasmValue::V128(result)
}
