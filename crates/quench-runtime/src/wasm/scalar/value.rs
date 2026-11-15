use super::*;

impl WasmValue {
    pub(crate) fn fits_type(self, ty: WasmType) -> bool {
        let WasmType::Reference { kind, nullable } = ty else {
            return self.ty() == ty;
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
    pub fn ty(self) -> WasmType {
        match self {
            Self::I32(_) => WasmType::I32,
            Self::I64(_) => WasmType::I64,
            Self::F32(_) => WasmType::F32,
            Self::F64(_) => WasmType::F64,
            Self::V128(_) => WasmType::V128,
            Self::FuncRef(_) => WasmType::FUNCREF,
            Self::ExternRef(_) => WasmType::EXTERNREF,
            Self::GcRef(value) if super::i31::bits(value).is_some() => super::i31::REFERENCE_TYPE,
            Self::GcRef(_) => WasmType::Reference {
                kind: WasmReferenceKind::Internal(wasmparser::AbstractHeapType::Any),
                nullable: true,
            },
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

    /// The scalar of type `ty` with bits `bits`, as `bits` reads it back
    /// (the low word for 32-bit types).
    #[inline(always)]
    pub(crate) fn from_bits(ty: WasmType, bits: u64) -> Self {
        match ty {
            WasmType::I32 => Self::I32(bits as u32 as i32),
            WasmType::F32 => Self::F32(bits as u32),
            WasmType::I64 => Self::I64(bits as i64),
            WasmType::F64 => Self::F64(bits),
            _ => unreachable!("numeric scalar type"),
        }
    }

    pub(crate) fn bits(self) -> Option<ScalarBits> {
        Some(match self {
            Self::I32(value) => ScalarBits::Bits32(value as u32),
            Self::F32(bits) => ScalarBits::Bits32(bits),
            Self::I64(value) => ScalarBits::Bits64(value as u64),
            Self::F64(bits) => ScalarBits::Bits64(bits),
            Self::V128(bits) => ScalarBits::Bits128(bits.to_le_bytes()),
            Self::FuncRef(_) | Self::ExternRef(_) | Self::GcRef(_) => return None,
        })
    }

    pub(in crate::wasm) fn constant(self) -> Option<Constant> {
        match self {
            Self::GcRef(value) if super::i31::bits(value).is_some() => Some(Constant::Number(
                f64::from(super::i31::bits(value).unwrap()),
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
