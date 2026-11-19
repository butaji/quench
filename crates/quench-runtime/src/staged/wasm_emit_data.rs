//! Emit functions for the Wasm lowering's reference, table, constant, memory and atomic
//! operations, and register copies. Each one names the meaning of an instruction (a type test,
//! a table store, a constant load) and returns the instruction it lowers to as an `Emission`:
//! the same opcode and operands the lowering built before, so the bytecode is unchanged. The
//! lowering (`wasm/reference.rs`, `wasm/table.rs`, `wasm/memory.rs`, `wasm/atomic.rs`) calls
//! these and names no opcode itself.

use crate::bytecode::{ImmediateLayout, Op, Register};

/// One instruction a lowering emits: its opcode, its three register operands and its immediate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Emission {
    pub(crate) op: Op,
    pub(crate) a: Register,
    pub(crate) b: Register,
    pub(crate) c: Register,
    pub(crate) imm: u32,
}

impl Emission {
    const fn new(op: Op, a: Register, b: Register, c: Register, imm: u32) -> Self {
        Self { op, a, b, c, imm }
    }
}

/// `result = conversion(input)`, an externref internalize or externalize, `conversion` the
/// lowering's selector.
pub(crate) fn external_conversion(result: Register, input: Register, conversion: u32) -> Emission {
    Emission::new(Op::WasmExternalConversion, result, input, 0, conversion)
}

/// `result = left === right` on references.
pub(crate) fn ref_eq(result: Register, left: Register, right: Register) -> Emission {
    Emission::new(Op::WasmRefEq, result, left, right, 0)
}

/// `result = reference is target`, a plain test, or a descriptor-matched test where
/// `descriptor` names the descriptor register.
pub(crate) fn ref_test(
    result: Register,
    reference: Register,
    descriptor: Option<Register>,
    target: u32,
) -> Emission {
    match descriptor {
        Some(descriptor) => Emission::new(Op::WasmDescriptorTest, result, reference, descriptor, target),
        None => Emission::new(Op::WasmRefTest, result, reference, 0, target),
    }
}

/// `result = input cast to target`, a plain cast, or a descriptor-matched cast where
/// `descriptor` names the descriptor register.
pub(crate) fn ref_cast(
    result: Register,
    input: Register,
    descriptor: Option<Register>,
    target: u32,
) -> Emission {
    match descriptor {
        Some(descriptor) => Emission::new(Op::WasmDescriptorCast, result, input, descriptor, target),
        None => Emission::new(Op::WasmRefCast, result, input, 0, target),
    }
}

/// `result = operator(input)` on an i32, `operator` the lowering's `I32UnaryOperator` tag.
pub(crate) fn i32_unary(result: Register, input: Register, operator: u32) -> Emission {
    Emission::new(Op::WasmI32Unary, result, input, 0, operator)
}

/// `result = input` where `input` is non-null, trapping with the `check` diagnostic otherwise.
pub(crate) fn ref_as_non_null(result: Register, input: Register, check: u32) -> Emission {
    Emission::new(Op::WasmRefAsNonNull, result, input, 0, check)
}

/// `result = input is null`.
pub(crate) fn ref_is_null(result: Register, input: Register) -> Emission {
    Emission::new(Op::WasmRefIsNull, result, input, 0, 0)
}

/// `target` is the reference an indirect call of `signature` reads from `table` at the index
/// `target` holds, in place: the result and the index are the same register.
pub(crate) fn indirect_target(target: Register, table: Register, signature: u32) -> Emission {
    Emission::new(Op::WasmIndirectTarget, target, table, target, signature)
}

/// `destination = constant`, `constant` the index the lowering appended to the constant pool.
pub(crate) fn load_constant(destination: Register, constant: u32) -> Emission {
    Emission::new(Op::LoadConst, destination, 0, 0, constant)
}

/// `result = the function reference of function_index`.
pub(crate) fn ref_func(result: Register, function_index: u32) -> Emission {
    Emission::new(Op::WasmRefFunc, result, 0, 0, function_index)
}

/// `result = table[index]`, `table` the table's binding register.
pub(crate) fn table_get(result: Register, table: Register, index: Register) -> Emission {
    Emission::new(Op::WasmTableGet, result, table, index, 0)
}

/// `table[index] = value`, `table` the table's binding register.
pub(crate) fn table_set(value: Register, table: Register, index: Register) -> Emission {
    Emission::new(Op::WasmTableSet, value, table, index, 0)
}

/// `result = the size of table`.
pub(crate) fn table_size(result: Register, table: Register) -> Emission {
    Emission::new(Op::WasmTableSize, result, table, 0, 0)
}

