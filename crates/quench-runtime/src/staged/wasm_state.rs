//! The Wasm instance's state as staged rows (`Decoder::WasmState`): a global's load and store, and
//! the fill of a register window with one constant. The global's cell is the VM's: the frame loads
//! and stores it through the instance rule (`Frame::wasm_global_load`, `Frame::wasm_global_store`),
//! and a binding that is no mutable global leaves the row to its miss, which raises. The decode of
//! the operation from its record is in `wasm_state/glue.rs`.

use super::register_frame::WasmRep;
use super::{Concrete, Constant, Frame, Step};
use crate::bytecode::Register;

/// An operation on the instance's state, with the operands its record names.
#[derive(Clone, Copy)]
pub enum WasmStateOperation {
    /// `destination = global bound in binding` (`WasmGlobalGet`).
    GlobalGet { destination: Register, binding: Register },
    /// The global bound in `binding` `= source` (`WasmGlobalSet`).
    GlobalSet { source: Register, binding: Register },
    /// Every register of the window from `base`, `count` of them, `= constant` (`WasmFillRegisters`).
    Fill { constant: Constant, base: Register, count: u16 },
    /// `destination = slot of the running instance` (`WasmInstanceBinding`): the instance's slot
    /// the frame reads, which a memory binding's view update leaves to its hand handler.
    InstanceBinding { destination: Register, slot: u32 },
}

/// Run `operation` on `frame`: its value, or `Unresolved` where the frame leaves it. A global store
/// answers the word it stored; a fill answers the constant it wrote.
#[inline(always)]
pub fn step<F: Frame<Concrete>>(
    stage: &mut Concrete,
    frame: &mut F,
    operation: WasmStateOperation,
) -> Step<u64, F::Control> {
    match operation {
        WasmStateOperation::GlobalGet { destination, binding } => {
            let Some(binding) = frame.wasm_read(stage, binding, WasmRep::Tagged) else {
                return Step::Unresolved;
            };
            let Some(word) = frame.wasm_global_load(stage, binding) else {
                return Step::Unresolved;
            };
            match frame.wasm_write(stage, destination, word, WasmRep::Tagged) {
                Some(tagged) => Step::Value(tagged),
                None => Step::Unresolved,
            }
        }
        WasmStateOperation::GlobalSet { source, binding } => {
            let Some(binding) = frame.wasm_read(stage, binding, WasmRep::Tagged) else {
                return Step::Unresolved;
            };
            let Some(value) = frame.wasm_read(stage, source, WasmRep::Tagged) else {
                return Step::Unresolved;
            };
            match frame.wasm_global_store(stage, binding, value) {
                Some(()) => Step::Value(value),
                None => Step::Unresolved,
            }
        }
        WasmStateOperation::Fill { constant, base, count } => {
            let (word, fact) = frame.constant(stage, constant);
            for register in base..base + count {
                frame.write(stage, register, word, fact);
            }
            Step::Value(word)
        }
        WasmStateOperation::InstanceBinding { destination, slot } => {
            let Some(word) = frame.wasm_instance_slot(stage, slot) else {
                return Step::Unresolved;
            };
            match frame.wasm_write(stage, destination, word, WasmRep::Tagged) {
                Some(tagged) => Step::Value(tagged),
                None => Step::Unresolved,
            }
        }
    }
}

#[cfg(not(stencil_build))]
mod glue;
#[cfg(not(stencil_build))]
pub use glue::*;
