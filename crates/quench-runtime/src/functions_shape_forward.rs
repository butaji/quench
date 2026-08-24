fn execute_forward_zero(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    receiver: &crate::value::Value,
    plan: ForwardZeroPlan,
) -> Option<crate::value::Value> {
    let crate::value::Value::Object(receiver) = receiver else { return None };
    let code = function.code.code()?;
    let nested = crate::vm::get_named_cached_object(
        receiver,
        &code.metadata_at(plan.receiver_pc)?.named_cache,
    )?;
    let nested = crate::locals::resolved_replacement(nested);
    let crate::value::Value::Object(nested_object) = &nested else { return None };
    let callee = crate::vm::get_named_cached_object(
        nested_object,
        &code.metadata_at(plan.call_pc)?.named_cache,
    )?;
    let crate::value::Value::Function(callee) = callee else { return None };
    let receiver = crate::vm::bare_call_receiver(&callee, &nested);
    let value = execute_shape_kernel(&callee, &receiver, &[])?;
    crate::execution_trace::event(crate::execution_trace::Event::ShapeKernelHit);
    crate::execution_trace::kernel("shape_forward_zero", false);
    Some(value)
}

fn match_forward_zero(
    function: &std::rc::Rc<crate::value::FunctionValue>,
) -> Option<ForwardZeroPlan> {
    let code = function.code.code()?;
    if code.len() != 6 || function.params != 0 {
        return None;
    }
    let [receiver, nested, call, returned, fallback, fallback_return] =
        std::array::from_fn(|pc| code.instruction(pc).unwrap());
    use crate::ir::Opcode::*;
    (receiver.opcode == LoadLocalChecked
        && (nested.opcode, nested.b) == (GetN, receiver.a)
        && call.opcode == CallN
        && call.flags == 0
        && call.b == nested.a
        && (returned.opcode, returned.a) == (Return, call.a)
        && fallback.opcode == LoadConst
        && (fallback_return.opcode, fallback_return.a) == (Return, fallback.a))
        .then_some(ForwardZeroPlan {
            receiver_pc: 1,
            call_pc: 2,
        })
}
