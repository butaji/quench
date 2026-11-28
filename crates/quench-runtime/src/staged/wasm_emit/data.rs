//! Emit functions for the Wasm lowering's reference, i31, table, segment, global, memory and
//! atomic operations. Each one names the meaning of an instruction (a type test, a table store, a
//! memory access) and returns the `Emission` it lowers to: the same opcode and operands the
//! lowering built before, so the bytecode is unchanged.

use super::Emission;
use crate::bytecode::{ImmediateLayout, Op, Register};
use crate::wasm::i31::I31Operator;
#[cfg(not(stencil_build))]
#[allow(unused_imports)]
use crate::staged::glue_prelude::*;

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
        Some(descriptor) => Emission::new(
            Op::WasmDescriptorTest,
            result,
            reference,
            descriptor,
            target,
        ),
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
        Some(descriptor) => {
            Emission::new(Op::WasmDescriptorCast, result, input, descriptor, target)
        }
        None => Emission::new(Op::WasmRefCast, result, input, 0, target),
    }
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
pub(crate) fn table_grow(
    result: Register,
    table: Register,
    initial: Register,
    delta: Register,
) -> Emission {
    Emission::new(
        Op::WasmTableGrow,
        result,
        table,
        initial,
        ImmediateLayout::register_pair_immediate(delta, delta),
    )
}

/// `table[output .. output + length] = value`, a fill of `length` elements from `output`.
pub(crate) fn table_fill(
    table: Register,
    value: Register,
    length: Register,
    output: Register,
) -> Emission {
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

/// `destination = global`, the global whose binding register is `binding`.
pub(crate) fn global_get(destination: Register, binding: Register) -> Emission {
    Emission::new(Op::WasmGlobalGet, destination, binding, 0, 0)
}

/// `global = value`, the global whose binding register is `binding`.
pub(crate) fn global_set(value: Register, binding: Register) -> Emission {
    Emission::new(Op::WasmGlobalSet, value, binding, 0, 0)
}

/// `result = operator(input)` on an i31 reference, the operator its selector.
pub(crate) fn i31_operation(result: Register, input: Register, operator: I31Operator) -> Emission {
    Emission::new(Op::WasmI31, result, input, 0, operator as u32)
}

/// `effective = address + offset`, a memory32 effective address on `memory`, `offset` the
/// constant the lowering appended to the constant pool.
pub(crate) fn memory_address(
    effective: Register,
    address: Register,
    memory: Register,
    offset: u32,
) -> Emission {
    Emission::new(Op::WasmMemoryAddress, effective, address, memory, offset)
}

/// `result = memory[selector] at effective`, a scalar or vector load from `memory`.
pub(crate) fn memory_load(
    result: Register,
    memory: Register,
    effective: Register,
    selector: u32,
) -> Emission {
    Emission::new(Op::WasmMemoryLoad, result, memory, effective, selector)
}

/// `memory[selector] at effective = value`, a scalar or vector store to `memory`.
pub(crate) fn memory_store(
    value: Register,
    memory: Register,
    effective: Register,
    selector: u32,
) -> Emission {
    Emission::new(Op::WasmMemoryStore, value, memory, effective, selector)
}

/// `result = vector` with lane `selector` replaced by `scalar`.
pub(crate) fn lane_replace(
    result: Register,
    vector: Register,
    scalar: Register,
    selector: u32,
) -> Emission {
    Emission::new(Op::WasmSimd, result, vector, scalar, selector)
}

/// `scalar = the lane selector of vector`.
pub(crate) fn lane_extract(scalar: Register, vector: Register, selector: u32) -> Emission {
    Emission::new(Op::WasmSimd, scalar, vector, 0, selector)
}

/// `result = left op right` for the SIMD operator `selector` names (lane in its high bits).
pub(crate) fn simd(result: Register, left: Register, right: Register, selector: u32) -> Emission {
    Emission::new(Op::WasmSimd, result, left, right, selector)
}

/// `result = shuffle(left, right)` by the 16 lane indices in constant slot `constant`.
pub(crate) fn simd_shuffle(result: Register, left: Register, right: Register, constant: u32) -> Emission {
    Emission::new(Op::WasmSimdShuffle, result, left, right, constant)
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
pub(crate) fn memory_init(
    memory: Register,
    source: Register,
    length: Register,
    output: Register,
    input: Register,
) -> Emission {
    Emission::new(
        Op::WasmMemoryInit,
        memory,
        source,
        length,
        ImmediateLayout::register_pair_immediate(output, input),
    )
}

/// `memory[output ..] = source[input ..]` over `length` bytes, a copy between memories.
pub(crate) fn memory_copy(
    memory: Register,
    source: Register,
    length: Register,
    output: Register,
    input: Register,
) -> Emission {
    Emission::new(
        Op::WasmMemoryCopy,
        memory,
        source,
        length,
        ImmediateLayout::register_pair_immediate(output, input),
    )
}

/// `memory[output .. output + length] = value`, a fill of `memory`.
pub(crate) fn memory_fill(
    memory: Register,
    value: Register,
    length: Register,
    output: Register,
) -> Emission {
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
pub(crate) fn atomic_access(
    result: Register,
    memory: Register,
    inputs: Register,
    operator: u32,
) -> Emission {
    Emission::new(Op::WasmAtomicAccess, result, memory, inputs, operator)
}

/// Drops the instance binding of element or data segment `slot`.
pub(crate) fn segment_drop(slot: u16) -> Emission {
    Emission::new(Op::WasmSegmentDrop, 0, 0, 0, u32::from(slot))
}

/// Copies `length` items of element segment `elements` into `table` at `output`, from `input`.
pub(crate) fn table_init(
    table: Register,
    elements: Register,
    length: Register,
    output: Register,
    input: Register,
) -> Emission {
    Emission::new(
        Op::WasmTableInit,
        table,
        elements,
        length,
        ImmediateLayout::register_pair_immediate(output, input),
    )
}
