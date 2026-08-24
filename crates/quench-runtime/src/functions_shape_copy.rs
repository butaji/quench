fn execute_copy_method_property(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    receiver: &crate::value::Value,
    plan: CopyMethodPropertyPlan,
) -> Option<crate::value::Value> {
    let code = function.code.code()?;
    let output = execute_named_shape_zero(receiver, code, plan.output_call_pc)?;
    let input = execute_named_shape_zero(receiver, code, plan.input_call_pc)?;
    let crate::value::Value::Object(input) = crate::locals::resolved_replacement(input) else {
        return None;
    };
    let value = crate::vm::get_named_cached_object(
        &input,
        &code.metadata_at(plan.get_pc)?.named_cache,
    )?
    .as_number()?;
    let crate::value::Value::Object(output) = crate::locals::resolved_replacement(output) else {
        return None;
    };
    let cell = crate::vm::get_named_cached_cell(
        &output,
        &code.metadata_at(plan.set_pc)?.named_cache,
    )?;
    // SAFETY: the guarded output object retains its ordinary data-property cell.
    unsafe { &*cell }.store(crate::value::Value::Number(value));
    crate::execution_trace::event(crate::execution_trace::Event::ShapeKernelHit);
    crate::execution_trace::kernel("shape_copy_method_property", false);
    Some(crate::value::Value::Undefined)
}

fn execute_named_shape_zero(
    receiver: &crate::value::Value,
    code: crate::machine::CodeView<'_>,
    pc: usize,
) -> Option<crate::value::Value> {
    let metadata = code.metadata_at(pc)?;
    let callee = crate::vm::get_named_property_result(
        receiver,
        metadata.name.as_deref()?,
        &metadata.named_cache,
    )
    .ok()?;
    let crate::value::Value::Function(callee) = callee else { return None };
    let receiver = crate::vm::bare_call_receiver(&callee, receiver);
    execute_shape_kernel(&callee, &receiver, &[])
}

fn match_copy_method_property(
    function: &std::rc::Rc<crate::value::FunctionValue>,
) -> Option<CopyMethodPropertyPlan> {
    let code = function.code.code()?;
    if !copy_method_property_shape(code, function.params) {
        return None;
    }
    Some(CopyMethodPropertyPlan {
        output_call_pc: 1,
        input_call_pc: 5,
        get_pc: 6,
        set_pc: 7,
    })
}

fn copy_method_property_shape(code: crate::machine::CodeView<'_>, params: u16) -> bool {
    if code.len() != 10 || params != 0 {
        return false;
    }
    let [receiver, output, move_one, move_two, receiver_again, input, get, set, undefined, returned] =
        std::array::from_fn(|pc| code.instruction(pc).unwrap());
    use crate::ir::Opcode::*;
    receiver.opcode == LoadLocalChecked
        && (output.opcode, output.flags, output.b) == (CallN, 0, receiver.a)
        && (move_one.opcode, move_one.b) == (Move, output.a)
        && (move_two.opcode, move_two.b) == (Move, move_one.a)
        && (receiver_again.opcode, receiver_again.b) == (LoadLocalChecked, receiver.b)
        && (input.opcode, input.flags, input.b) == (CallN, 0, receiver_again.a)
        && (get.opcode, get.b) == (GetN, input.a)
        && (set.opcode, set.a, set.b) == (SetN, move_two.a, get.a)
        && code.metadata_at(6).and_then(|meta| meta.name.as_deref())
            == code.metadata_at(7).and_then(|meta| meta.name.as_deref())
        && matches!(code.constant_at(8), Some((_, crate::ops::Constant::Undefined)))
        && undefined.opcode == LoadConst
        && (returned.opcode, returned.a) == (Return, undefined.a)
}
