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
pub enum Instance {
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
    /// `a = frame local imm`: a block's saved input, or the thrown value a handler binds.
    LoadLocal,
    /// `frame local imm = a`.
    StoreLocal,
}

/// The register holding frame local `slot` of a function with `registers` registers: the u-jit
/// keeps its locals after its registers, in the same register file. A slot past the file's end
/// saturates to the last register, which no state holds, so its use is rejected.
#[cfg(not(stencil_build))]
pub fn local_register(registers: u16, slot: u32) -> u16 {
    u16::try_from(slot).map_or(u16::MAX, |slot| registers.saturating_add(slot))
}

#[cfg(not(stencil_build))]
impl Instance {
    pub const fn of(op: Op) -> Option<Self> {
        match op.class() {
            Class::Move => Some(Self::Move),
            Class::LoadConst => Some(Self::Constant),
            Class::WasmFillRegisters => Some(Self::Fill),
            Class::WasmInstanceBinding => Some(Self::Binding),
            Class::WasmGlobalGet => Some(Self::GlobalGet),
            Class::WasmGlobalSet => Some(Self::GlobalSet),
            Class::LoadLocal => Some(Self::LoadLocal),
            Class::StoreLocal => Some(Self::StoreLocal),
            _ => None,
        }
    }

    /// The registers a constant operation defines: one, or a window.
    pub fn constant_registers(self, instruction: WideInstruction) -> Option<Range<u16>> {
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
pub enum Storage {
    /// An i32 or f32: the payload in the low 32 bits.
    Bits32,
    /// An i64 or f64: the payload whole.
    Bits64,
    /// A reference or a v128: the bits of the handle to the object or the vector's box, which
    /// the collector follows.
    Handle,
}

impl Storage {
    pub const fn of(ty: WasmType) -> Self {
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
pub trait WasmInstance: WasmStage {
    /// The word of the global bound at environment slot `slot`.
    fn global_load(&mut self, slot: u32, storage: Storage) -> Self::Word;
    fn global_store(&mut self, slot: u32, storage: Storage, value: Self::Word);
}

/// `global.get`: the word of the global bound at `slot`, as its storage holds it. Every tier's
/// global read is this rule over its own stage.
#[inline(always)]
pub fn global_get<S: WasmInstance>(stage: &mut S, slot: u32, storage: Storage) -> S::Word {
    stage.global_load(slot, storage)
}

/// `global.set`: `value` into the global bound at `slot`, narrowed by its storage.
#[inline(always)]
pub fn global_set<S: WasmInstance>(stage: &mut S, slot: u32, storage: Storage, value: S::Word) {
    stage.global_store(slot, storage, value);
}

/// The instance fields a binding step reads: the environment slots its function's instance
/// holds, each a handle the binding step copies into a register (`Instance::Binding`).
#[cfg(not(stencil_build))]
pub(crate) trait WasmBindings: WasmStage {
    /// The word of the environment slot `slot`, or `None` where the instance holds no field there.
    fn binding_word(&mut self, slot: u32) -> Option<Self::Word>;
}

/// `binding`: the word of the instance field at environment slot `slot`. A slot with no field
/// is `None`, which the caller leaves for the general path to raise.
#[cfg(not(stencil_build))]
#[inline(always)]
pub fn binding_get<S: WasmBindings>(stage: &mut S, slot: u32) -> Option<S::Word> {
    stage.binding_word(slot)
}
