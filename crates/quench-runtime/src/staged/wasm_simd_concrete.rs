//! The SIMD stage on bits: vectors are `u128`, the native rows and the operators' kernels are the
//! shared bodies of `wasm_simd_rules`, and the shuffle is the interpreter's own. The interpreter runs
//! every vector operator through it (`execute`); the tests run every row against its kernel, which is
//! the oracle the Wasm tier's vector instructions must equal.

use super::wasm_simd_rules::{
    Native, WasmSimd, kernel_bits, native_bits, shuffle_bits, simd, simd_scalar,
};
use crate::WasmType;
use crate::WasmValue;
use crate::wasm::V128_BYTES;
use crate::wasm::simd::{SimdKernel, bits_of, value_of};

pub(crate) struct ConcreteSimd;

/// The operator `selector` names, its `kernel` already resolved from that selector, applied to its
/// decoded operands by the staged decision (`simd`) on the bit stage: the interpreter's execution of
/// a vector operator. The result's type is the kernel's.
#[inline(always)]
pub(crate) fn execute(kernel: &SimdKernel, selector: u32, left: WasmValue, right: Option<WasmValue>) -> WasmValue {
    let (left, right) = (bits_of(left), right.map(bits_of));
    let result = kernel.result;
    let bits = match result {
        WasmType::V128 => simd(&mut ConcreteSimd, selector, left, right),
        _ => u128::from(simd_scalar(&mut ConcreteSimd, selector, left, right)),
    };
    value_of(result, bits)
}

impl WasmSimd for ConcreteSimd {
    type Value = u128;
    type Scalar = u64;

    fn constant_vector(&mut self, bits: u128) -> u128 {
        bits
    }

    fn vector_native(&mut self, native: Native, left: u128, right: Option<u128>) -> u128 {
        native_bits(native, left, right.unwrap_or(0))
    }

    /// The kernel's bits, the scalar in their low word.
    fn scalar_kernel(&mut self, selector: u32, left: u128, right: Option<u128>) -> u64 {
        self.vector_kernel(selector, left, right) as u64
    }

    fn vector_kernel(&mut self, selector: u32, left: u128, right: Option<u128>) -> u128 {
        kernel_bits(selector, left, right)
    }

    fn vector_shuffle(&mut self, lanes: [u8; V128_BYTES], left: u128, right: u128) -> u128 {
        shuffle_bits(left, right, lanes)
    }
}
