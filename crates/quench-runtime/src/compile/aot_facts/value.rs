//! The facts the closed-world analyses carry: what a register holds, and which binding a write
//! reaches.

use crate::typed_ir::Representation;

/// A binding: a global name (by atom), or a slot of the environment or frame of `owner`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Key {
    Global(u32),
    Slot { owner: u32, slot: u16 },
    /// The named property `atom` of any object: every store of that name, wherever it lands.
    Member(u32),
}

/// What a register is known to hold at a point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Fact {
    /// Any value.
    Unknown,
    /// The closure of function `function`.
    Closure(u32),
    /// The value the binding held when the register was loaded from it.
    Binding(Key),
    /// The value of a named property `atom` read off some object (a method read before its call).
    Member(u32),
    /// A Number held as an int32.
    Int32,
    /// A Number.
    Float64,
}

impl Fact {
    pub(super) fn is_number(self) -> bool {
        matches!(self, Self::Int32 | Self::Float64)
    }

    /// The fact both describe: equal facts, or `Float64` where both are Numbers.
    pub(super) fn join(self, other: Self) -> Self {
        match () {
            _ if self == other => self,
            _ if self.is_number() && other.is_number() => Self::Float64,
            _ => Self::Unknown,
        }
    }

    /// The fact of the Number `value`: an int32 unless it is not one, or is `-0`.
    pub(super) fn number(value: f64) -> Self {
        let int = value as i32;
        if f64::from(int) == value && (int != 0 || value.is_sign_positive()) {
            Self::Int32
        } else {
            Self::Float64
        }
    }

    /// The representation a register holding this fact at every point has.
    pub(super) fn representation(self) -> Option<Representation> {
        match self {
            Self::Int32 => Some(Representation::Int32),
            Self::Float64 => Some(Representation::Float64),
            _ => None,
        }
    }
}
