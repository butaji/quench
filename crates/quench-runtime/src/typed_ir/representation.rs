//! How a value is held in machine form.
#![allow(dead_code)]

use super::Type;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Representation {
    Int32,
    Float64,
    Tagged,
}

/// What a value in one representation is known to be once a load checked it, and the
/// register view that holds it.
struct RepresentationRow {
    checked: Type,
    view: crate::WasmType,
}

/// One row per [`Representation`], in its order.
const REPRESENTATIONS: [RepresentationRow; 3] = [
    RepresentationRow { checked: Type::INT32, view: crate::WasmType::I32 },
    RepresentationRow { checked: Type::NUMBER, view: crate::WasmType::F64 },
    RepresentationRow { checked: Type::ANY, view: crate::WasmType::I64 },
];

impl Representation {
    /// The type a checked load in this representation establishes: unboxing proves the
    /// Number set, a tagged load proves nothing.
    pub(crate) const fn checked_type(self) -> Type {
        REPRESENTATIONS[self as usize].checked
    }

    /// The register view that holds a value in this representation.
    pub(crate) const fn view(self) -> crate::WasmType {
        REPRESENTATIONS[self as usize].view
    }

    /// Implicit views supported by the shared emitter. Narrowing and unboxing
    /// require explicit checked operations, never a register assignment.
    pub(crate) fn can_receive(self, source: Self) -> bool {
        self == source || self == Self::Tagged || (self == Self::Float64 && source == Self::Int32)
    }
    pub(crate) fn accepts(self, ty: Type) -> bool {
        ty != Type::NEVER
            && match self {
                Self::Int32 => ty.is_subtype(Type::INT32),
                Self::Float64 => ty.is_subtype(Type::NUMBER),
                Self::Tagged => true,
            }
    }
}
