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
/// The lanes of one width, whose shape the compiler sees: each lanewise operator below is
/// its scalar operator on every lane, exactly `binary_lane`'s result for the operators it
/// admits, as one loop the width unrolls.
#[derive(Clone, Copy)]
struct Lanes<const WIDTH: u32>;

impl<const WIDTH: u32> Lanes<WIDTH> {
    const MASK: u64 = u64::MAX >> (u64::BITS - WIDTH);

    #[inline(always)]
    fn map(self, left: u128, right: u128, operation: impl Fn(u64, u64) -> u64) -> u128 {
        let mut result = 0;
        let mut index = 0;
        while index < u128::BITS / WIDTH {
            let shift = WIDTH * index;
            let a = (left >> shift) as u64 & Self::MASK;
            let b = (right >> shift) as u64 & Self::MASK;
            result |= u128::from(operation(a, b) & Self::MASK) << shift;
            index += 1;
        }
        result
    }

    #[inline(always)]
    fn binary(self, left: u128, right: u128, op: Binary) -> Option<u128> {
        Some(match op {
            Binary::Add => self.map(left, right, u64::wrapping_add),
            Binary::Subtract => self.map(left, right, u64::wrapping_sub),
            Binary::Multiply => self.map(left, right, u64::wrapping_mul),
            Binary::And => left & right,
            Binary::Or => left | right,
            Binary::Xor => left ^ right,
            _ => return None,
        })
    }

    /// A shift of every lane by `count`, below the lane width.
    #[inline(always)]
    fn shift(self, value: u128, count: u32, op: Binary) -> Option<u128> {
        Some(match op {
            Binary::ShiftLeft => self.map(value, 0, |a, _| a << count),
            Binary::ShiftRightUnsigned => self.map(value, 0, |a, _| a >> count),
            Binary::ShiftRightSigned => {
                self.map(value, 0, |a, _| (signed(a, WIDTH) >> count) as u64)
            }
            _ => return None,
        })
    }
}

/// `$operation` on `Lanes` of `$width`, one of the integer lane widths, named `$lanes`:
/// one arm per width, so each inlines its own loop.
macro_rules! by_width {
    ($width:expr, |$lanes:ident| $operation:expr) => {
        match $width {
            8 => {
                let $lanes = Lanes::<8>;
                $operation
            }
            16 => {
                let $lanes = Lanes::<16>;
                $operation
            }
            32 => {
                let $lanes = Lanes::<32>;
                $operation
            }
            64 => {
                let $lanes = Lanes::<64>;
                $operation
            }
            _ => None,
        }
    };
}

pub(super) fn binary(left: u128, right: u128, width: u32, op: Binary) -> WasmValue {
    if let Some(bits) = by_width!(width, |lanes| lanes.binary(left, right, op)) {
        return WasmValue::V128(bits);
    }
    map_lanes(left, Some(right), width, |a, b| {
        binary_lane(a, b.unwrap(), width, op)
    })
}
pub(super) fn shift(value: u128, count: u32, width: u32, op: Binary) -> WasmValue {
    if let Some(bits) = by_width!(width, |lanes| lanes.shift(value, count % width, op)) {
        return WasmValue::V128(bits);
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The lanewise fast paths (`Lanes`) agree with the per-lane scalar rule (`binary_lane`)
    /// for every width and operator they admit.
    #[test]
    fn lanewise_operators_match_the_scalar_lane_rule() {
        let mut state = 0x9e37_79b9_7f4a_7c15_u64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let binaries = [
            Binary::Add,
            Binary::Subtract,
            Binary::Multiply,
            Binary::And,
            Binary::Or,
            Binary::Xor,
        ];
        let shifts = [
            Binary::ShiftLeft,
            Binary::ShiftRightUnsigned,
            Binary::ShiftRightSigned,
        ];
        for _ in 0..200 {
            let left = u128::from(next()) << 64 | u128::from(next());
            let right = u128::from(next()) << 64 | u128::from(next());
            let count = (next() % 200) as u32;
            for width in [8, 16, 32, 64] {
                for op in binaries {
                    let expected = map_lanes(left, Some(right), width, |a, b| {
                        binary_lane(a, b.unwrap(), width, op)
                    });
                    assert_eq!(
                        binary(left, right, width, op),
                        expected,
                        "{op:?} width {width}"
                    );
                }
                for op in shifts {
                    let lane_count = u64::from(count % width);
                    let expected = map_lanes(left, None, width, |bits, _| {
                        binary_lane(bits, lane_count, width, op)
                    });
                    assert_eq!(
                        shift(left, count, width, op),
                        expected,
                        "{op:?} width {width}"
                    );
                }
            }
        }
    }
}
