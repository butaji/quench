//! `Unary` records answered in the lane where the conversion runs no guest code.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

/// A `Unary` record's value where it runs no guest code and throws nothing: logical not and
/// `typeof` of any value, and every other operator of a Number, String, Boolean, null or
/// undefined, whose conversions are pure.
///
/// # Safety
/// `cx` is the running lane's context and `r` its registers.
#[inline(always)]
pub(crate) unsafe fn unary_value(
    cx: *mut LaneContext,
    r: Registers,
    i: &LaneInstruction,
) -> Option<Value> {
    unsafe { unary_values(cx, i.imm(), r.get(i.b)) }
}

/// `unary_value` of `operator` over `operand` (the lane frame's answer to a row's `Unary`
/// call-out, `LaneFrame::vm_step`).
///
/// # Safety
/// `cx` is the running lane's context.
#[inline(always)]
pub(crate) unsafe fn unary_values(cx: *mut LaneContext, operator: u32, operand: Value) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    // A Number operand of a Number operator (negation, plus, increments): its result made
    // here, where `Vm::unary` would be a call (Box2D's `-x` on doubles).
    if let Some(number) = operand.as_number()
        && let Some(result) = quench_runtime::vm::operations::number_unary_result(operator, number)
    {
        return Some(Value::number(result));
    }
    // ToString of a value whose ToString runs nothing is the value itself, decided first.
    let pure = vm.converts_to_string_purely(operand);
    if matches!(operator, quench_runtime::bytecode::UNARY_TO_STRING_PRIMITIVE | quench_runtime::bytecode::UNARY_ADD_OPERAND_STRING) && pure {
        return Some(operand);
    }
    if let Some(answer) = unary_query(vm, operator, operand) {
        return Some(answer);
    }
    if operand.as_number().is_some() && super::coercion::numeric_unary(operator) {
        // SAFETY: the context's program outlives the lane run.
        return vm.unary(unsafe { &*(*cx).code }, operator, operand).ok();
    }
    if !pure {
        return None;
    }
    // SAFETY: the context's program outlives the lane run.
    let value = vm.unary(unsafe { &*(*cx).code }, operator, operand).ok()?;
    // Converting to a string can allocate.
    note_allocation(vm);
    Some(value)
}

/// Logical not and the `typeof` operators: they read any value's truthiness or type, so no
/// operand runs guest code. `None` for every other operator.
#[inline(always)]
fn unary_query(vm: &mut Vm, operator: u32, operand: Value) -> Option<Value> {
    if operator == super::coercion::LOGICAL_NOT {
        return Some(vm.logical_not(operand));
    }
    if let Some((index, negated)) = quench_runtime::bytecode::typeof_test(operator) {
        return Some(vm.typeof_test_value(operand, index, negated));
    }
    if operator == quench_runtime::bytecode::UNARY_TYPEOF_INDEX {
        return Some(vm.typeof_index_value(operand));
    }
    if let Some(elements) = quench_runtime::bytecode::array_pattern_test(operator) {
        return Some(Value::boolean(vm.array_pattern_fits(operand, elements)));
    }
    if operator == quench_runtime::bytecode::UNARY_FRESH_ARRAY_PATTERN {
        return Some(Value::boolean(vm.fresh_array_pattern_fits()));
    }
    if operator == super::coercion::TYPEOF {
        let name = vm.typeof_value(operand);
        // Interning the type's name can allocate.
        note_allocation(vm);
        return Some(name);
    }
    None
}
