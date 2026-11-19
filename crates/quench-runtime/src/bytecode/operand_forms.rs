//! The operand forms of a record that the staged rows carry as data: a register window, a
//! construction's arguments, a named field access's base and lookup, and the result register's
//! flag bits. This file names nothing else of the runtime, so the b-jit stencils include it by
//! path too (`quench-bjit/stencils/facade.rs`).

use super::{Atom, Register};

pub(crate) const RETURN_REGISTER: Register = 1 << 15;
pub(crate) const SET_THIS_REGISTER: Register = 1 << 14;
pub(crate) const REGISTER_MASK: Register = SET_THIS_REGISTER - 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RegisterWindow {
    pub(crate) base: Register,
    pub(crate) count: u16,
}

impl RegisterWindow {
    /// The window's last register, when it has one.
    pub(crate) fn last(self) -> Option<Register> {
        self.count.checked_sub(1).map(|offset| self.base + offset)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConstructArguments {
    Registers(RegisterWindow),
    Array(Register),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FieldLookup {
    Atom {
        atom: Atom,
        base: FieldBase,
        cache_site: u16,
    },
    Site(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub(crate) struct FieldBase(pub(crate) u16);

impl FieldBase {
    pub(crate) const THIS: Self = Self(u16::MAX);
    pub(crate) const NESTED: u16 = u16::MAX - 1;
    const LOCAL_FLAG: u16 = 0x4000;

    #[inline(always)]
    pub(crate) fn register(value: Register) -> Self {
        debug_assert!(value <= REGISTER_MASK);
        Self(value)
    }

    #[inline(always)] pub(crate) fn register_index(self) -> Option<Register> {
        (self.0 <= REGISTER_MASK).then_some(self.0)
    }

    #[inline(always)]
    pub(crate) fn local(value: u16) -> Option<Self> {
        (value <= REGISTER_MASK).then_some(Self(Self::LOCAL_FLAG | value))
    }

    #[inline(always)] pub(crate) fn local_index(self) -> Option<u16> {
        (self.0 & Self::LOCAL_FLAG != 0
            && self.0 & !(Self::LOCAL_FLAG | REGISTER_MASK) == 0)
            .then_some(self.0 & REGISTER_MASK)
    }
}
