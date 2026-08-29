#[derive(Clone, Copy)]
struct PropertySelectPlan {
    state_pc: usize,
    expected_slot: u16,
    expected_pc: usize,
    conditional_pc: usize,
}

fn execute_property_select(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    receiver: &crate::value::Value,
    plan: PropertySelectPlan,
) -> Option<crate::value::Value> {
    let crate::value::Value::Object(receiver) = receiver else { return None };
    let code = function.code.code()?;
    let state = cached_number(receiver, code, plan.state_pc)?;
    let expected = crate::locals::resolved_replacement(function.captures.get(plan.expected_slot));
    let crate::value::Value::Object(expected) = expected else { return None };
    let expected = cached_number(&expected, code, plan.expected_pc)?;
    let branch = conditional_branch(code, plan.conditional_pc, state == expected)?;
    let value = branch_property(branch, receiver)?;
    crate::execution_trace::event(crate::execution_trace::Event::ShapeKernelHit);
    crate::execution_trace::kernel("shape_property_select", false);
    Some(value)
}

fn cached_number(
    object: &crate::value::ObjectData,
    code: crate::machine::CodeView<'_>,
    pc: usize,
) -> Option<f64> {
    crate::vm::get_named_cached_object(object, &code.metadata_at(pc)?.named_cache)?.as_number()
}

fn conditional_branch(
    code: crate::machine::CodeView<'_>,
    pc: usize,
    condition: bool,
) -> Option<crate::machine::CodeView<'_>> {
    let crate::ops::Op::Conditional { consequent, alternate, .. } = code.cold_at(pc)? else {
        return None;
    };
    if condition { consequent.code() } else { alternate.code() }
}

fn branch_property(
    branch: crate::machine::CodeView<'_>,
    receiver: &crate::value::ObjectData,
) -> Option<crate::value::Value> {
    branch_property_shape(branch).then_some(())?;
    crate::vm::get_named_cached_object(receiver, &branch.metadata_at(1)?.named_cache)
}

fn match_property_select(
    function: &std::rc::Rc<crate::value::FunctionValue>,
) -> Option<PropertySelectPlan> {
    let code = function.code.code()?;
    if code.len() != 9 || function.params != 0 {
        return None;
    }
    let ops = std::array::from_fn(|pc| code.instruction(pc).unwrap());
    let expected_object = ops[2];
    let conditional = ops[5];
    let crate::ops::Op::Conditional { dst, condition, consequent, alternate } = code.cold(conditional)? else {
        return None;
    };
    property_select_shape(ops, *dst, *condition, consequent.code()?, alternate.code()?)
        .then_some(PropertySelectPlan {
            state_pc: 1,
            expected_slot: expected_object.b,
            expected_pc: 3,
            conditional_pc: 5,
        })
}

fn property_select_shape(
    [receiver, state, expected_object, expected, equal, conditional, returned, fallback, fallback_return]: [crate::ir::Instruction; 9],
    dst: u16,
    condition: u16,
    consequent: crate::machine::CodeView<'_>,
    alternate: crate::machine::CodeView<'_>,
) -> bool {
    use crate::ir::Opcode::*;
    is_local_load(receiver)
        && (state.opcode, state.b) == (GetN, receiver.a)
        && is_local_load(expected_object)
        && (expected.opcode, expected.b) == (GetN, expected_object.a)
        && equal.opcode == Binary
        && equal.flags == crate::ir::compact_binary_id(crate::ops::BinaryOp::Equal)
        && (equal.b, equal.c) == (state.a, expected.a)
        && conditional.opcode == Slow
        && condition == equal.a
        && branch_property_shape(consequent)
        && branch_property_shape(alternate)
        && (returned.opcode, returned.a) == (Return, dst)
        && fallback.opcode == LoadConst
        && (fallback_return.opcode, fallback_return.a) == (Return, fallback.a)
}

fn branch_property_shape(branch: crate::machine::CodeView<'_>) -> bool {
    if branch.len() != 3 {
        return false;
    }
    let [load, get, returned] = std::array::from_fn(|pc| branch.instruction(pc).unwrap());
    is_local_load(load)
        && (get.opcode, get.b) == (crate::ir::Opcode::GetN, load.a)
        && (returned.opcode, returned.a) == (crate::ir::Opcode::Return, get.a)
}
