/// Execute the structural `while (this.current != null)` scheduler shape.
///
/// Admission is based only on compact code and named-property facts. Before
/// the first mutation, every reachable link is proved to expose ordinary own
/// data slots and callable predicate/run properties. The loop then mutates the
/// canonical eight-byte slots in place; no replacement object or parallel
/// property representation is created.
fn execute_linked_schedule(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    receiver: &crate::value::Value,
) -> Result<Option<crate::value::Value>, crate::execute::VmError> {
    let Some(plan) = match_linked_schedule(function) else {
        return Ok(None);
    };
    let crate::value::Value::Object(scheduler) = receiver else {
        return Ok(None);
    };
    let Some(list) = crate::vm::proven_own_word(scheduler, plan.list) else {
        return Ok(None);
    };
    let Some(current) = writable_own_word(scheduler, plan.current) else {
        return Ok(None);
    };
    let Some(current_id) = writable_own_word(scheduler, plan.current_id) else {
        return Ok(None);
    };

    let list_value = list.load();
    if !linked_schedule_chain_is_proven(&list_value, &plan)? {
        return Ok(None);
    }
    current.store(list_value);

    loop {
        if scheduler.has_replacement() {
            return continue_linked_schedule_slow(receiver.clone(), &plan).map(Some);
        }
        let current_value = current.load();
        let crate::value::Value::Object(tcb) = &current_value else {
            if matches!(current_value, crate::value::Value::Null) {
                break;
            }
            return continue_linked_schedule_slow(receiver.clone(), &plan).map(Some);
        };
        if tcb.has_replacement() {
            return continue_linked_schedule_slow(receiver.clone(), &plan).map(Some);
        }
        let held = call_linked_method(&current_value, plan.body, plan.predicate_pc)?;
        if crate::vm::is_truthy(&held) {
            let Some(link) = crate::vm::proven_own_word(tcb, plan.link) else {
                return continue_linked_schedule_slow(receiver.clone(), &plan).map(Some);
            };
            current.store(link.load());
        } else {
            let Some(id) = crate::vm::proven_own_word(tcb, plan.id) else {
                return continue_linked_schedule_slow(receiver.clone(), &plan).map(Some);
            };
            current_id.store(id.load());
            let next = call_linked_method(&current_value, plan.ready, plan.run_pc)?;
            current.store(next);
        }
        crate::execution_trace::kernel("linked_schedule_word_slots", false);
    }
    Ok(Some(crate::value::Value::Undefined))
}

#[cold]
fn continue_linked_schedule_slow(
    mut scheduler: crate::value::Value,
    plan: &LinkedSchedulePlan<'_>,
) -> Result<crate::value::Value, crate::execute::VmError> {
    loop {
        scheduler = crate::locals::resolved_replacement(scheduler);
        let current = crate::execute::get_property_result(&scheduler, plan.current)?;
        if crate::equality::abstract_equal(&current, &crate::value::Value::Null)? {
            return Ok(crate::value::Value::Undefined);
        }
        let predicate = linked_method(&current, plan.body, plan.predicate_pc)?;
        let held = crate::functions::execute_target(&predicate, &current, &[])?;
        let next = if crate::vm::is_truthy(&held) {
            crate::execute::get_property_result(&current, plan.link)?
        } else {
            let id = crate::execute::get_property_result(&current, plan.id)?;
            scheduler = crate::properties::assign_set_property(&scheduler, plan.current_id, id)?;
            let run = linked_method(&current, plan.ready, plan.run_pc)?;
            crate::functions::execute_target(&run, &current, &[])?
        };
        scheduler = crate::properties::assign_set_property(&scheduler, plan.current, next)?;
    }
}

