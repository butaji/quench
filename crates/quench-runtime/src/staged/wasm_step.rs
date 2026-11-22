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
pub enum Step {
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
pub enum Family {
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

/// The opcode lists the lane fusions in `vm::dispatch_fast` match by name, each keyed by what it
/// is for. `wasm_lane_list!($m => $key)` applies `$m!` to the list's opcodes;
/// `wasm_lane_list!($m ($pre) ($post) => $key)` applies it to `$pre`, the opcodes, then `$post`,
/// for a consumer that takes leading or trailing arguments. Each expansion is the token list the
/// consumer's own spelled list gave, so the fusions generated from them are unchanged.
#[allow(unused_macros)]
macro_rules! wasm_lane_list {
    ($m:ident => $key:ident) => {
        $crate::staged::wasm_step::wasm_lane_list!(@emit $m () () $key)
    };
    ($m:ident ($($pre:tt)*) ($($post:tt)*) => $key:ident) => {
        $crate::staged::wasm_step::wasm_lane_list!(@emit $m ($($pre)*) ($($post)*) $key)
    };
    (@emit $m:ident ($($pre:tt)*) ($($post:tt)*) load_store_copies) => {
        $m!($($pre)* (WasmI32Load, WasmI32Store), (WasmI32Load8U, WasmI32Store8), (WasmI64Load, WasmI64Store) $($post)*)
    };
    (@emit $m:ident ($($pre:tt)*) ($($post:tt)*) i32_loads) => {
        $m!($($pre)* WasmI32Load, WasmI32Load8S, WasmI32Load8U, WasmI32Load16S, WasmI32Load16U $($post)*)
    };
    (@emit $m:ident ($($pre:tt)*) ($($post:tt)*) scalar_loads) => {
        $m!($($pre)* WasmI32Load, WasmI64Load, WasmF32Load, WasmF64Load, WasmI32Load8S, WasmI32Load8U, WasmI32Load16S, WasmI32Load16U, WasmI64Load8S, WasmI64Load8U, WasmI64Load16S, WasmI64Load16U, WasmI64Load32S, WasmI64Load32U $($post)*)
    };
    (@emit $m:ident ($($pre:tt)*) ($($post:tt)*) load_operator_pairs) => {
        $m!($($pre)* (WasmI32Load, WasmI32Add), (WasmI32Load, WasmI32Multiply), (WasmI32Load16S, WasmI32Multiply), (WasmI32Load16U, WasmI32Multiply), (WasmI32Load8U, WasmI32Add) $($post)*)
    };
    (@emit $m:ident ($($pre:tt)*) ($($post:tt)*) immediate_equal_jumps) => {
        $m!($($pre)* WasmJumpI32EqualImmediate, WasmJumpI32NotEqualImmediate, WasmJumpI32LessUnsignedImmediate, WasmJumpI32LessEqualUnsignedImmediate, WasmJumpI32GreaterUnsignedImmediate, WasmJumpI32GreaterEqualUnsignedImmediate $($post)*)
    };
    (@emit $m:ident ($($pre:tt)*) ($($post:tt)*) register_equal_jumps) => {
        $m!($($pre)* WasmJumpI32Equal, WasmJumpI32NotEqual $($post)*)
    };
    (@emit $m:ident ($($pre:tt)*) ($($post:tt)*) bit_field_seconds) => {
        $m!($($pre)* WasmI32ShiftRightUnsignedAndImmediate, WasmI32Multiply, WasmI32XorImmediate $($post)*)
    };
    (@emit $m:ident ($($pre:tt)*) ($($post:tt)*) operator_constant_selects) => {
        $m!($($pre)* WasmI32GreaterSigned, WasmI32GreaterUnsigned, WasmI32LessSigned, WasmI32LessUnsigned, WasmI32Equal, WasmI32NotEqual, WasmI32GreaterEqualSigned, WasmI32LessEqualSigned $($post)*)
    };
    (@emit $m:ident ($($pre:tt)*) ($($post:tt)*) i32_stores) => {
        $m!($($pre)* WasmI32Store, WasmI32Store8, WasmI32Store16 $($post)*)
    };
    (@emit $m:ident ($($pre:tt)*) ($($post:tt)*) scalar_stores) => {
        $m!($($pre)* WasmI32Store, WasmI64Store, WasmF32Store, WasmF64Store, WasmI32Store8, WasmI32Store16, WasmI64Store8, WasmI64Store16, WasmI64Store32 $($post)*)
    };
    (@emit $m:ident ($($pre:tt)*) ($($post:tt)*) i32_updates) => {
        $m!($($pre)* WasmI32Add, WasmI32Subtract, WasmI32Multiply, WasmI32And, WasmI32Or, WasmI32Xor, WasmI32ShiftLeft, WasmI32ShiftRightSigned, WasmI32ShiftRightUnsigned, WasmI32Equal, WasmI32NotEqual, WasmI32LessSigned, WasmI32LessUnsigned $($post)*)
    };
    (@emit $m:ident ($($pre:tt)*) ($($post:tt)*) i32_load) => {
        $m!($($pre)* WasmI32Load $($post)*)
    };
    (@emit $m:ident ($($pre:tt)*) ($($post:tt)*) i32_load_u8) => {
        $m!($($pre)* WasmI32Load8U $($post)*)
    };
    (@emit $m:ident ($($pre:tt)*) ($($post:tt)*) equal_immediate_jump) => {
        $m!($($pre)* WasmJumpI32EqualImmediate $($post)*)
    };
    (@emit $m:ident ($($pre:tt)*) ($($post:tt)*) not_equal_immediate_jump) => {
        $m!($($pre)* WasmJumpI32NotEqualImmediate $($post)*)
    };
    (@emit $m:ident ($($pre:tt)*) ($($post:tt)*) i32_add) => {
        $m!($($pre)* WasmI32Add $($post)*)
    };
    (@emit $m:ident ($($pre:tt)*) ($($post:tt)*) i32_add_immediate) => {
        $m!($($pre)* WasmI32AddImmediate $($post)*)
    };
    (@emit $m:ident ($($pre:tt)*) ($($post:tt)*) i32_add_immediate_jump_non_zero) => {
        $m!($($pre)* WasmI32AddImmediateJumpNonZero $($post)*)
    };
    (@emit $m:ident ($($pre:tt)*) ($($post:tt)*) i32_subtract_immediate) => {
        $m!($($pre)* WasmI32SubtractImmediate $($post)*)
    };
    (@emit $m:ident ($($pre:tt)*) ($($post:tt)*) i32_and_immediate) => {
        $m!($($pre)* WasmI32AndImmediate $($post)*)
    };
}
#[allow(unused_imports)]
pub(crate) use wasm_lane_list;

/// A Wasm tier: what it makes of each family of steps, in its own terms (`Out`: the u-jit's
/// fall-through flag, the b-jit's rows). `dispatch` is the one match over `Step` the tiers run,
/// so a family added to `Step` is a method every tier must answer, and no tier keeps its own
/// match over the families.
pub trait WasmTier {
    type Out;
    fn numeric(&mut self, pc: usize, instruction: WideInstruction, operation: WasmOperation) -> Self::Out;
    fn instance(&mut self, pc: usize, instruction: WideInstruction, step: Instance) -> Self::Out;
    fn memory(&mut self, pc: usize, instruction: WideInstruction, step: Memory) -> Self::Out;
    fn flow(&mut self, pc: usize, instruction: WideInstruction, flow: Flow) -> Self::Out;
    fn call(&mut self, pc: usize, instruction: WideInstruction, call: Call) -> Self::Out;
    fn table(&mut self, pc: usize, instruction: WideInstruction, op: TableOp) -> Self::Out;
    fn simd(&mut self, pc: usize, instruction: WideInstruction, step: Simd) -> Self::Out;
    fn gc(&mut self, pc: usize, instruction: WideInstruction, op: GcOp) -> Self::Out;
    fn exception(&mut self, pc: usize, instruction: WideInstruction, op: Exception) -> Self::Out;
    fn atomic(&mut self, pc: usize, instruction: WideInstruction, op: Atomic) -> Self::Out;
}

/// The tier's answer for `step`, the step of `instruction` at `pc`.
#[inline(always)]
pub fn dispatch<T: WasmTier>(tier: &mut T, pc: usize, instruction: WideInstruction, step: Step) -> T::Out {
    match step {
        Step::Numeric(operation) => tier.numeric(pc, instruction, operation),
        Step::Instance(step) => tier.instance(pc, instruction, step),
        Step::Memory(step) => tier.memory(pc, instruction, step),
        Step::Flow(flow) => tier.flow(pc, instruction, flow),
        Step::Call(call) => tier.call(pc, instruction, call),
        Step::Table(op) => tier.table(pc, instruction, op),
        Step::Simd(step) => tier.simd(pc, instruction, step),
        Step::Gc(op) => tier.gc(pc, instruction, op),
        Step::Exception(op) => tier.exception(pc, instruction, op),
        Step::Atomic(op) => tier.atomic(pc, instruction, op),
    }
}
