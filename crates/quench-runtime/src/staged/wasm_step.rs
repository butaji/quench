//! What a Wasm function's instruction is, as data: the one table every tier reads (the
//! analysis, the emitter, the loop and threading passes). No tier names a Wasm opcode.

use super::wasm_atomic::Atomic;
use super::wasm_call::Call;
use super::wasm_exception::Exception;
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
    /// Exception construction, the catch's tag test and payload read, and `throw_ref`.
    Exception(Exception),
    /// The atomic fence and the atomic accesses: read-modify-write, compare-exchange, wait and
    /// notify.
    Atomic(Atomic),
}

impl Step {
    /// The step `op` is, with `tag` its operator selector where the opcode takes one.
    pub(crate) const fn of(op: Op, tag: u32) -> Option<Self> {
        if let Some(step) = WasmOperation::of(op, tag) {
            return Some(Self::Numeric(step));
        }
        if let Some(step) = Instance::of(op) {
            return Some(Self::Instance(step));
        }
        if let Some(step) = Memory::of(op) {
            return Some(Self::Memory(step));
        }
        if let Some(step) = Flow::of(op) {
            return Some(Self::Flow(step));
        }
        if let Some(step) = Call::of(op) {
            return Some(Self::Call(step));
        }
        if let Some(step) = TableOp::of(op) {
            return Some(Self::Table(step));
        }
        if let Some(step) = Simd::of(op, tag) {
            return Some(Self::Simd(step));
        }
        if let Some(step) = GcOp::of(op) {
            return Some(Self::Gc(step));
        }
        if let Some(step) = Exception::of(op) {
            return Some(Self::Exception(step));
        }
        match Atomic::of(op) {
            Some(atomic) => Some(Self::Atomic(atomic)),
            None => None,
        }
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
