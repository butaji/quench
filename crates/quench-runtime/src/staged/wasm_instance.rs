//! The registers and globals of a Wasm function as data: which opcode moves a register, loads
//! a constant, binds an instance field or accesses a global, and the one primitive set
//! (`WasmInstance`) every tier implements for the state of the instance. The decisions about a
//! global's type and where it lives are binding-time facts a tier reads once, not per access.

use super::wasm::WasmStage;
use crate::WasmType;
#[cfg(not(stencil_build))]
use crate::bytecode::{Class, Op, WideInstruction};
#[cfg(not(stencil_build))]
use std::ops::Range;

/// The binding-time decisions the interpreter and the u-jit make per instruction; the stencil
/// build does not decode instructions, so it has none of them.
#[cfg(not(stencil_build))]
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

#[cfg(not(stencil_build))]
impl Instance {
    pub(crate) const fn of(op: Op) -> Option<Self> {
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
}

/// How a global's native slot (`Cell::WasmGlobal::bits`) holds its value, decided once from the
/// global's type: the static operand of every access. A slot is one raw word.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Storage {
    /// An i32 or f32: the payload in the low 32 bits.
    Bits32,
    /// An i64 or f64: the payload whole.
    Bits64,
    /// A reference or a v128: the bits of the handle to the object or the vector's box, which
    /// the collector follows.
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

    /// The word a slot holds for `word`: a 32-bit payload zero-extended, any other whole.
    pub(crate) const fn narrow(self, word: u64) -> u64 {
        match self {
            Self::Bits32 => word as u32 as u64,
            Self::Bits64 | Self::Handle => word,
        }
    }
}

/// The state of the instance a function runs in. A word is a global's raw slot as the stage's
/// registers hold it: a 32-bit payload zero-extended, a 64-bit payload whole, a handle's bits.
pub(crate) trait WasmInstance: WasmStage {
    /// The word of the global bound at environment slot `slot`.
    fn global_load(&mut self, slot: u32, storage: Storage) -> Self::Word;
    fn global_store(&mut self, slot: u32, storage: Storage, value: Self::Word);
}

/// `global.get`: the word of the global bound at `slot`, as its storage holds it. Every tier's
/// global read is this rule over its own stage.
#[inline(always)]
pub(crate) fn global_get<S: WasmInstance>(stage: &mut S, slot: u32, storage: Storage) -> S::Word {
    stage.global_load(slot, storage)
}

/// `global.set`: `value` into the global bound at `slot`, narrowed by its storage.
#[inline(always)]
pub(crate) fn global_set<S: WasmInstance>(stage: &mut S, slot: u32, storage: Storage, value: S::Word) {
    stage.global_store(slot, storage, value);
}
