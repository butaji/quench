#![allow(unused_imports)]
use super::*;
use crate::bytecode::{Class, Op, WideInstruction};
use std::ops::Range;

impl Instance {
    /// The instance step `op` is (`Decoder::WasmInstance`), or none for an opcode it does not name.
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
}

/// The registers a constant step defines, read from its bytecode: runtime only.
pub trait InstanceWasmInstanceExt: Sized {
    /// The registers a constant operation defines: one, or a window.
    fn constant_registers(self, instruction: WideInstruction) -> Option<Range<u16>>;
}

impl InstanceWasmInstanceExt for Instance {
    fn constant_registers(self, instruction: WideInstruction) -> Option<Range<u16>> {
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
