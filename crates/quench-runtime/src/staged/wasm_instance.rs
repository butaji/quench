//! The registers and globals of a Wasm function as data: which opcode moves a register, loads
//! a constant, binds an instance field or accesses a global, and the one primitive set
//! (`WasmInstance`) every tier implements for the state of the instance. The decisions about a
//! global's type and where it lives are binding-time facts a tier reads once, not per access.

use super::wasm::WasmStage;
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

/// The state of the instance a function runs in. A global holds an i32 here: a wider value is
/// boxed and stays with the interpreter.
pub(crate) trait WasmInstance: WasmStage {
    /// The i32 value of the global bound at environment slot `slot`.
    fn global_load(&mut self, slot: u32) -> Self::Word;
    fn global_store(&mut self, slot: u32, value: Self::Word);
}
