//! The operand encodings an instruction's fields carry: a register, constant, field or local
//! operand (`Operand`), a field read's base (`FieldBase`) and lookup (`FieldLookup`). Plain data
//! over `Register` and `Atom`, so the stencil build (`quench-bjit`) mounts this file as the runtime
//! does and the staged rows decode one encoding.

use crate::bytecode::{Atom, Register};

pub const SET_THIS_REGISTER: Register = 1 << 14;
pub const REGISTER_MASK: Register = SET_THIS_REGISTER - 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct Operand(pub u16);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum OperandKind {
    Register = 0,
    Constant = 1,
    Field = 2,
    Local = 3,
}
#[cfg(feature = "profile-aggregate")]
impl OperandKind {
    pub const COUNT: usize = Self::Local as usize + 1;
}

impl Operand {
    const TAG_SHIFT: u16 = 14;
    pub const PAYLOAD_MASK: u16 = (1 << Self::TAG_SHIFT) - 1;

    #[inline] pub const fn kind(self) -> Option<OperandKind> {
        match self.0 >> Self::TAG_SHIFT {
            tag if tag == OperandKind::Register as u16 => Some(OperandKind::Register),
            tag if tag == OperandKind::Constant as u16 => Some(OperandKind::Constant),
            tag if tag == OperandKind::Field as u16 => Some(OperandKind::Field),
            tag if tag == OperandKind::Local as u16 => Some(OperandKind::Local),
            _ => None,
        }
    }

    #[inline]
    pub fn register(value: Register) -> Self {
        debug_assert!(value <= Self::PAYLOAD_MASK);
        Self(((OperandKind::Register as u16) << Self::TAG_SHIFT) | value)
    }

    /// A constant-pool operand, when the pool index fits the operand payload;
    /// larger programs load the constant into a register instead.
    #[inline]
    pub fn constant(index: u32) -> Option<Self> {
        u16::try_from(index)
            .ok()
            .filter(|index| *index <= Self::PAYLOAD_MASK)
            .map(|index| Self(((OperandKind::Constant as u16) << Self::TAG_SHIFT) | index))
    }

    #[inline]
    pub fn local(slot: u16) -> Self {
        debug_assert!(slot <= Self::PAYLOAD_MASK);
        Self(((OperandKind::Local as u16) << Self::TAG_SHIFT) | slot)
    }

    #[inline]
    pub fn tag(self) -> u16 {
        self.0 >> Self::TAG_SHIFT
    }

    #[inline] pub fn payload(self) -> u16 {
        self.0 & Self::PAYLOAD_MASK
    }

    #[inline(always)] pub fn register_index(self) -> Option<Register> {
        (self.kind() == Some(OperandKind::Register)).then_some(self.payload())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct FieldBase(pub u16);

impl FieldBase {
    pub const THIS: Self = Self(u16::MAX);
    pub const NESTED: u16 = u16::MAX - 1;
    const LOCAL_FLAG: u16 = 0x4000;

    #[inline(always)]
    pub fn register(value: Register) -> Self {
        debug_assert!(value <= REGISTER_MASK);
        Self(value)
    }

    #[inline(always)] pub fn register_index(self) -> Option<Register> {
        (self.0 <= REGISTER_MASK).then_some(self.0)
    }

    #[inline(always)]
    pub fn local(value: u16) -> Option<Self> {
        (value <= REGISTER_MASK).then_some(Self(Self::LOCAL_FLAG | value))
    }

    #[inline(always)] pub fn local_index(self) -> Option<u16> {
        (self.0 & Self::LOCAL_FLAG != 0
            && self.0 & !(Self::LOCAL_FLAG | REGISTER_MASK) == 0)
            .then_some(self.0 & REGISTER_MASK)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldLookup {
    Atom {
        atom: Atom,
        base: FieldBase,
        cache_site: u16,
    },
    Site(usize),
}

/// A run of consecutive registers: a call's arguments, an object literal's values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegisterWindow {
    pub base: Register,
    pub count: u16,
}

impl RegisterWindow {
    /// The window's last register, when it has one.
    pub fn last(self) -> Option<Register> {
        self.count.checked_sub(1).map(|offset| self.base + offset)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConstructArguments {
    Registers(RegisterWindow),
    Array(Register),
}
