//! The Wasm instance's state as staged rows (`Decoder::WasmState`): a global's load and store, and
//! the fill of a register window with one constant. The global's cell is the VM's: the frame loads
//! and stores it through the instance rule (`Frame::wasm_global_load`, `Frame::wasm_global_store`),
//! and a binding that is no mutable global leaves the row to its miss, which raises.

use super::register_frame::WasmRep;
use super::{Concrete, Constant, DecodeContext, Fields, Frame, Operation, Step};
use crate::bytecode::{Op, Register};

/// An operation on the instance's state, with the operands its record names.
#[derive(Clone, Copy)]
pub(crate) enum WasmStateOperation {
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

impl WasmStateOperation {
    /// The operation `instruction` denotes, from its record's fields; a fill's constant is the
    /// record's constant index, read through `context` (or the lane's own constant).
    pub(crate) fn decode<F: Fields>(instruction: F, context: DecodeContext<'_>) -> Option<Self> {
        Some(match instruction.op() {
            Op::WasmGlobalGet => Self::GlobalGet {
                destination: instruction.result_register(),
                binding: instruction.register_b(),
            },
            Op::WasmGlobalSet => Self::GlobalSet {
                source: instruction.register_a(),
                binding: instruction.register_b(),
            },
            Op::WasmInstanceBinding => Self::InstanceBinding {
                destination: instruction.result_register(),
                slot: instruction.wide().imm(),
            },
            Op::WasmFillRegisters => {
                let window = instruction.wide().register_window();
                Self::Fill {
                    constant: instruction.constant(context)?,
                    base: window.base,
                    count: window.count,
                }
            }
            _ => return None,
        })
    }
}

impl<W, P> Operation<W, P> {
    /// The instance-state operation `instruction` denotes (`Decoder::WasmState`).
    pub fn decode_wasm_state<F: Fields>(instruction: F, context: DecodeContext<'_>) -> Option<Self> {
        WasmStateOperation::decode(instruction, context).map(Self::WasmState)
    }
}

/// Run `operation` on `frame`: its value, or `Unresolved` where the frame leaves it. A global store
/// answers the word it stored; a fill answers the constant it wrote.
#[inline(always)]
pub(crate) fn step<F: Frame<Concrete>>(
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
