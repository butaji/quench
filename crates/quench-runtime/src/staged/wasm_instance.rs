//! The registers and globals of a Wasm function as data: which opcode moves a register, loads
//! a constant, binds an instance field or accesses a global, and the one primitive set
//! (`WasmInstance`) every tier implements for the state of the instance. The decisions about a
//! global's type and where it lives are binding-time facts a tier reads once, not per access.

use super::wasm::WasmStage;
use crate::WasmType;
use crate::bytecode::{Class, Op, WideInstruction};
use std::ops::Range;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Instance {
    /// `a = b`.
    Move,
    /// `a = constant[index]`.
    Constant,
    /// Every register of a window `= constant[index]`.
    Fill,
    /// `a = environment slot imm`: the instance field a later access names.
    Binding,
    /// `a = global bound in b`.
    GlobalGet,
    /// The global bound in `b` `= a`.
    GlobalSet,
}

impl Instance {
    pub(crate) fn of(op: Op) -> Option<Self> {
        match op.class() {
            Class::Move => Some(Self::Move),
            Class::LoadConst => Some(Self::Constant),
            Class::WasmFillRegisters => Some(Self::Fill),
            Class::WasmInstanceBinding => Some(Self::Binding),
            Class::WasmGlobalGet => Some(Self::GlobalGet),
            Class::WasmGlobalSet => Some(Self::GlobalSet),
            _ => None,
        }
    }

    /// The registers a constant operation defines: one, or a window.
    pub(crate) fn constant_registers(self, instruction: WideInstruction) -> Option<Range<u16>> {
        match self {
            Self::Constant => Some(instruction.a()..instruction.a() + 1),
            Self::Fill => {
                let window = instruction.register_window();
                Some(window.base..window.base + window.count)
            }
            _ => None,
        }
    }

    /// Whether it reads the global its `b` register binds.
    pub(crate) const fn accesses_global(self) -> bool {
        matches!(self, Self::GlobalGet | Self::GlobalSet)
    }
}

/// How a global's cell holds its value, decided once from the global's type: the static operand
/// of every access. A cell is one encoded slot, the interpreter's register representation.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Storage {
    /// An i32 or f32: the low 32 bits of the slot, under the integer tag.
    Bits32,
    /// An i64 or f64: the slot's 64 bits whole, in place or a handle to the box of a payload in
    /// the tag space.
    Bits64,
    /// A reference or a v128: the slot whole, a handle to the object or the vector's box.
    Handle,
}

impl Storage {
    pub(crate) const fn of(ty: WasmType) -> Self {
        match ty {
            WasmType::I32 | WasmType::F32 => Self::Bits32,
            WasmType::I64 | WasmType::F64 => Self::Bits64,
            _ => Self::Handle,
        }
    }
}

/// The state of the instance a function runs in. A word is a global's value as the stage's
/// registers hold it: a 32-bit payload zero-extended; a 64-bit one as the interpreter holds it
/// (its cell's slot, in place or a box's handle) or as the tier computes with it (the payload,
/// which the tier reads through the VM's box where the cell holds one); a handle whole.
pub(crate) trait WasmInstance: WasmStage {
    /// The word of the global bound at environment slot `slot`.
    fn global_load(&mut self, slot: u32, storage: Storage) -> Self::Word;
    fn global_store(&mut self, slot: u32, storage: Storage, value: Self::Word);
}
