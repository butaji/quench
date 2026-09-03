//! One register slot: Native | Fast | Dynamic. Storage is on the payload.

use crate::dynamic::Dynamic;
use crate::fast::Fast;
use crate::hir::{Kind, Ty};
use crate::layer::{GuardKind, Layer};
use crate::native::Native;

/// Canonical runtime value. Not a fourth "universal" type: it *is* the ladder.
#[derive(Clone, Debug, PartialEq)]
pub enum Slot {
    Native(Native),
    Fast(Fast),
    Dynamic(Dynamic),
}

impl Slot {
    pub fn layer(&self) -> Layer {
        match self {
            Self::Native(_) => Layer::Native,
            Self::Fast(_) => Layer::Fast,
            Self::Dynamic(_) => Layer::Dynamic,
        }
    }

    pub fn storage(&self) -> crate::layer::Storage {
        match self {
            Self::Native(crate::native::Native::Ref(_)) | Self::Dynamic(_) => {
                crate::layer::Storage::Gc
            }
            _ => crate::layer::Storage::Arena,
        }
    }

    pub fn native_i32(value: i32) -> Self {
        Self::Native(Native::I32(value))
    }

    pub fn as_native_i32(&self) -> Option<i32> {
        match self {
            Self::Native(Native::I32(value)) => Some(*value),
            _ => None,
        }
    }

    pub fn zero(ty: Ty) -> Option<Self> {
        match (ty.layer, ty.kind) {
            (Layer::Native, Kind::I32) => Some(Self::Native(Native::I32(0))),
            (Layer::Native, Kind::I64) => Some(Self::Native(Native::I64(0))),
            (Layer::Native, Kind::F32) => Some(Self::Native(Native::F32(0))),
            (Layer::Native, Kind::F64) => Some(Self::Native(Native::F64(0))),
            (Layer::Native, Kind::V128) => Some(Self::Native(Native::V128(0))),
            (Layer::Native, Kind::Ref) => {
                Some(Self::Native(Native::Ref(crate::native::RefVal::Null)))
            }
            (Layer::Fast, Kind::I32) => Some(Self::Fast(Fast::I32(0))),
            (Layer::Fast, Kind::F64) => Some(Self::Fast(Fast::Number(0.0))),
            (Layer::Dynamic, _) => Some(Self::Dynamic(Dynamic::undefined())),
            _ => None,
        }
    }

    /// Native/Fast → Dynamic. The only way a proven value becomes untyped.
    pub fn box_dynamic(&self) -> Option<Self> {
        match self {
            Self::Native(Native::I32(value)) => {
                Some(Self::Dynamic(Dynamic::from_number(*value as f64)))
            }
            Self::Native(Native::F64(bits)) => {
                Some(Self::Dynamic(Dynamic::from_number(f64::from_bits(*bits))))
            }
            Self::Fast(fast) => Some(Self::Dynamic(Dynamic::from_number(fast.as_number()?))),
            Self::Dynamic(_) => Some(self.clone()),
            _ => None,
        }
    }

    /// Dynamic → Fast. Failure leaves the value Dynamic.
    pub fn guard(&self, kind: GuardKind) -> Option<Self> {
        let number = match self {
            Self::Dynamic(dynamic) => dynamic.as_number()?,
            Self::Fast(fast) => fast.as_number()?,
            Self::Native(Native::I32(value)) => *value as f64,
            Self::Native(Native::F64(bits)) => f64::from_bits(*bits),
            _ => return None,
        };
        match kind {
            GuardKind::Number => Some(Self::Fast(Fast::Number(number))),
            GuardKind::I32 if number as i32 as f64 == number => {
                Some(Self::Fast(Fast::I32(number as i32)))
            }
            GuardKind::I32 => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Slot;
    use crate::layer::GuardKind;

    #[test]
    fn box_then_guard_i32() {
        let native = Slot::native_i32(7);
        let dynamic = native.box_dynamic().expect("box");
        assert_eq!(dynamic.layer(), crate::layer::Layer::Dynamic);
        let fast = dynamic.guard(GuardKind::I32).expect("guard");
        assert_eq!(fast, Slot::Fast(crate::fast::Fast::I32(7)));
    }

    #[test]
    fn native_scalar_is_arena_ref_is_gc() {
        assert_eq!(Slot::native_i32(1).storage(), crate::layer::Storage::Arena);
        assert_eq!(
            Slot::Native(crate::native::Native::Ref(crate::native::RefVal::Null)).storage(),
            crate::layer::Storage::Gc
        );
    }
}
