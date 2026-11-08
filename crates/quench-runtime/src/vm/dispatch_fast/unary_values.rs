//! `Unary` records answered in the lane where the conversion runs no guest code.
use super::*;

/// A `Unary` record's value where it runs no guest code and throws nothing: logical not and
/// `typeof` of any value, and every other operator of a Number, String, Boolean, null or
/// undefined, whose conversions are pure.
///
/// # Safety
/// `cx` is the running lane's context and `r` its registers.
#[inline(always)]
pub(super) unsafe fn lane_unary_value<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    i: &LaneInstruction,
) -> Option<Value> {
    let operator = i.imm();
    let operand = r.get(i.b);
    let vm = unsafe { vm(cx) };
    if let Some(answer) = lane_value_query(vm, operator, operand) {
        return Some(answer);
    }
    let pure = vm.converts_to_string_purely(operand);
    if operator == crate::bytecode::UNARY_TO_STRING_PRIMITIVE && pure {
        return Some(operand);
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
fn lane_value_query<H: Host>(vm: &mut Vm<H>, operator: u32, operand: Value) -> Option<Value> {
    if operator == super::coercion::LOGICAL_NOT {
        return Some(vm.logical_not(operand));
    }
    if let Some((index, negated)) = crate::bytecode::typeof_test(operator) {
        return Some(vm.typeof_test_value(operand, index, negated));
    }
    if operator == crate::bytecode::UNARY_TYPEOF_INDEX {
        return Some(vm.typeof_index_value(operand));
    }
    if operator == super::coercion::TYPEOF {
        let name = vm.typeof_value(operand);
        // Interning the type's name can allocate.
        note_allocation(vm);
        return Some(name);
    }
    None
}
