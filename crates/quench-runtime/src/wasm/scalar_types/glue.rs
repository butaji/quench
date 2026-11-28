#![allow(unused_imports)]
use super::*;
use crate::bytecode::Constant;

/// The `WasmValue` methods that name interpreter state: the i31 reference rules (`wasm/i31.rs`)
/// and the bytecode constant form. They stay in the runtime when the data types move.
pub trait WasmValueScalarExt: Sized {
    /// The Wasm type of the value, a reference's type included.
    fn ty(self) -> WasmType;
    /// The constant the bytecode carries for the value; `None` for a non-null reference.
    fn constant(self) -> Option<Constant>;
}

impl WasmValueScalarExt for WasmValue {
    fn ty(self) -> WasmType {
        match self.numeric_type() {
            Some(ty) => ty,
            None => match self {
                Self::GcRef(value) if crate::wasm::i31::bits(value).is_some() => {
                    crate::wasm::i31::REFERENCE_TYPE
                }
                Self::FuncRef(_) => WasmType::FUNCREF,
                Self::ExternRef(_) => WasmType::EXTERNREF,
                Self::GcRef(_) => WasmType::Reference {
                    kind: WasmReferenceKind::Internal(wasmparser::AbstractHeapType::Any),
                    nullable: true,
                },
                _ => unreachable!("numeric scalars are typed by numeric_type"),
            },
        }
    }

    fn constant(self) -> Option<Constant> {
        match self {
            Self::GcRef(value) if crate::wasm::i31::bits(value).is_some() => Some(Constant::Number(
                f64::from(crate::wasm::i31::bits(value).unwrap()),
            )),
            Self::FuncRef(value) | Self::ExternRef(value) | Self::GcRef(value) => {
                value.is_null().then_some(Constant::Null)
            }
            _ => self.bits().map(|bits| match bits {
                ScalarBits::Bits32(bits) => Constant::Number(f64::from(bits as i32)),
                ScalarBits::Bits64(bits) => Constant::WasmBits64(bits),
                ScalarBits::Bits128(bits) => Constant::WasmV128(bits),
            }),
        }
    }
}
