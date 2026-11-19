//! The SIMD stage on bits: vectors are `u128`, the native rows are per-lane loops, the kernels
//! and the shuffle are the interpreter's own. The interpreter runs every vector operator through
//! it (`execute`); the tests run every row against its kernel, which is the oracle the Wasm
//! tier's vector instructions must equal.

use super::wasm_simd::{Lanes, Native, NativeOp, WasmSimd, simd};
use crate::WasmValue;
use crate::wasm::V128_BYTES;
use crate::wasm::simd::{SimdKernel, bits_of, lane, replace, shuffle, value_of};

pub(crate) struct ConcreteSimd;

/// The operator `selector` names on its decoded operands, by the staged decision (`simd`) on the
/// bit stage: the interpreter's execution of a vector operator. The result's type is the
/// operator's, read from its table entry.
#[inline(always)]
pub(crate) fn execute(selector: u32, left: WasmValue, right: Option<WasmValue>) -> WasmValue {
    let bits = simd(&mut ConcreteSimd, selector, bits_of(left), right.map(bits_of));
    value_of(SimdKernel::result_of(selector), bits)
}

/// `operation` on each lane of `bits`, cut at `width`.
fn each_lane(width: u32, bits: u128, operation: impl Fn(u8, u64) -> u64) -> u128 {
    (0..u128::BITS / width).fold(0, |result, index| {
        let index = index as u8;
        replace(
            result,
            width,
            index,
            operation(index, lane(bits, width, index)),
        )
    })
}

/// `operation` on the lane `value` of the signed lane type of `width`, as its sign-extended
/// 64-bit integer.
fn signed(width: u32, value: u64, operation: impl Fn(i64) -> i64) -> u64 {
    let unused = u64::BITS - width;
    operation(((value << unused) as i64) >> unused) as u64
}

/// A lane-wise `native` on its lanes; its shift count is the low bits of `right`.
fn apply_per_lane(native: Native, left: u128, right: u128) -> u128 {
    let width = native.lanes.width();
    let count = (right as u32) % width;
    let other = |index: u8| lane(right, width, index);
    each_lane(width, left, |index, value| match native.op {
        NativeOp::Add => value.wrapping_add(other(index)),
        NativeOp::Subtract => value.wrapping_sub(other(index)),
        NativeOp::ShiftLeft => value << count,
        NativeOp::ShiftRightUnsigned => value >> count,
        NativeOp::ShiftRightSigned => signed(width, value, |extended| extended >> count),
        _ => unreachable!("a whole-vector or widening row"),
    })
}

impl WasmSimd for ConcreteSimd {
    type Value = u128;

    fn vector_native(&mut self, native: Native, left: u128, right: Option<u128>) -> u128 {
        let right = right.unwrap_or(0);
        match (native.op, native.lanes) {
            (NativeOp::Not, _) => !left,
            (NativeOp::And, _) => left & right,
            (NativeOp::Or, _) => left | right,
            (NativeOp::Xor, _) => left ^ right,
            (NativeOp::AndNot, _) => left & !right,
            (NativeOp::ExtendMultiplyLowUnsigned, Lanes::I32x4) => {
                let (narrow, wide) = (native.lanes.width(), u64::BITS);
                (0..u128::BITS / wide).fold(0, |result, index| {
                    let index = index as u8;
                    let product = lane(left, narrow, index) * lane(right, narrow, index);
                    replace(result, wide, index, product)
                })
            }
            _ => apply_per_lane(native, left, right),
        }
    }

    fn vector_kernel(&mut self, selector: u32, left: u128, right: Option<u128>) -> u128 {
        let (kernel, lane) = SimdKernel::of(selector).expect("an admitted selector");
        kernel.apply_bits(left, right.unwrap_or(0), lane)
    }

    fn vector_shuffle(&mut self, lanes: [u8; V128_BYTES], left: u128, right: u128) -> u128 {
        shuffle(left, right, lanes)
    }
}
