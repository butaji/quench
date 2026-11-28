//! The Wasm scalar data types and their pure rules. Nothing here names interpreter state: the
//! methods that do (`WasmValue::ty`, `WasmValue::constant`) are the extension traits in `glue.rs`.

use crate::staged::wasm::V128_BYTES;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WasmType {
    I32,
    I64,
    F32,
    F64,
    V128,
    Reference {
        kind: WasmReferenceKind,
        nullable: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WasmReferenceKind {
    Function,
    External,
    DeclaredFunction { index: u32, exact: bool },
    DeclaredGc { index: u32, exact: bool },
    Internal(wasmparser::AbstractHeapType),
    BottomFunction,
    BottomExternal,
}

impl WasmType {
    pub const FUNCREF: Self = Self::Reference {
        kind: WasmReferenceKind::Function,
        nullable: true,
    };
    pub const EXTERNREF: Self = Self::Reference {
        kind: WasmReferenceKind::External,
        nullable: true,
    };
    pub const FUNC: Self = Self::Reference {
        kind: WasmReferenceKind::Function,
        nullable: false,
    };
    pub const EXTERN: Self = Self::Reference {
        kind: WasmReferenceKind::External,
        nullable: false,
    };

    /// Whether this is `exnref` or `nullexnref`, which no JavaScript value represents.
    pub fn is_exception_reference(self) -> bool {
        matches!(
            self,
            Self::Reference {
                kind: WasmReferenceKind::Internal(
                    wasmparser::AbstractHeapType::Exn | wasmparser::AbstractHeapType::NoExn
                ),
                ..
            }
        )
    }

    pub fn requires_declarations(self) -> bool {
        matches!(
            self,
            Self::Reference {
                kind: WasmReferenceKind::DeclaredFunction { .. }
                    | WasmReferenceKind::DeclaredGc { .. },
                ..
            }
        )
    }

    /// Exact embedding type projection; declared identities stay in WasmTypes.
    pub fn value_type(self) -> wasmparser::ValType {
        match self {
            Self::I32 => wasmparser::ValType::I32,
            Self::I64 => wasmparser::ValType::I64,
            Self::F32 => wasmparser::ValType::F32,
            Self::F64 => wasmparser::ValType::F64,
            Self::V128 => wasmparser::ValType::V128,
            Self::Reference { .. } => wasmparser::ValType::Ref(self.reference_type().unwrap()),
        }
    }

    pub fn reference_type(self) -> Option<wasmparser::RefType> {
        let Self::Reference { kind, nullable } = self else {
            return None;
        };
        let ty = match kind {
            WasmReferenceKind::Function => wasmparser::RefType::FUNC,
            WasmReferenceKind::External => wasmparser::RefType::EXTERN,
            WasmReferenceKind::BottomFunction => wasmparser::RefType::NOFUNC,
            WasmReferenceKind::BottomExternal => wasmparser::RefType::NOEXTERN,
            WasmReferenceKind::Internal(ty) => wasmparser::RefType::new(
                false,
                wasmparser::HeapType::Abstract { shared: false, ty },
            )
            .unwrap(),
            WasmReferenceKind::DeclaredFunction { index, exact }
            | WasmReferenceKind::DeclaredGc { index, exact } => {
                let index = wasmparser::UnpackedIndex::Module(index);
                let heap = if exact {
                    wasmparser::HeapType::Exact(index)
                } else {
                    wasmparser::HeapType::Concrete(index)
                };
                wasmparser::RefType::new(false, heap).unwrap()
            }
        };
        Some(if nullable { ty.nullable() } else { ty })
    }

    pub fn from_wasm(ty: wasmparser::ValType) -> Option<Self> {
        match ty {
            wasmparser::ValType::I32 => Some(Self::I32),
            wasmparser::ValType::I64 => Some(Self::I64),
            wasmparser::ValType::F32 => Some(Self::F32),
            wasmparser::ValType::F64 => Some(Self::F64),
            wasmparser::ValType::V128 => Some(Self::V128),
            wasmparser::ValType::Ref(ty) => {
                let kind = match ty.heap_type() {
                    wasmparser::HeapType::FUNC => WasmReferenceKind::Function,
                    wasmparser::HeapType::EXTERN => WasmReferenceKind::External,
                    wasmparser::HeapType::Abstract {
                        shared: false,
                        ty: wasmparser::AbstractHeapType::NoFunc,
                    } => WasmReferenceKind::BottomFunction,
                    wasmparser::HeapType::Abstract {
                        shared: false,
                        ty: wasmparser::AbstractHeapType::NoExtern,
                    } => WasmReferenceKind::BottomExternal,
                    wasmparser::HeapType::Abstract {
                        shared: false,
                        ty:
                            ty @ (wasmparser::AbstractHeapType::Any
                            | wasmparser::AbstractHeapType::Eq
                            | wasmparser::AbstractHeapType::Struct
                            | wasmparser::AbstractHeapType::Array
                            | wasmparser::AbstractHeapType::I31
                            | wasmparser::AbstractHeapType::None
                            | wasmparser::AbstractHeapType::Exn
                            | wasmparser::AbstractHeapType::NoExn),
                    } => WasmReferenceKind::Internal(ty),
                    _ => return None,
                };
                Some(Self::Reference {
                    kind,
                    nullable: ty.is_nullable(),
                })
            }
        }
    }

    pub fn default_value(self) -> Option<WasmValue> {
        Some(match self {
            Self::I32 => WasmValue::I32(0),
            Self::I64 => WasmValue::I64(0),
            Self::F32 => WasmValue::F32(0),
            Self::F64 => WasmValue::F64(0),
            Self::V128 => WasmValue::V128(0),
            Self::Reference {
                nullable: false, ..
            } => return None,
            Self::Reference {
                kind:
                    WasmReferenceKind::Function
                    | WasmReferenceKind::DeclaredFunction { .. }
                    | WasmReferenceKind::BottomFunction,
                nullable: true,
            } => WasmValue::FuncRef(crate::Value::NULL),
            Self::Reference {
                kind: WasmReferenceKind::External | WasmReferenceKind::BottomExternal,
                nullable: true,
            } => WasmValue::ExternRef(crate::Value::NULL),
            Self::Reference {
                kind: WasmReferenceKind::Internal(_) | WasmReferenceKind::DeclaredGc { .. },
                nullable: true,
            } => WasmValue::GcRef(crate::Value::NULL),
        })
    }
}

/// Floating-point variants contain IEEE bits, preserving NaN payloads and -0.
/// Reference variants hold values belonging to the invoking Runtime. Root a
/// returned heap reference with Runtime::root before subsequent VM work, and
/// keep that root alive while retaining or passing it back to the runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WasmValue {
    I32(i32),
    I64(i64),
    F32(u32),
    F64(u64),
    V128(u128),
    /// Shared VM references follow the Runtime Value rooting contract.
    FuncRef(crate::Value),
    ExternRef(crate::Value),
    GcRef(crate::Value),
}

impl WasmValue {
    /// The type of a numeric scalar; `None` for a reference.
    pub fn numeric_type(self) -> Option<WasmType> {
        Some(match self {
            Self::I32(_) => WasmType::I32,
            Self::I64(_) => WasmType::I64,
            Self::F32(_) => WasmType::F32,
            Self::F64(_) => WasmType::F64,
            Self::V128(_) => WasmType::V128,
            Self::FuncRef(_) | Self::ExternRef(_) | Self::GcRef(_) => return None,
        })
    }

    pub fn fits_type(self, ty: WasmType) -> bool {
        let WasmType::Reference { kind, nullable } = ty else {
            return self.numeric_type() == Some(ty);
        };
        let value = match (self, kind) {
            (
                Self::FuncRef(value),
                WasmReferenceKind::Function | WasmReferenceKind::DeclaredFunction { .. },
            ) => value,
            (Self::ExternRef(value), WasmReferenceKind::External) => value,
            (
                Self::GcRef(value),
                WasmReferenceKind::Internal(wasmparser::AbstractHeapType::None),
            )
            | (Self::FuncRef(value), WasmReferenceKind::BottomFunction)
            | (Self::ExternRef(value), WasmReferenceKind::BottomExternal) => {
                return nullable && value.is_null();
            }
            (
                Self::GcRef(value),
                WasmReferenceKind::Internal(_) | WasmReferenceKind::DeclaredGc { .. },
            ) => value,
            _ => return false,
        };
        !value.is_null() || nullable
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

    /// The scalar of type `ty` with bits `bits`, as `bits` reads it back
    /// (the low word for 32-bit types).
    #[inline(always)]
    pub fn from_bits(ty: WasmType, bits: u64) -> Self {
        match ty {
            WasmType::I32 => Self::I32(bits as u32 as i32),
            WasmType::F32 => Self::F32(bits as u32),
            WasmType::I64 => Self::I64(bits as i64),
            WasmType::F64 => Self::F64(bits),
            _ => unreachable!("numeric scalar type"),
        }
    }

    #[inline(always)]
    pub fn bits(self) -> Option<ScalarBits> {
        Some(match self {
            Self::I32(value) => ScalarBits::Bits32(value as u32),
            Self::F32(bits) => ScalarBits::Bits32(bits),
            Self::I64(value) => ScalarBits::Bits64(value as u64),
            Self::F64(bits) => ScalarBits::Bits64(bits),
            Self::V128(bits) => ScalarBits::Bits128(bits.to_le_bytes()),
            Self::FuncRef(_) | Self::ExternRef(_) | Self::GcRef(_) => return None,
        })
    }
}

#[derive(Clone, Copy)]
pub enum ScalarBits {
    Bits32(u32),
    Bits64(u64),
    Bits128([u8; V128_BYTES]),
}

impl WasmType {
    pub fn decode(self, bits: ScalarBits) -> Option<WasmValue> {
        match (self, bits) {
            (Self::I32, ScalarBits::Bits32(bits)) => Some(WasmValue::I32(bits as i32)),
            (Self::F32, ScalarBits::Bits32(bits)) => Some(WasmValue::F32(bits)),
            (Self::I64, ScalarBits::Bits64(bits)) => Some(WasmValue::I64(bits as i64)),
            (Self::F64, ScalarBits::Bits64(bits)) => Some(WasmValue::F64(bits)),
            (Self::V128, ScalarBits::Bits128(bits)) => {
                Some(WasmValue::V128(u128::from_le_bytes(bits)))
            }
            _ => None,
        }
    }
}

#[cfg(not(stencil_build))]
mod glue;
#[cfg(not(stencil_build))]
pub use glue::*;
