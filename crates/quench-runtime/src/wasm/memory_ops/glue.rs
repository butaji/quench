#![allow(unused_imports)]
use super::*;
use crate::wasm::simd::{SimdOperator, SimdOperatorSimdExt};
use crate::wasm::{WasmTrap, WasmValue};

/// The `MemoryLoad` methods that name interpreter state: the SIMD lane rules (`wasm/simd.rs`) and
/// the linear-memory read (`staged/wasm_memory_bytes.rs`).
pub trait MemoryLoadMemoryOpsExt: Sized {
    /// The value of a vector load of the zero-extended lane `word` (its `width` bytes, the staged
    /// lane load): the widening, splatting or zeroing the load's own semantics.
    fn vector_of_word(self, word: u64) -> WasmValue;
    /// The value at `address` of `bytes`, trapping when the address is not aligned to the load's
    /// width.
    fn read_atomic(self, bytes: &[u8], address: u64) -> Result<WasmValue, WasmTrap>;
}

impl MemoryLoadMemoryOpsExt for MemoryLoad {
    fn vector_of_word(self, word: u64) -> WasmValue {
        match self {
            Self::V128Load => WasmValue::V128(u128::from(word)),
            Self::V128Load8x8S => SimdOperator::I16x8ExtendLowI8x16S.apply(WasmValue::V128(u128::from(word)), None, 0),
            Self::V128Load8x8U => SimdOperator::I16x8ExtendLowI8x16U.apply(WasmValue::V128(u128::from(word)), None, 0),
            Self::V128Load16x4S => SimdOperator::I32x4ExtendLowI16x8S.apply(WasmValue::V128(u128::from(word)), None, 0),
            Self::V128Load16x4U => SimdOperator::I32x4ExtendLowI16x8U.apply(WasmValue::V128(u128::from(word)), None, 0),
            Self::V128Load32x2S => SimdOperator::I64x2ExtendLowI32x4S.apply(WasmValue::V128(u128::from(word)), None, 0),
            Self::V128Load32x2U => SimdOperator::I64x2ExtendLowI32x4U.apply(WasmValue::V128(u128::from(word)), None, 0),
            Self::V128Load8Splat => {
                let word = word as u8;
                SimdOperator::I8x16Splat.apply(WasmValue::I32(word as i32), None, 0)
            }
            Self::V128Load16Splat => {
                let word = word as u16;
                SimdOperator::I16x8Splat.apply(WasmValue::I32(word as i32), None, 0)
            }
            Self::V128Load32Splat => {
                let word = word as u32;
                SimdOperator::I32x4Splat.apply(WasmValue::I32(word as i32), None, 0)
            }
            Self::V128Load64Splat => SimdOperator::I64x2Splat.apply(WasmValue::I64(word as i64), None, 0),
            Self::V128Load32Zero => WasmValue::V128(u128::from(word as u32)),
            Self::V128Load64Zero => WasmValue::V128(u128::from(word)),
            Self::I32Load
            | Self::I64Load
            | Self::F32Load
            | Self::F64Load
            | Self::I32Load8S
            | Self::I32Load8U
            | Self::I32Load16S
            | Self::I32Load16U
            | Self::I64Load8S
            | Self::I64Load8U
            | Self::I64Load16S
            | Self::I64Load16U
            | Self::I64Load32S
            | Self::I64Load32U => unreachable!("a scalar load has an access"),
        }
    }

    fn read_atomic(self, bytes: &[u8], address: u64) -> Result<WasmValue, WasmTrap> {
        if !address.is_multiple_of(self.width() as u64) {
            return Err(WasmTrap::UnalignedAtomic);
        }
        self.read(bytes, address)
    }
}
