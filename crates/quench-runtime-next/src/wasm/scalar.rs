//! Scalar signatures own interpretation; execution slots preserve raw bits.

use crate::{Value, bytecode::Constant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WasmType {
    I32,
    I64,
    F32,
    F64,
    V128,
    FuncRef,
    ExternRef,
    I31Ref,
    ExnRef,
}

impl WasmType {
    pub fn from_wasm(ty: wasmparser::ValType) -> Option<Self> {
        match ty {
            wasmparser::ValType::I32 => Some(Self::I32),
            wasmparser::ValType::I64 => Some(Self::I64),
            wasmparser::ValType::F32 => Some(Self::F32),
            wasmparser::ValType::F64 => Some(Self::F64),
            wasmparser::ValType::V128 => Some(Self::V128),
            wasmparser::ValType::Ref(ty) => Self::from_ref_type(ty),
        }
    }

    pub fn from_ref_type(ty: wasmparser::RefType) -> Option<Self> {
        match ty.heap_type() {
            wasmparser::HeapType::Abstract {
                ty: wasmparser::AbstractHeapType::Func | wasmparser::AbstractHeapType::NoFunc,
                ..
            }
            | wasmparser::HeapType::Concrete(_) => Some(Self::FuncRef),
            wasmparser::HeapType::Abstract {
                ty: wasmparser::AbstractHeapType::Cont | wasmparser::AbstractHeapType::NoCont,
                ..
            } => None,
            wasmparser::HeapType::Exact(_) => Some(Self::FuncRef),
            wasmparser::HeapType::Abstract {
                ty: wasmparser::AbstractHeapType::I31,
                ..
            } => Some(Self::I31Ref),
            wasmparser::HeapType::Abstract {
                ty: wasmparser::AbstractHeapType::Exn | wasmparser::AbstractHeapType::NoExn,
                ..
            } => Some(Self::ExnRef),
            wasmparser::HeapType::Abstract { .. } => Some(Self::ExternRef),
        }
    }

    pub fn zero(self) -> WasmValue {
        match self {
            Self::I32 => WasmValue::I32(0),
            Self::I64 => WasmValue::I64(0),
            Self::F32 => WasmValue::F32(0),
            Self::F64 => WasmValue::F64(0),
            Self::V128 => WasmValue::V128(0),
            Self::FuncRef => WasmValue::FuncRef(None),
            Self::ExternRef => WasmValue::ExternRef(None),
            Self::I31Ref => WasmValue::I31Ref(None),
            Self::ExnRef => WasmValue::ExnRef(None),
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
    V128(u128),
    FuncRef(Option<u32>),
    ExternRef(Option<u32>),
    I31Ref(Option<u32>),
    ExnRef(Option<Value>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WasmGcField {
    pub ty: WasmType,
    pub mutable: bool,
    pub packed_bits: Option<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WasmGcType {
    Array(WasmGcField),
    Struct(Vec<WasmGcField>),
}

impl WasmValue {
    pub fn ty(self) -> WasmType {
        match self {
            Self::I32(_) => WasmType::I32,
            Self::I64(_) => WasmType::I64,
            Self::F32(_) => WasmType::F32,
            Self::F64(_) => WasmType::F64,
            Self::V128(_) => WasmType::V128,
            Self::FuncRef(_) => WasmType::FuncRef,
            Self::ExternRef(_) => WasmType::ExternRef,
            Self::I31Ref(_) => WasmType::I31Ref,
            Self::ExnRef(_) => WasmType::ExnRef,
        }
    }

    pub fn is_canonical_nan(self) -> bool {
        self.nan_bits()
            .is_some_and(|(bits, canonical, sign)| bits & !sign == canonical)
    }

    pub fn is_arithmetic_nan(self) -> bool {
        self.nan_bits()
            .is_some_and(|(bits, canonical, _)| bits & canonical == canonical)
    }

    fn nan_bits(self) -> Option<(u64, u64, u64)> {
        use super::numeric::{canonical_nan_bits, sign_mask};
        match self {
            Self::F32(bits) => Some((
                u64::from(bits),
                canonical_nan_bits(f32::MANTISSA_DIGITS, u64::from(f32::INFINITY.to_bits())),
                sign_mask(u32::BITS),
            )),
            Self::F64(bits) => Some((
                bits,
                canonical_nan_bits(f64::MANTISSA_DIGITS, f64::INFINITY.to_bits()),
                sign_mask(u64::BITS),
            )),
            _ => None,
        }
    }

    pub(crate) fn bits(self) -> ScalarBits {
        match self {
            Self::I32(value) => ScalarBits::Bits32(value as u32),
            Self::F32(bits) => ScalarBits::Bits32(bits),
            Self::I64(value) => ScalarBits::Bits64(value as u64),
            Self::F64(bits) => ScalarBits::Bits64(bits),
            Self::V128(bits) => ScalarBits::Bits128(bits),
            Self::FuncRef(reference) | Self::ExternRef(reference) => {
                ScalarBits::Reference(reference)
            }
            Self::I31Ref(reference) => ScalarBits::I31Reference(reference),
            Self::ExnRef(reference) => ScalarBits::ExnReference(reference),
        }
    }

    pub(super) fn constant(self) -> Constant {
        match self.bits() {
            ScalarBits::Bits32(bits) => Constant::Number(f64::from(bits as i32)),
            ScalarBits::Bits64(bits) => Constant::WasmBits64(bits),
            ScalarBits::Bits128(bits) => Constant::BigInt(bits.to_string()),
            ScalarBits::Reference(None) => Constant::Null,
            ScalarBits::Reference(Some(index)) => Constant::Number(index as i32 as f64),
            ScalarBits::I31Reference(None) => Constant::Null,
            ScalarBits::I31Reference(Some(bits)) => Constant::Number(bits as f64),
            ScalarBits::ExnReference(None) => Constant::Null,
            ScalarBits::ExnReference(Some(_)) => {
                unreachable!("non-null Wasm exception references are not constants")
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WasmSignature {
    pub params: Vec<WasmType>,
    pub result: Option<WasmType>,
    pub additional_results: Vec<WasmType>,
}

impl WasmSignature {
    pub fn result_types(&self) -> impl Iterator<Item = WasmType> + '_ {
        self.result
            .into_iter()
            .chain(self.additional_results.iter().copied())
    }

    pub fn result_count(&self) -> usize {
        usize::from(self.result.is_some()) + self.additional_results.len()
    }
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
    Bits128(u128),
    Reference(Option<u32>),
    I31Reference(Option<u32>),
    ExnReference(Option<Value>),
}

impl WasmType {
    pub(crate) fn decode(self, bits: ScalarBits) -> Option<WasmValue> {
        match (self, bits) {
            (Self::I32, ScalarBits::Bits32(bits)) => Some(WasmValue::I32(bits as i32)),
            (Self::F32, ScalarBits::Bits32(bits)) => Some(WasmValue::F32(bits)),
            (Self::I64, ScalarBits::Bits64(bits)) => Some(WasmValue::I64(bits as i64)),
            (Self::F64, ScalarBits::Bits64(bits)) => Some(WasmValue::F64(bits)),
            (Self::V128, ScalarBits::Bits128(bits)) => Some(WasmValue::V128(bits)),
            (Self::FuncRef, ScalarBits::Reference(reference)) => {
                Some(WasmValue::FuncRef(reference))
            }
            (Self::ExternRef, ScalarBits::Reference(reference)) => {
                Some(WasmValue::ExternRef(reference))
            }
            (Self::I31Ref, ScalarBits::I31Reference(reference)) => {
                Some(WasmValue::I31Ref(reference))
            }
            (Self::ExnRef, ScalarBits::ExnReference(reference)) => {
                Some(WasmValue::ExnRef(reference))
            }
            _ => None,
        }
    }
}
