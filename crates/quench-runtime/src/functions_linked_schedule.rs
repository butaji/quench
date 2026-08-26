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
    current.copy_from(list);
    let mut iterations = 0usize;
    loop {
        if scheduler.has_replacement() {
            return continue_linked_schedule_slow(receiver.clone(), &plan).map(Some);
        }
        let Some(tcb) = current.object_or_null_ptr() else {
            return continue_linked_schedule_slow(receiver.clone(), &plan).map(Some);
        };
        let Some(tcb) = tcb else { break };
        // SAFETY: the current slot owns the object through this iteration.
        let tcb = unsafe { &*tcb };
        if tcb.has_replacement() {
            return continue_linked_schedule_slow(receiver.clone(), &plan).map(Some);
        }
        let current_value = current.load();
        let held = crate::vm::is_truthy(&call_linked_method(
            &current_value,
            plan.body,
            plan.predicate_pc,
        )?);
        if held {
            let link = crate::vm::proven_own_word(tcb, plan.link);
            let Some(link) = link else {
                return continue_linked_schedule_slow(receiver.clone(), &plan).map(Some);
            };
            current.copy_from(link);
        } else {
            let id = crate::vm::proven_own_word(tcb, plan.id);
            let Some(id) = id else {
                return continue_linked_schedule_slow(receiver.clone(), &plan).map(Some);
            };
            current_id.copy_from(id);
            let next = call_linked_method(&current_value, plan.ready, plan.run_pc)?;
            current.store(next);
        }
        iterations += 1;
        crate::execution_trace::kernel("L|S|C", false);
    }
    crate::execution_trace::numeric_kernel_iterations("L|S|C", 0, iterations, 0, 0);
    Ok(Some(crate::value::Value::Undefined))
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
    if function.params != 0 {
        return None;
    }
    let (loop_pc, test, body, post_test) = (0..code.len()).find_map(|pc| {
        let instruction = code.instruction(pc)?;
        (instruction.opcode == crate::ir::Opcode::Slow).then_some(())?;
        let crate::ops::Op::Loop {
            test,
            body,
            post_test,
            ..
        } = code.cold(instruction)?
        else {
            return None;
        };
        Some((pc, test.code()?, body.code()?, *post_test))
    })?;
    if post_test {
        return None;
    }
    let current = named_get_in_null_test(test)?;
    let predicate_pc = named_zero_call(body)?;
    let (held, ready) = branch_arms(body)?;
    let (held_target, link) = named_get_assignment(held)?;
    let (ready_target, run_pc) = named_call_assignment(ready)?;
    if held_target != current || ready_target != current {
        return None;
    }
    let (current_id, id) = named_get_assignment_excluding(ready, current)?;
    let (setup_target, list) = named_get_assignment_before(code, loop_pc)?;
    (setup_target == current).then_some(LinkedSchedulePlan {
        body,
        ready,
        list,
        current,
        current_id,
        link,
        id,
        predicate_pc,
        run_pc,
    })
}

fn named_get_in_null_test(code: crate::machine::CodeView<'_>) -> Option<&str> {
    let has_not_equal = (0..code.len()).any(|pc| {
        code.instruction(pc).is_some_and(|instruction| {
            instruction.opcode == crate::ir::Opcode::Binary
                && crate::ir::compact_binary_operator(instruction.flags)
                    == Some(crate::ops::BinaryOp::NotEqual)
        })
    });
    has_not_equal.then_some(())?;
    (0..code.len()).find_map(|pc| named_opcode(code, pc, crate::ir::Opcode::GetN))
}

fn named_zero_call(code: crate::machine::CodeView<'_>) -> Option<usize> {
    (0..code.len()).find_map(|pc| {
        let call = code.instruction(pc)?;
        (call.opcode == crate::ir::Opcode::CallN && call.flags == 0).then_some(())?;
        code.metadata_at(pc)?.name.as_deref()?;
        (0..pc).rev().find(|get_pc| {
            code.instruction(*get_pc)
                .is_some_and(|get| get.opcode == crate::ir::Opcode::GetN && get.a == call.b)
        })?;
        Some(pc)
    })
}

fn branch_arms(
    code: crate::machine::CodeView<'_>,
) -> Option<(crate::machine::CodeView<'_>, crate::machine::CodeView<'_>)> {
    (0..code.len()).find_map(|pc| {
        let instruction = code.instruction(pc)?;
        (instruction.opcode == crate::ir::Opcode::Slow).then_some(())?;
        let crate::ops::Op::Branch {
            then_ops, else_ops, ..
        } = code.cold(instruction)?
        else {
            return None;
        };
        Some((then_ops.code()?, else_ops.code()?))
    })
}

fn named_opcode(
    code: crate::machine::CodeView<'_>,
    pc: usize,
    opcode: crate::ir::Opcode,
) -> Option<&str> {
    (code.instruction(pc)?.opcode == opcode)
        .then_some(())
        .and_then(|()| code.metadata_at(pc)?.name.as_deref())
}

fn named_get_assignment(code: crate::machine::CodeView<'_>) -> Option<(&str, &str)> {
    named_get_assignment_filter(code, |_| true)
}

fn named_get_assignment_excluding<'a>(
    code: crate::machine::CodeView<'a>,
    excluded: &str,
) -> Option<(&'a str, &'a str)> {
    named_get_assignment_filter(code, |target| target != excluded)
}

fn named_get_assignment_before(
    code: crate::machine::CodeView<'_>,
    end: usize,
) -> Option<(&str, &str)> {
    named_get_assignment_filter(code.slice(0, end)?, |_| true)
}

fn named_get_assignment_filter(
    code: crate::machine::CodeView<'_>,
    accept: impl Fn(&str) -> bool,
) -> Option<(&str, &str)> {
    (0..code.len()).find_map(|set_pc| {
        let set = code.instruction(set_pc)?;
        let target = named_opcode(code, set_pc, crate::ir::Opcode::SetN)?;
        accept(target).then_some(())?;
        let get_pc = (0..set_pc).rev().find(|pc| {
            code.instruction(*pc)
                .is_some_and(|get| get.opcode == crate::ir::Opcode::GetN && get.a == set.b)
        })?;
        Some((target, named_opcode(code, get_pc, crate::ir::Opcode::GetN)?))
    })
}

fn named_call_assignment(code: crate::machine::CodeView<'_>) -> Option<(&str, usize)> {
    (0..code.len()).find_map(|set_pc| {
        let set = code.instruction(set_pc)?;
        let target = named_opcode(code, set_pc, crate::ir::Opcode::SetN)?;
        let call_pc = (0..set_pc).rev().find(|pc| {
            code.instruction(*pc).is_some_and(|call| {
                call.opcode == crate::ir::Opcode::CallN && call.flags == 0 && call.a == set.b
            })
        })?;
        let call = code.instruction(call_pc)?;
        let get_pc = (0..call_pc).rev().find(|pc| {
            code.instruction(*pc)
                .is_some_and(|get| get.opcode == crate::ir::Opcode::GetN && get.a == call.b)
        })?;
        named_opcode(code, get_pc, crate::ir::Opcode::GetN)?;
        Some((target, call_pc))
    })
}
