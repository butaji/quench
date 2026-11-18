//! What a Wasm function's instruction is, as data: the one table every tier reads (the
//! analysis, the emitter, the loop and threading passes). No tier names a Wasm opcode.

use super::wasm_call::Call;
use super::wasm_flow::Flow;
use super::wasm_gc::GcOp;
use super::wasm_instance::Instance;
use super::wasm_memory::Memory;
use super::wasm_operation::WasmOperation;
use super::wasm_simd::Simd;
use super::wasm_table_ops::TableOp;
use crate::bytecode::{InstructionField, Op, WideInstruction};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Step {
    /// Integer, float and conversion arithmetic, and the jumps on it.
    Numeric(WasmOperation),
    /// Registers, constants, instance bindings and globals.
    Instance(Instance),
    /// Linear-memory loads, stores, size and bulk operations.
    Memory(Memory),
    /// Jumps, tables, selects, traps and returns.
    Flow(Flow),
    /// Direct and indirect calls and the references they call.
    Call(Call),
    /// Table, element-segment and data-segment operations, and `memory.init`.
    Table(TableOp),
    /// Vector operators, shuffles and vector memory accesses.
    Simd(Simd),
    /// Struct and array accesses, reference tests and casts, and the GC allocations.
    Gc(GcOp),
}

impl Step {
    /// The step `op` is, with `tag` its operator selector where the opcode takes one.
    pub(crate) fn of(op: Op, tag: u32) -> Option<Self> {
        WasmOperation::of(op, tag)
            .map(Self::Numeric)
            .or_else(|| Instance::of(op).map(Self::Instance))
            .or_else(|| Memory::of(op).map(Self::Memory))
            .or_else(|| Flow::of(op).map(Self::Flow))
            .or_else(|| Call::of(op).map(Self::Call))
            .or_else(|| TableOp::of(op).map(Self::Table))
            .or_else(|| Simd::of(op, tag).map(Self::Simd))
            .or_else(|| GcOp::of(op).map(Self::Gc))
    }

    /// The register whose slot the plan records for the step: its first operand, or a vector
    /// access's address.
    pub(crate) fn operand(self, instruction: WideInstruction) -> u16 {
        match self {
            Self::Simd(simd) => simd.operand(instruction),
            Self::Table(table) => table.bindings(instruction).0.unwrap_or(instruction.b()),
            _ => instruction.b(),
        }
    }

    /// Whether `field` of the instruction is an operand slot nothing reads.
    pub(crate) fn leaves_unread(self, field: InstructionField) -> bool {
        matches!(self, Self::Simd(simd) if simd.leaves_unread(field))
    }

    /// The function a step calls statically.
    pub(crate) fn callee(self, instruction: WideInstruction) -> Option<usize> {
        match self {
            Self::Call(call) => call.callee(instruction),
            _ => None,
        }
    }

    /// The register of the second binding a table copy or segment init names, if any.
    pub(crate) fn second_binding(self, instruction: WideInstruction) -> Option<u16> {
        match self {
            Self::Table(table) => table.bindings(instruction).1,
            _ => None,
        }
    }

    /// Whether the step returns from the function.
    pub(crate) const fn returns(self) -> bool {
        matches!(self, Self::Flow(Flow::Return))
    }

    /// Whether the step reads the function's memory register, which no other step may.
    pub(crate) const fn reads_memory(self) -> bool {
        matches!(self, Self::Memory(_) | Self::Instance(Instance::Binding))
            || matches!(self, Self::Table(TableOp::MemoryInit))
            || matches!(self, Self::Simd(simd) if simd.reads_memory())
    }
}