fn linked_schedule_chain_is_proven(
    start: &crate::value::Value,
    plan: &LinkedSchedulePlan<'_>,
) -> Result<bool, crate::execute::VmError> {
    let mut cursor = start.clone();
    let mut seen = std::collections::HashSet::new();
    loop {
        let crate::value::Value::Object(object) = &cursor else {
            return Ok(matches!(cursor, crate::value::Value::Null));
        };
        if object.has_replacement() {
            return Ok(false);
        }
        if !seen.insert(std::rc::Rc::as_ptr(object) as usize) {
            return Ok(false);
        }
        let Some(link) = crate::vm::proven_own_word(object, plan.link) else {
            return Ok(false);
        };
        if crate::vm::proven_own_word(object, plan.id).is_none() {
            return Ok(false);
        }
        let predicate = linked_method(&cursor, plan.body, plan.predicate_pc)?;
        let run = linked_method(&cursor, plan.ready, plan.run_pc)?;
        if !crate::conversion::is_callable(&predicate) || !crate::conversion::is_callable(&run) {
            return Ok(false);
        }
        cursor = link.load();
    }
}

fn linked_method(
    receiver: &crate::value::Value,
    code: crate::machine::CodeView<'_>,
    pc: usize,
) -> Result<crate::value::Value, crate::execute::VmError> {
    let metadata = code
        .metadata_at(pc)
        .ok_or(crate::execute::VmError::MissingReturn)?;
    let name = metadata
        .name
        .as_deref()
        .ok_or(crate::execute::VmError::MissingReturn)?;
    crate::vm::get_named_property_result(receiver, name, &metadata.named_cache)
}

fn call_linked_method(
    receiver: &crate::value::Value,
    code: crate::machine::CodeView<'_>,
    pc: usize,
) -> Result<crate::value::Value, crate::execute::VmError> {
    let callee = linked_method(receiver, code, pc)?;
    crate::functions::execute_target(&callee, receiver, &[])
}

fn writable_own_word<'a>(
    object: &'a crate::value::ObjectData,
    key: &str,
) -> Option<&'a crate::register_file::SlotWord> {
    if object.hot_properties().names().any(|name| {
        crate::builtins::is_deleted_key_for(name, key)
            || crate::builtins::is_descriptor_key_for(name, key)
    }) {
        return None;
    }
    crate::vm::proven_own_word(object, key)
}

#[derive(Clone, Copy)]
struct LinkedSchedulePlan<'a> {
    body: crate::machine::CodeView<'a>,
    ready: crate::machine::CodeView<'a>,
    list: &'a str,
    current: &'a str,
    current_id: &'a str,
    link: &'a str,
    id: &'a str,
    predicate_pc: usize,
    run_pc: usize,
}

