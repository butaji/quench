//! Emit functions for control flow, locals, calls, arrays, returns, exceptions and register moves.

use super::Emission;
use crate::bytecode::{ImmediateLayout, Op, Operand, Register};
use crate::wasm::integer::I32BinaryOperator;

/// An unconditional jump whose target is patched when its label is bound.
pub(crate) fn jump() -> Emission {
    Emission::new(Op::Jump, 0, 0, 0, 0)
}

/// A jump taken when the i32 register `value` is zero.
pub(crate) fn jump_if_zero_i32(value: Register) -> Emission {
    Emission::new(Op::WasmJumpI32Zero, value, 0, 0, 0)
}

/// A jump taken when the i32 register `value` is nonzero.
pub(crate) fn jump_if_nonzero_i32(value: Register) -> Emission {
    Emission::new(Op::WasmJumpI32NonZero, value, 0, 0, 0)
}

/// A fused comparison of two registers that jumps when it holds.
#[cfg(not(stencil_build))]
pub(crate) fn compare_and_jump(
    comparison: I32BinaryOperator,
    left: Register,
    right: Register,
) -> Emission {
    let op = comparison.jump_op().expect("fusable comparison has a jump");
    Emission::new(op, left, right, 0, 0)
}

/// A fused comparison of a register with a constant that jumps when it holds.
#[cfg(not(stencil_build))]
pub(crate) fn compare_immediate_and_jump(
    comparison: I32BinaryOperator,
    left: Register,
    constant: Register,
) -> Emission {
    let op = comparison
        .immediate_jump_op()
        .expect("fusable comparison has a constant jump");
    Emission::new(op, left, constant, 0, 0)
}

/// A counted loop's step `value = source + addend`, absorbed into its back edge.
pub(crate) fn add_immediate_jump_if_nonzero(
    value: Register,
    source: Register,
    addend: Register,
) -> Emission {
    Emission::new(Op::WasmI32AddImmediateJumpNonZero, value, source, addend, 0)
}

/// A jump table over `cases` entries, followed by one jump per entry and the default.
pub(crate) fn branch_table(index: Register, cases: u32) -> Emission {
    Emission::new(Op::WasmBranchTable, index, 0, 0, cases)
}

/// Throws the exception object held in `reference`.
pub(crate) fn throw_ref(reference: Register) -> Emission {
    Emission::new(Op::WasmThrowRef, reference, 0, 0, 0)
}

/// Traps: the lowered code is unreachable.
pub(crate) fn unreachable() -> Emission {
    Emission::new(Op::WasmUnreachable, 0, 0, 0, 0)
}

/// `result = condition ? left : right`.
pub fn select(
    result: Register,
    left: Register,
    right: Register,
    condition: Register,
) -> Emission {
    Emission::new(
        Op::WasmSelect,
        result,
        left,
        right,
        ImmediateLayout::register_pair_immediate(condition, condition),
    )
}

/// `destination = local[slot]`.
pub(crate) fn load_local(destination: Register, slot: u16) -> Emission {
    Emission::new(Op::LoadLocal, destination, 0, 0, u32::from(slot))
}

/// `local[slot] = value`.
pub(crate) fn store_local(value: Register, slot: u16) -> Emission {
    Emission::new(Op::StoreLocal, value, 0, 0, u32::from(slot))
}

/// `destination = source`.
pub(crate) fn move_register(destination: Register, source: Register) -> Emission {
    Emission::new(Op::Move, destination, source, 0, 0)
}

/// `destination = constants[constant]`.
pub(crate) fn load_constant(destination: Register, constant: u32) -> Emission {
    Emission::new(Op::LoadConst, destination, 0, 0, constant)
}

/// `destination = array` of `count` elements, filled by `define_array_element`.
pub(crate) fn make_array(destination: Register, count: u16) -> Emission {
    Emission::new(Op::MakeArray, destination, 0, 0, u32::from(count))
}

/// `array[index] = element`.
pub(crate) fn define_array_element(element: Register, array: Register, index: u16) -> Emission {
    Emission::new(Op::DefineArrayElement, element, array, 0, u32::from(index))
}

/// Returns `value` from the function.
pub(crate) fn return_value(value: Register) -> Emission {
    Emission::new(Op::Return, value, 0, 0, 0)
}

/// Builds an exception object at `base` from the `count` values after the tag.
pub(crate) fn exception_new(base: Register, count: u16) -> Emission {
    Emission::new(Op::WasmExceptionNew, base, base, count, 0)
}

/// `condition = exception matches tag`.
pub(crate) fn exception_match(condition: Register, exception: Register, tag: Register) -> Emission {
    Emission::new(Op::WasmExceptionMatch, condition, exception, tag, 0)
}

/// `destination = payload[offset]` of `exception`, whose value has type `ty` (field c), so a
/// tier can type the register without the tag.
pub(crate) fn exception_payload(
    destination: Register,
    exception: Register,
    offset: u16,
    ty: crate::staged::wasm_exception::PayloadType,
) -> Emission {
    Emission::new(
        Op::WasmExceptionPayload,
        destination,
        exception,
        ty.code(),
        u32::from(offset),
    )
}

/// Fills `count` registers from `start` with the default value constant `value`.
pub(crate) fn fill_registers(start: Register, count: Register, value: u32) -> Emission {
    Emission::new(Op::WasmFillRegisters, 0, start, count, value)
}

/// `destination = call of the known function callee`, its argument window `immediate`.
pub(crate) fn call_known(destination: Register, callee: Register, immediate: u32) -> Emission {
    Emission::new(Op::CallKnown, destination, callee, 0, immediate)
}

/// `destination = call of the function reference`, its receiver the void result in `receiver`.
pub(crate) fn call_through(
    destination: Register,
    reference: Register,
    receiver: Register,
    immediate: u32,
) -> Emission {
    Emission::new(Op::Call, destination, reference, receiver, immediate)
}

/// `destination = array[index]`, the element of the bundle `array` at the index register `index`.
pub(crate) fn get_index(destination: Register, array: Register, index: Register) -> Emission {
    Emission::new(
        Op::GetIndex,
        destination,
        Operand::register(array).0,
        Operand::register(index).0,
        0,
    )
}
