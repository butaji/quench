#![allow(unused_imports)]
use super::*;
use crate::bytecode::{Class, Op, WideInstruction};
use std::ops::Range;

/// The instance step `op` is (`Decoder::WasmInstance`), or none for an opcode it does not name. A
/// const fn, so a free fn: an inherent impl of `Instance` cannot live outside quench-staged.
pub const fn instance_of(op: Op) -> Option<Instance> {
    match op.class() {
        Class::Move => Some(Instance::Move),
        Class::LoadConst => Some(Instance::Constant),
        Class::WasmFillRegisters => Some(Instance::Fill),
        Class::WasmInstanceBinding => Some(Instance::Binding),
        Class::WasmGlobalGet => Some(Instance::GlobalGet),
        Class::WasmGlobalSet => Some(Instance::GlobalSet),
        Class::LoadLocal => Some(Instance::LoadLocal),
        Class::StoreLocal => Some(Instance::StoreLocal),
        _ => None,
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
