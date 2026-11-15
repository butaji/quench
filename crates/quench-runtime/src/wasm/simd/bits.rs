//! A SIMD operator on bits and the bits of a typed value: the representation a stage holds a
//! vector or scalar in (a vector whole, a scalar in the low bits), and the type table an
//! execution reads a result's type from.

use super::{SELECTOR_LANE_BITS, SimdKernel, SimdOperator, WasmType, WasmValue, scalar};

impl SimdKernel {
    /// The type of the result of the operator `selector` names, from a table of each
    /// operator's `result_type` built once: an execution reads it instead of running the kernel.
    pub(crate) fn result_of(selector: u32) -> WasmType {
        static RESULTS: std::sync::LazyLock<Vec<WasmType>> = std::sync::LazyLock::new(|| {
            SimdOperator::KERNELS
                .iter()
                .map(|kernel| kernel.result_type(0))
                .collect()
        });
        RESULTS[(selector >> SELECTOR_LANE_BITS) as usize]
    }

    /// The operator applied to operands given as bits, its result as bits: a vector whole,
    /// a scalar in the low bits (`ScalarBits`), as compiled code holds them.
    #[inline(always)]
    pub(crate) fn apply_bits(&self, left: u128, right: u128, lane: u8) -> u128 {
        (self.bits)(left, right, lane)
    }

    pub(super) fn apply_bits_typed(&self, left: u128, right: u128, lane: u8) -> WasmValue {
        let right = self.right.map(|ty| value_of(ty, right));
        (self.apply)(value_of(self.left, left), right, lane)
    }
}

/// The value of type `ty` that `bits` hold: a vector whole, a scalar in the low bits.
pub(crate) fn value_of(ty: WasmType, bits: u128) -> WasmValue {
    match ty {
        WasmType::V128 => WasmValue::V128(bits),
        ty => WasmValue::from_bits(ty, bits as u64),
    }
}

/// The bits of `value`: a vector whole, a scalar in the low bits (the inverse of `value_of`).
pub(crate) fn bits_of(value: WasmValue) -> u128 {
    match value {
        WasmValue::V128(bits) => bits,
        value => u128::from(scalar(value)),
    }
}
