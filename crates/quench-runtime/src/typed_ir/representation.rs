//! How a value is held in machine form.

use crate::typed_ir::{IntRange, Type};

/// How a value is held in machine form. It is also how the word of an arena slot encodes its
/// value: a static projection of the slot owner's type (`SlotType::rep`), not a fact stored apart
/// from it. A specializer that proved the owner's type under a dependency reads and writes the
/// word in this representation with no tag test.
///
/// The variants are ordered narrowest to widest (`Int32`, `Int64`, `Float64`, `Tagged`), so the
/// greatest of several representations is the widest one that holds all of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Representation {
    /// A `Value` whose tag the owner proved int32; the bits are the tagged word's.
    Int32,
    /// An integer Number of `IntRange::EXACT` in a signed 64-bit word: the Number's own value,
    /// which an index word and a sum of index words need no binary64 round trip to share.
    Int64,
    /// The Number as its IEEE double, which every Number is stored as (NaN canonical): a raw
    /// double is already a valid `Value` word, so the collector reads it as a non-reference.
    Float64,
    /// Any `Value`, as the interpreter holds it.
    Tagged,
}

impl Representation {
    /// Implicit views supported by the shared emitter. Narrowing and unboxing
    /// require explicit checked operations, never a register assignment.
    #[cfg(test)]
    pub(crate) fn can_receive(self, source: Self) -> bool {
        self == source || self == Self::Tagged
            || (self == Self::Int64 && source == Self::Int32)
            || (self == Self::Float64 && matches!(source, Self::Int32 | Self::Int64))
    }
    pub fn accepts(self, ty: Type) -> bool {
        ty != Type::NEVER
            && match self {
                Self::Int32 => ty.is_subtype(Type::INT32),
                Self::Int64 => ty.int_range().is_some_and(|range| IntRange::EXACT.contains(range)),
                Self::Float64 => ty.is_subtype(Type::NUMBER),
                Self::Tagged => true,
            }
    }
}

/// The narrowest representation that accepts every value of `ty`: `Tagged` for `NEVER` and for any
/// type no unboxed view holds.
#[inline]
pub fn repr(ty: Type) -> Representation {
    [Representation::Int32, Representation::Int64, Representation::Float64]
        .into_iter().find(|representation| representation.accepts(ty)).unwrap_or(Representation::Tagged)
}
