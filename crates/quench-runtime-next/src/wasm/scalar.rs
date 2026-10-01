//! Scalar signatures own interpretation; execution slots preserve raw bits.

use crate::bytecode::Constant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WasmType {
    I32,
    I64,
    F32,
    F64,
}

impl WasmType {
    pub fn from_wasm(ty: wasmparser::ValType) -> Option<Self> {
        match ty {
            wasmparser::ValType::I32 => Some(Self::I32),
            wasmparser::ValType::I64 => Some(Self::I64),
            wasmparser::ValType::F32 => Some(Self::F32),
            wasmparser::ValType::F64 => Some(Self::F64),
            _ => None,
        }
    }

    pub(super) fn zero(self) -> WasmValue {
        match self {
            Self::I32 => WasmValue::I32(0),
            Self::I64 => WasmValue::I64(0),
            Self::F32 => WasmValue::F32(0),
            Self::F64 => WasmValue::F64(0),
        }
    }
}

/// Floating-point variants contain IEEE bits, preserving NaN payloads and -0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WasmValue {
    I32(i32),
    I64(i64),
    F32(u32),
    F64(u64),
}

impl WasmValue {
    pub fn ty(self) -> WasmType {
        match self {
            Self::I32(_) => WasmType::I32,
            Self::I64(_) => WasmType::I64,
            Self::F32(_) => WasmType::F32,
            Self::F64(_) => WasmType::F64,
        }
    }

    pub(crate) fn bits(self) -> ScalarBits {
        match self {
            Self::I32(value) => ScalarBits::Bits32(value as u32),
            Self::F32(bits) => ScalarBits::Bits32(bits),
            Self::I64(value) => ScalarBits::Bits64(value as u64),
            Self::F64(bits) => ScalarBits::Bits64(bits),
        }
    }

    pub(super) fn constant(self) -> Constant {
        match self.bits() {
            ScalarBits::Bits32(bits) => Constant::Number(f64::from(bits as i32)),
            ScalarBits::Bits64(bits) => Constant::WasmBits64(bits),
        }
    }
}

#[derive(Clone, Debug)]
pub struct WasmSignature {
    pub params: Vec<WasmType>,
    pub result: Option<WasmType>,
}

/// Frontend-validated facts and the decoded stream, consumed during lowering.
pub struct WasmFunctionBody<I> {
    pub signature: WasmSignature,
    pub locals: Vec<WasmType>,
    pub operators: I,
}

#[derive(Clone, Copy)]
pub(crate) enum ScalarBits {
    Bits32(u32),
    Bits64(u64),
}

impl WasmType {
    pub(crate) fn decode(self, bits: ScalarBits) -> Option<WasmValue> {
        match (self, bits) {
            (Self::I32, ScalarBits::Bits32(bits)) => Some(WasmValue::I32(bits as i32)),
            (Self::F32, ScalarBits::Bits32(bits)) => Some(WasmValue::F32(bits)),
            (Self::I64, ScalarBits::Bits64(bits)) => Some(WasmValue::I64(bits as i64)),
            (Self::F64, ScalarBits::Bits64(bits)) => Some(WasmValue::F64(bits)),
            _ => None,
        }
    }
}
