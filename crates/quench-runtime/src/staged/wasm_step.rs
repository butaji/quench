//! What a Wasm function's instruction is, as data: the one table every tier reads (the
//! analysis, the emitter, the loop and threading passes). No tier names a Wasm opcode.

use super::wasm_instance::Instance;
use super::wasm_operation::WasmOperation;
use crate::bytecode::Op;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Step {
    /// Integer, float and conversion arithmetic, and the jumps on it.
    Numeric(WasmOperation),
    /// Registers, constants, instance bindings and globals.
    Instance(Instance),
}

impl Step {
    /// The step `op` is, with `tag` its operator selector where the opcode takes one.
    pub(crate) fn of(op: Op, tag: u32) -> Option<Self> {
        WasmOperation::of(op, tag)
            .map(Self::Numeric)
            .or_else(|| Instance::of(op).map(Self::Instance))
    }
}
