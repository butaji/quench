//! Immediate i31 references use the shared integer Value and retain 31 payload bits.
use super::{WasmReferenceKind, WasmTrap, WasmType, WasmValue};
use crate::Value;

pub(crate) const I31_BITS: u32 = i32::BITS - 1;
pub(crate) const I31_MASK: i32 = (u32::MAX >> (u32::BITS - I31_BITS)) as i32;
const I31_SIGN_EXTENSION_SHIFT: u32 = i32::BITS - I31_BITS;

pub(crate) fn bits(value: Value) -> Option<i32> {
    value.as_int().filter(|bits| (0..=I31_MASK).contains(bits))
}

pub(crate) const SIGNED_MIN: i32 = -(1 << (I31_BITS - 1));
pub(crate) const SIGNED_MAX: i32 = (1 << (I31_BITS - 1)) - 1;

pub(crate) fn signed(bits: i32) -> i32 {
    (bits << I31_SIGN_EXTENSION_SHIFT) >> I31_SIGN_EXTENSION_SHIFT
}

pub(crate) const REFERENCE_TYPE: WasmType = WasmType::Reference {
    kind: WasmReferenceKind::Internal(wasmparser::AbstractHeapType::I31),
    nullable: false,
};

super::numeric::selectors! { I31Operator;
    New, RefI31;
    GetSigned, I31GetS;
    GetUnsigned, I31GetU;
}

impl I31Operator {
    pub(crate) fn input_type(self) -> WasmType {
        match self {
            Self::New => WasmType::I32,
            Self::GetSigned | Self::GetUnsigned => WasmType::Reference {
                kind: WasmReferenceKind::Internal(wasmparser::AbstractHeapType::I31),
                nullable: true,
            },
        }
    }

    pub(crate) fn apply(self, value: WasmValue) -> Result<WasmValue, WasmTrap> {
        Ok(match (self, value) {
            (Self::New, WasmValue::I32(value)) => {
                WasmValue::GcRef(Value::integer(value & I31_MASK))
            }
            (Self::GetSigned | Self::GetUnsigned, WasmValue::GcRef(value)) => {
                if value.is_null() {
                    return Err(WasmTrap::NullI31Reference);
                }
                let bits = bits(value).expect("decoded i31 reference");
                WasmValue::I32(if self == Self::GetSigned {
                    signed(bits)
                } else {
                    bits
                })
            }
            _ => unreachable!("decoded i31 operand"),
        })
    }
}
