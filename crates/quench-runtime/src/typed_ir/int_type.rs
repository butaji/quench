//! Integer ranges inside the type lattice: a Number known to be an integer within a range is a
//! subtype of `NUMBER`, and `INT32` is the range of the whole int32 span. The join of two ranges
//! is their hull; widening (`Type::widen`) is the join that settles a growing range at int32's.

use crate::typed_ir::{IntRange, Type, TypeKind};

impl Type {
    /// The integers within `range`: any such range is a Number type, and one that fits int32
    /// (`is_int32`) is what a machine integer holds.
    pub const fn int(range: IntRange) -> Self { Self(TypeKind::Int(range)) }

    /// The range of an integer type; `None` for any other.
    pub const fn int_range(self) -> Option<IntRange> {
        match self.0 {
            TypeKind::Int(range) => Some(range),
            _ => None,
        }
    }

    /// Whether this is an integer type whose range fits int32.
    pub fn is_int32(self) -> bool { self.int_range().is_some_and(|range| range.fits_int32()) }

    /// What a machine view of a value of this type proves of it: an integer type is int32 where
    /// its range fits and a Number where it does not, and a NaN-free Number is a Number. The range and
    /// the NaN bit are the analysis's: no load or edge checks them, so a representation conforms to
    /// this type, not to either refinement (a Float64 word is a Number whether or not it is NaN).
    pub fn machine(self) -> Self {
        match self.int_range() {
            Some(_) if self.is_int32() => Self::INT32,
            Some(_) => Self::NUMBER,
            None if self == Self::NOT_NAN_NUMBER => Self::NUMBER,
            None => self,
        }
    }

    /// The join where it needs no case analysis: equal types, `NEVER`, two integer ranges, or any mix of
    /// integers and NaN-free Numbers, which is NaN-free (`NOT_NAN_NUMBER`).
    pub(super) fn trivial_join(self, other: Self) -> Option<Self> {
        if self == other || other == Self::NEVER { return Some(self); }
        if self == Self::NEVER { return Some(other); }
        match (self.0, other.0) {
            (TypeKind::Int(known), TypeKind::Int(incoming)) => Some(Self::int(known.join(incoming))),
            (TypeKind::Int(_) | TypeKind::NotNaN, TypeKind::Int(_) | TypeKind::NotNaN) => Some(Self::NOT_NAN_NUMBER),
            _ => None,
        }
    }

    /// `self` joined with `next`, where an integer range that grew settles at int32's span.
    pub fn widen(self, next: Self) -> Self {
        match (self.0, next.0) {
            (TypeKind::Int(known), TypeKind::Int(incoming)) => Self::int(known.widen(incoming)),
            _ => self.join(next),
        }
    }
}
