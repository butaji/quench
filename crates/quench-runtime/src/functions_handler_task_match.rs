fn match_handler_task(function: &crate::value::FunctionValue) -> Option<HandlerTaskPlan> {
    let code = function.code.code()?;
    if !is_handler_task_candidate(function) || !handler_main_shape(code) {
        return None;
    }
    let crate::ops::Op::Branch {
        then_ops: incoming, ..
    } = code.cold_at(4)?
    else {
        return None;
    };
    let incoming = incoming.code()?;
    let crate::ops::Op::Branch {
        then_ops: work,
        else_ops: device,
        ..
    } = incoming.cold_at(5)?
    else {
        return None;
    };
    if !handler_incoming_shape(incoming, work.code()?, device.code()?) {
        return None;
    }
    let crate::ops::Op::Branch {
        then_ops: ready, ..
    } = code.cold_at(10)?
    else {
        return None;
    };
    let ready = ready.code()?;
    let crate::ops::Op::Branch {
        then_ops: partial,
        else_ops: complete,
        ..
    } = ready.cold_at(8)?
    else {
        return None;
    };
    if !handler_ready_shape(ready, partial.code()?, complete.code()?) {
        return None;
    }
    Some(HandlerTaskPlan {
        work_kind_slot: incoming.instruction(2)?.b,
        data_size_slot: ready.instruction(5)?.b,
    })
}

fn handler_main_shape(code: crate::machine::CodeView<'_>) -> bool {
    use crate::ir::Opcode::*;
    let ops: [_; 17] = std::array::from_fn(|pc| code.instruction(pc).unwrap());
    is_local_load(ops[0])
        && matches!(code.constant_at(1), Some((_, crate::ops::Constant::Null)))
        && binary_shape(code, 2, crate::ops::BinaryOp::NotEqual, ops[0].a, ops[1].a)
        && matches!(code.cold_at(4), Some(crate::ops::Op::Branch { .. }))
        && is_local_load(ops[5])
        && (ops[6].opcode, ops[6].b) == (GetN, ops[5].a)
        && matches!(code.constant_at(7), Some((_, crate::ops::Constant::Null)))
        && binary_shape(code, 8, crate::ops::BinaryOp::NotEqual, ops[6].a, ops[7].a)
        && matches!(code.cold_at(10), Some(crate::ops::Op::Branch { .. }))
        && is_local_load(ops[11])
        && (ops[12].opcode, ops[12].b) == (GetN, ops[11].a)
        && ops[13].opcode == CallN
        && ops[13].flags == 0
        && ops[13].b == ops[12].a
        && (ops[14].opcode, ops[14].a) == (Return, ops[13].a)
        && named(code, 6, "v1")
        && named(code, 12, "scheduler")
        && named(code, 13, "suspendCurrent")
}

fn handler_incoming_shape(
    test: crate::machine::CodeView<'_>,
    work: crate::machine::CodeView<'_>,
    device: crate::machine::CodeView<'_>,
) -> bool {
    test.len() == 7
        && work.len() == 10
        && device.len() == 10
        && named(test, 1, "kind")
        && named(work, 4, "addTo")
        && named(work, 6, "v1")
        && named(work, 8, "v1")
        && named(device, 4, "addTo")
        && named(device, 6, "v2")
        && named(device, 8, "v2")
        && work
            .instruction(7)
            .is_some_and(|op| op.opcode == crate::ir::Opcode::CallN && op.flags == 1)
        && device
            .instruction(7)
            .is_some_and(|op| op.opcode == crate::ir::Opcode::CallN && op.flags == 1)
}

fn handler_ready_shape(
    ready: crate::machine::CodeView<'_>,
    partial: crate::machine::CodeView<'_>,
    complete: crate::machine::CodeView<'_>,
) -> bool {
    if ready.len() != 10
        || partial.len() != 7
        || complete.len() != 17
        || !named(ready, 1, "v1")
        || !named(ready, 2, "a1")
        || !named(partial, 1, "v2")
        || !named(complete, 1, "v1")
        || !named(complete, 8, "link")
        || !named(complete, 11, "scheduler")
        || !named(complete, 12, "queue")
    {
        return false;
    }
    let Some(crate::ops::Op::Branch { then_ops, .. }) = partial.cold_at(5) else {
        return false;
    };
    let Some(work) = then_ops.code() else {
        return false;
    };
    work.len() == 34
        && named(work, 1, "v2")
        && named(work, 8, "link")
        && named(work, 14, "v1")
        && named(work, 15, "a2")
        && named(work, 18, "a1")
        && named(work, 20, "v1")
        && named(work, 26, "a1")
        && named(work, 28, "scheduler")
        && named(work, 29, "queue")
}