/// `result = table.grow(delta, initial)`, `delta` the growth register.
pub(crate) fn table_grow(result: Register, table: Register, initial: Register, delta: Register) -> Emission {
    Emission::new(
        Op::WasmTableGrow,
        result,
        table,
        initial,
        ImmediateLayout::register_pair_immediate(delta, delta),
    )
}

/// `table[output .. output + length] = value`, a fill of `length` elements from `output`.
pub(crate) fn table_fill(table: Register, value: Register, length: Register, output: Register) -> Emission {
    Emission::new(
        Op::WasmTableFill,
        table,
        value,
        length,
        ImmediateLayout::register_pair_immediate(output, output),
    )
}

/// `table[output .. output + length] = source[input ..]`, a copy of `length` elements from
/// `source` at `input` into `table` at `output`.
pub(crate) fn table_copy(
    table: Register,
    source: Register,
    length: Register,
    output: Register,
    input: Register,
) -> Emission {
    Emission::new(
        Op::WasmTableCopy,
        table,
        source,
        length,
        ImmediateLayout::register_pair_immediate(output, input),
    )
}

/// `destination = the instance binding of slot`, the register a memory or data segment is read
/// through.
pub(crate) fn instance_binding(destination: Register, slot: u32) -> Emission {
    Emission::new(Op::WasmInstanceBinding, destination, 0, 0, slot)
}

/// `effective = address + offset`, a memory32 effective address on `memory`, `offset` the
/// constant the lowering appended to the constant pool.
pub(crate) fn memory_address(effective: Register, address: Register, memory: Register, offset: u32) -> Emission {
    Emission::new(Op::WasmMemoryAddress, effective, address, memory, offset)
}

/// `result = memory[selector] at effective`, a scalar or vector load from `memory`.
pub(crate) fn memory_load(result: Register, memory: Register, effective: Register, selector: u32) -> Emission {
    Emission::new(Op::WasmMemoryLoad, result, memory, effective, selector)
}

/// `memory[selector] at effective = value`, a scalar or vector store to `memory`.
pub(crate) fn memory_store(value: Register, memory: Register, effective: Register, selector: u32) -> Emission {
    Emission::new(Op::WasmMemoryStore, value, memory, effective, selector)
}

/// `result = vector` with lane `selector` replaced by `scalar`.
pub(crate) fn lane_replace(result: Register, vector: Register, scalar: Register, selector: u32) -> Emission {
    Emission::new(Op::WasmSimd, result, vector, scalar, selector)
}

/// `scalar = the lane selector of vector`.
pub(crate) fn lane_extract(scalar: Register, vector: Register, selector: u32) -> Emission {
    Emission::new(Op::WasmSimd, scalar, vector, 0, selector)
}

/// `result = memory.grow(delta)`.
pub(crate) fn memory_grow(result: Register, memory: Register, delta: Register) -> Emission {
    Emission::new(Op::WasmMemoryGrow, result, memory, delta, 0)
}

/// `result = the size of memory`.
pub(crate) fn memory_size(result: Register, memory: Register) -> Emission {
    Emission::new(Op::WasmMemorySize, result, memory, 0, 0)
}

/// `memory[output ..] = source[input ..]` over `length` bytes, the source a data segment's
/// instance binding.
pub(crate) fn memory_init(memory: Register, source: Register, length: Register, output: Register, input: Register) -> Emission {
    Emission::new(
        Op::WasmMemoryInit,
        memory,
        source,
        length,
        ImmediateLayout::register_pair_immediate(output, input),
    )
}

/// `memory[output ..] = source[input ..]` over `length` bytes, a copy between memories.
pub(crate) fn memory_copy(memory: Register, source: Register, length: Register, output: Register, input: Register) -> Emission {
    Emission::new(
        Op::WasmMemoryCopy,
        memory,
        source,
        length,
        ImmediateLayout::register_pair_immediate(output, input),
    )
}

/// `memory[output .. output + length] = value`, a fill of `memory`.
pub(crate) fn memory_fill(memory: Register, value: Register, length: Register, output: Register) -> Emission {
    Emission::new(
        Op::WasmMemoryFill,
        memory,
        value,
        length,
        ImmediateLayout::register_pair_immediate(output, output),
    )
}

/// A sequentially consistent fence.
pub(crate) fn atomic_fence() -> Emission {
    Emission::new(Op::WasmAtomicFence, 0, 0, 0, 0)
}

/// `result = the atomic operator (operator) on memory`, its window `inputs` registers wide.
pub(crate) fn atomic_access(result: Register, memory: Register, inputs: Register, operator: u32) -> Emission {
    Emission::new(Op::WasmAtomicAccess, result, memory, inputs, operator)
}

/// `destination = source`, a register copy.
pub(crate) fn register_copy(destination: Register, source: Register) -> Emission {
    Emission::new(Op::Move, destination, source, 0, 0)
}
