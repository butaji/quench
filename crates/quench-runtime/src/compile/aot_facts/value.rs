//! The facts the closed-world analyses carry: what a register holds, and which binding a write
//! reaches.

use crate::typed_ir::{Representation, Type};

/// A binding: a global name (by atom), or a slot of the environment or frame of `owner`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Key {
    Global(u32),
    Slot { owner: u32, slot: u16 },
    /// The named property `atom` of any object: every store of that name, wherever it lands.
    Member(u32),
}

/// Where a register's value was read from, which the census follows to the closure it names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Origin {
    /// The value the binding held when the register was loaded from it.
    Binding(Key),
    /// The value of a named property `atom` read off some object (a method read before its call).
    Member(u32),
}

/// What a register is known to hold at a point: its type in the one fact lattice
/// (`typed_ir::Type`: a closure's body, int32, Number, any value) and, when it was read from a
/// binding or a named property, where from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Fact {
    pub(super) ty: Type,
    pub(super) origin: Option<Origin>,
}

impl Fact {
    /// Any value, from nowhere the census follows.
    pub(super) const UNKNOWN: Self = Self::of(Type::ANY);

    /// A value of type `ty`.
    pub(super) const fn of(ty: Type) -> Self {
        Self { ty, origin: None }
    }

    /// Any value, read from `origin`.
    pub(super) const fn from(origin: Origin) -> Self {
        Self { ty: Type::ANY, origin: Some(origin) }
    }

    /// The closure of function `function`.
    pub(super) const fn closure(function: u32) -> Self {
        Self::of(Type::function_body(function))
    }

    pub(super) fn is_number(self) -> bool {
        self.ty != Type::NEVER && self.ty.is_subtype(Type::NUMBER)
    }

    /// The fact both describe: the join of their types, from the origin both share.
    pub(super) fn join(self, other: Self) -> Self {
        Self { ty: self.ty.join(other.ty), origin: if self.origin == other.origin { self.origin } else { None } }
    }

    /// The fact of the Number `value`: an int32 unless it is not one, or is `-0`.
    pub(super) fn number(value: f64) -> Self {
        let int = value as i32;
        Self::of(if f64::from(int) == value && (int != 0 || value.is_sign_positive()) { Type::INT32 } else { Type::NUMBER })
    }

    /// The representation a register holding this fact at every point has.
    pub(super) fn representation(self) -> Option<Representation> {
        [Representation::Int32, Representation::Float64].into_iter().find(|representation| representation.accepts(self.ty))
    }
}
