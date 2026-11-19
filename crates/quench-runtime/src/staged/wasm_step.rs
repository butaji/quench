//! What a Wasm function's instruction is, as data: the one table every tier reads (the
//! analysis, the emitter, the loop and threading passes). No tier names a Wasm opcode.

use super::wasm_atomic::Atomic;
use super::wasm_call::Call;
use super::wasm_exception::Exception;
use super::wasm_flow::Flow;
use super::wasm_gc::GcOp;
use super::wasm_instance::{Instance, local_register};
use super::wasm_memory::Memory;
use super::wasm_operation::WasmOperation;
use super::wasm_simd::Simd;
use super::wasm_table_ops::TableOp;
use crate::bytecode::{Class, InstructionField, Op, WideInstruction};

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

    /// The register whose slot the plan records for the step: its first operand, a vector
    /// access's address, or the frame local a load reads (a local is a register past the
    /// `registers` of the frame, `local_register`).
    pub(crate) fn operand(self, instruction: WideInstruction, registers: u16) -> u16 {
        match self {
            Self::Simd(simd) => simd.operand(instruction),
            Self::Table(table) => table.bindings(instruction).0.unwrap_or(instruction.b()),
            Self::Instance(Instance::LoadLocal) => local_register(registers, instruction.imm()),
            Self::Instance(Instance::StoreLocal) => instruction.a(),
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

/// The families whose lane handler an opcode selects by the operation alone, not by its
/// selector: a memory access, scalar or vector, and a float operator by its width and whether
/// it runs over raw registers. The handler bodies still read the tag at run time.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Family {
    /// A linear-memory load, scalar or vector.
    MemoryLoad,
    /// A linear-memory store, scalar or vector.
    MemoryStore,
    /// An f64 binary operator, over boxed registers or raw words.
    FloatBinary64 { raw: bool },
    /// An f32 binary operator.
    FloatBinary32,
    /// An f64 unary operator, over boxed registers or raw words.
    FloatUnary64 { raw: bool },
}

impl Family {
    /// The family `op` is, if it is one of these.
    pub(crate) const fn of(op: Op) -> Option<Self> {
        match op.class() {
            Class::WasmMemoryLoad => Some(Self::MemoryLoad),
            Class::WasmMemoryStore => Some(Self::MemoryStore),
            Class::WasmFloatBinary64 => Some(Self::FloatBinary64 { raw: false }),
            Class::WasmFloatBinary64Raw => Some(Self::FloatBinary64 { raw: true }),
            Class::WasmFloatBinary32 => Some(Self::FloatBinary32),
            Class::WasmFloatUnary64 => Some(Self::FloatUnary64 { raw: false }),
            Class::WasmFloatUnary64Raw => Some(Self::FloatUnary64 { raw: true }),
            _ => None,
        }
    }
}

/// The opcodes the lane fusions match by name, where no operator or predicate already gives
/// them (`I32BinaryOperator`, `Op::is_wasm_*`).
pub(crate) mod opcodes {
    use crate::bytecode::Op;

    pub(crate) const I32_ADD: Op = Op::WasmI32Add;
    pub(crate) const I32_ADD_IMMEDIATE: Op = Op::WasmI32AddImmediate;
    pub(crate) const I32_ADD_IMMEDIATE_JUMP_NON_ZERO: Op = Op::WasmI32AddImmediateJumpNonZero;
    pub(crate) const I32_AND_IMMEDIATE: Op = Op::WasmI32AndImmediate;
    pub(crate) const I32_LOAD: Op = Op::WasmI32Load;
    pub(crate) const I32_STORE: Op = Op::WasmI32Store;
    pub(crate) const F64_LOAD: Op = Op::WasmF64Load;
    pub(crate) const F64_STORE: Op = Op::WasmF64Store;
    pub(crate) const SELECT: Op = Op::WasmSelect;
    pub(crate) const BRANCH_TABLE: Op = Op::WasmBranchTable;
    pub(crate) const SHIFT_LEFT_IMMEDIATE: Op = Op::WasmI32ShiftLeftImmediate;
    pub(crate) const SHIFT_RIGHT_UNSIGNED_AND_IMMEDIATE: Op = Op::WasmI32ShiftRightUnsignedAndImmediate;
}


