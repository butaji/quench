//! The bits of a typed value: the representation a stage holds a vector or scalar in (a vector whole, a
//! scalar in the low bits), and the value of a type that those bits read back as.

use super::{WasmType, WasmValue, scalar};

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