fn match_linked_schedule(
    function: &std::rc::Rc<crate::value::FunctionValue>,
) -> Option<LinkedSchedulePlan<'_>> {
    let code = function.code.code()?;
    if function.params != 0 || code.len() != 10 {
        return None;
    }
    use crate::ir::Opcode::*;
    let main: [_; 10] = std::array::from_fn(|pc| code.instruction(pc).unwrap());
    if !is_local_load(main[0])
        || (main[1].opcode, main[1].b) != (Move, main[0].a)
        || (main[2].opcode, main[2].b) != (Move, main[1].a)
        || !is_local_load(main[3])
        || (main[4].opcode, main[4].b) != (GetN, main[3].a)
        || (main[5].opcode, main[5].a, main[5].b) != (SetN, main[2].a, main[4].a)
        || main[7].opcode != Slow
    {
        return None;
    }
    let crate::ops::Op::Loop {
        test,
        body,
        post_test,
        ..
    } = code.cold(main[7])?
    else {
        return None;
    };
    if *post_test {
        return None;
    }
    let (test, body) = (test.code()?, body.code()?);
    if test.len() != 5 || body.len() != 6 {
        return None;
    }
    let test_ops: [_; 5] = std::array::from_fn(|pc| test.instruction(pc).unwrap());
    if !is_local_load(test_ops[0])
        || (test_ops[1].opcode, test_ops[1].b) != (GetN, test_ops[0].a)
        || test_ops[3].opcode != Binary
        || crate::ir::compact_binary_operator(test_ops[3].flags)
            != Some(crate::ops::BinaryOp::NotEqual)
    {
        return None;
    }
    let body_ops: [_; 6] = std::array::from_fn(|pc| body.instruction(pc).unwrap());
    if !is_local_load(body_ops[0])
        || (body_ops[1].opcode, body_ops[1].b) != (GetN, body_ops[0].a)
        || (body_ops[2].opcode, body_ops[2].flags, body_ops[2].b) != (CallN, 0, body_ops[1].a)
        || body_ops[4].opcode != Slow
    {
        return None;
    }
    let crate::ops::Op::Branch {
        then_ops, else_ops, ..
    } = body.cold(body_ops[4])?
    else {
        return None;
    };
    let (held, ready) = (then_ops.code()?, else_ops.code()?);
    if !held_schedule_shape(held) || !ready_schedule_shape(ready) {
        return None;
    }
    let list = code.metadata_at(4)?.name.as_deref()?;
    let current = code.metadata_at(5)?.name.as_deref()?;
    let current_id = ready.metadata_at(6)?.name.as_deref()?;
    let link = held.metadata_at(5)?.name.as_deref()?;
    let id = ready.metadata_at(5)?.name.as_deref()?;
    (test.metadata_at(1)?.name.as_deref()? == current
        && body.metadata_at(1)?.name.as_deref()? == current
        && held.metadata_at(4)?.name.as_deref()? == current
        && held.metadata_at(6)?.name.as_deref()? == current
        && ready.metadata_at(4)?.name.as_deref()? == current
        && ready.metadata_at(11)?.name.as_deref()? == current
        && ready.metadata_at(13)?.name.as_deref()? == current)
        .then_some(LinkedSchedulePlan {
            body,
            ready,
            list,
            current,
            current_id,
            link,
            id,
            predicate_pc: 2,
            run_pc: 12,
        })
}

fn held_schedule_shape(code: crate::machine::CodeView<'_>) -> bool {
    use crate::ir::Opcode::*;
    if code.len() != 8 {
        return false;
    }
    let ops: [_; 8] = std::array::from_fn(|pc| code.instruction(pc).unwrap());
    is_local_load(ops[0])
        && ops[1].opcode == Move
        && ops[2].opcode == Move
        && is_local_load(ops[3])
        && (ops[4].opcode, ops[4].b) == (GetN, ops[3].a)
        && (ops[5].opcode, ops[5].b) == (GetN, ops[4].a)
        && (ops[6].opcode, ops[6].a, ops[6].b) == (SetN, ops[2].a, ops[5].a)
}

fn ready_schedule_shape(code: crate::machine::CodeView<'_>) -> bool {
    use crate::ir::Opcode::*;
    if code.len() != 15 {
        return false;
    }
    let ops: [_; 15] = std::array::from_fn(|pc| code.instruction(pc).unwrap());
    is_local_load(ops[0])
        && ops[1].opcode == Move
        && ops[2].opcode == Move
        && is_local_load(ops[3])
        && (ops[4].opcode, ops[4].b) == (GetN, ops[3].a)
        && (ops[5].opcode, ops[5].b) == (GetN, ops[4].a)
        && (ops[6].opcode, ops[6].a, ops[6].b) == (SetN, ops[2].a, ops[5].a)
        && is_local_load(ops[7])
        && ops[8].opcode == Move
        && ops[9].opcode == Move
        && is_local_load(ops[10])
        && (ops[11].opcode, ops[11].b) == (GetN, ops[10].a)
        && (ops[12].opcode, ops[12].flags, ops[12].b) == (CallN, 0, ops[11].a)
        && (ops[13].opcode, ops[13].a, ops[13].b) == (SetN, ops[9].a, ops[12].a)
}
