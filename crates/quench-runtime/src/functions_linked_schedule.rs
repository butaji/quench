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
    let task_control_run = match &list_value {
        crate::value::Value::Object(_) => {
            let callee = linked_method(&list_value, plan.ready, plan.run_pc)?;
            match callee {
                crate::value::Value::Function(function) => {
                    match_task_control_run(&function).map(|run| (function, run))
                }
                _ => None,
            }
        }
        crate::value::Value::Null => None,
        _ => return Ok(None),
    };
    let Some((task_control_function, task_control_plan)) = task_control_run else {
        return Ok(None);
    };
    if !linked_schedule_chain_has_run(&list_value, &plan, &task_control_function)? {
        return Ok(None);
    }
    let task_runners = linked_task_runners(&list_value);
    let packet_link_cache = std::cell::Cell::new(0);
    current.copy_from(list);

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
        let direct = task_runners
            .as_ref()
            .and_then(|runners| runners.for_object(tcb));
        if direct.is_some() {
            crate::execution_trace::kernel("linked_tcb_word_slots", false);
        }
        let held = match direct.and_then(DirectTaskRunner::is_held_or_suspended) {
            Some(held) => held,
            None => {
                let current_value = current.load();
                crate::vm::is_truthy(&call_linked_method(
                    &current_value,
                    plan.body,
                    plan.predicate_pc,
                )?)
            }
        };
        if held {
            let link = direct
                .map(|runner| runner.word(runner.link))
                .or_else(|| crate::vm::proven_own_word(tcb, plan.link));
            let Some(link) = link else {
                return continue_linked_schedule_slow(receiver.clone(), &plan).map(Some);
            };
            current.copy_from(link);
        } else {
            let id = direct
                .map(|runner| runner.word(runner.id))
                .or_else(|| crate::vm::proven_own_word(tcb, plan.id));
            let Some(id) = id else {
                return continue_linked_schedule_slow(receiver.clone(), &plan).map(Some);
            };
            current_id.copy_from(id);
            let next = match execute_task_control_run(
                tcb,
                &task_control_plan,
                direct,
                &packet_link_cache,
            )? {
                Some(value) => value,
                None => {
                    let current_value = current.load();
                    crate::functions::execute_target(
                        &crate::value::Value::Function(std::rc::Rc::clone(&task_control_function)),
                        &current_value,
                        &[],
                    )?
                }
            };
            current.store(next);
        }
        crate::execution_trace::kernel("linked_schedule_word_slots", false);
    }
    Ok(Some(crate::value::Value::Undefined))
}

#[derive(Clone, Copy)]
struct TaskControlRunPlan {
    suspended_runnable: f64,
    running: f64,
    runnable: f64,
}

fn execute_task_control_run(
    tcb: &crate::value::ObjectData,
    plan: &TaskControlRunPlan,
    direct: Option<&DirectTaskRunner>,
    packet_link_cache: &std::cell::Cell<u64>,
) -> Result<Option<crate::value::Value>, crate::execute::VmError> {
    if tcb.has_replacement() {
        return Ok(None);
    }
    let state = direct
        .map(|runner| runner.word(runner.state))
        .or_else(|| writable_own_word(tcb, "state"));
    let Some(state) = state else {
        return Ok(None);
    };
    let Some(state_number) = state.number() else {
        return Ok(None);
    };
    let queue = direct
        .map(|runner| runner.word(runner.queue))
        .or_else(|| writable_own_word(tcb, "queue"));
    let Some(queue) = queue else {
        return Ok(None);
    };
    let task = direct
        .map(|runner| runner.word(runner.task_word))
        .or_else(|| crate::vm::proven_own_word(tcb, "task"));
    let Some(task) = task else {
        return Ok(None);
    };
    let task_pointer = task.object_or_null_ptr().flatten();
    let direct = direct.filter(|runner| task_pointer.is_some_and(|task| runner.matches(task)));
    let task_value = direct.is_none().then(|| task.load());
    let task = direct.map_or_else(
        || task_value.as_ref().expect("slow task materialized"),
        |runner| &runner.task_value,
    );

    let packet = if state_number == plan.suspended_runnable {
        let packet = queue.load();
        let crate::value::Value::Object(packet_object) = &packet else {
            return Ok(None);
        };
        if packet_object.has_replacement() {
            return Ok(None);
        }
        let Some(link) = linked_schedule_word(packet_object, "link", packet_link_cache) else {
            return Ok(None);
        };
        let next = link.load();
        queue.store(next.clone());
        state.store(crate::value::Value::Number(
            if matches!(
                next,
                crate::value::Value::Null | crate::value::Value::Undefined
            ) {
                plan.running
            } else {
                plan.runnable
            },
        ));
        packet
    } else {
        crate::value::Value::Null
    };
    let result = match direct {
        Some(runner) => match runner.execute(&packet)? {
            Some(result) => {
                crate::execution_trace::kernel("linked_task_direct", false);
                result
            }
            None => {
                crate::execution_trace::kernel("linked_task_direct", true);
                let run = crate::execute::get_property_result(&task, "run")?;
                crate::functions::execute_target(&run, &task, &[packet])?
            }
        },
        None => {
            let run = crate::execute::get_property_result(&task, "run")?;
            if !crate::conversion::is_callable(&run) {
                return Ok(None);
            }
            crate::functions::execute_target(&run, &task, &[packet])?
        }
    };
    crate::execution_trace::kernel("task_control_run_word_slots", false);
    Ok(Some(result))
}

fn linked_schedule_word<'a>(
    object: &'a crate::value::ObjectData,
    key: &str,
    cache: &std::cell::Cell<u64>,
) -> Option<&'a crate::register_file::SlotWord> {
    if let Some(crate::vm::NamedCachedPayload::Word(word)) =
        crate::vm::get_named_cached_payload(object, cache)
    {
        // SAFETY: the layout guard above proves that `word` belongs to the
        // retained object and names the admitted ordinary own slot.
        return Some(unsafe { &*word });
    }
    let word = crate::vm::proven_own_word(object, key)?;
    let slot = object.physical_slot_for_name(key)?;
    cache.set(crate::machine::pack_named_cache(
        object.semantic_layout_id(),
        slot as u32,
    ));
    Some(word)
}

fn match_task_control_run(
    function: &std::rc::Rc<crate::value::FunctionValue>,
) -> Option<TaskControlRunPlan> {
    let code = function.code.code()?;
    if function.params != 0 || code.len() != 14 {
        return None;
    }
    use crate::ir::Opcode::*;
    let ops: [_; 14] = std::array::from_fn(|pc| code.instruction(pc).unwrap());
    if !is_local_load(ops[0])
        || (ops[1].opcode, ops[1].b) != (GetN, ops[0].a)
        || !is_local_load(ops[2])
        || ops[3].opcode != Binary
        || crate::ir::compact_binary_operator(ops[3].flags) != Some(crate::ops::BinaryOp::Equal)
        || (ops[3].b, ops[3].c) != (ops[1].a, ops[2].a)
        || ops[5].opcode != Slow
        || !is_local_load(ops[6])
        || (ops[7].opcode, ops[7].b) != (GetN, ops[6].a)
        || (ops[8].opcode, ops[8].b) != (GetN, ops[7].a)
        || !is_local_load(ops[9])
        || (ops[10].opcode, ops[10].flags, ops[10].b, ops[10].c) != (CallN, 1, ops[7].a, ops[8].a)
        || (ops[11].opcode, ops[11].a) != (Return, ops[10].a)
        || code.metadata_at(1)?.name.as_deref()? != "state"
        || code.metadata_at(7)?.name.as_deref()? != "task"
        || code.metadata_at(8)?.name.as_deref()? != "run"
    {
        return None;
    }
    let crate::ops::Op::Branch {
        then_ops, else_ops, ..
    } = code.cold(ops[5])?
    else {
        return None;
    };
    let (ready, idle) = (then_ops.code()?, else_ops.code()?);
    if ready.len() != 16 || idle.len() != 3 {
        return None;
    }
    let ready_ops: [_; 16] = std::array::from_fn(|pc| ready.instruction(pc).unwrap());
    if !is_local_load(ready_ops[0])
        || (ready_ops[1].opcode, ready_ops[1].b) != (GetN, ready_ops[0].a)
        || ready_ops[2].opcode != StoreLocal
        || !is_local_load(ready_ops[3])
        || ready_ops[4].opcode != Move
        || ready_ops[5].opcode != Move
        || !is_local_load(ready_ops[6])
        || (ready_ops[7].opcode, ready_ops[7].b) != (GetN, ready_ops[6].a)
        || (ready_ops[8].opcode, ready_ops[8].a, ready_ops[8].b)
            != (SetN, ready_ops[5].a, ready_ops[7].a)
        || !is_local_load(ready_ops[9])
        || (ready_ops[10].opcode, ready_ops[10].b) != (GetN, ready_ops[9].a)
        || ready_ops[14].opcode != Slow
        || ready.metadata_at(1)?.name.as_deref()? != "queue"
        || ready.metadata_at(7)?.name.as_deref()? != "link"
        || ready.metadata_at(8)?.name.as_deref()? != "queue"
        || ready.metadata_at(10)?.name.as_deref()? != "queue"
    {
        return None;
    }
    let (crate::ops::Op::Branch {
        then_ops, else_ops, ..
    }
    | crate::ops::Op::Conditional {
        consequent: then_ops,
        alternate: else_ops,
        ..
    }) = ready.cold(ready_ops[14])?
    else {
        return None;
    };
    let (empty, nonempty) = (then_ops.code()?, else_ops.code()?);
    if !task_state_arm(empty) || !task_state_arm(nonempty) {
        return None;
    }
    let running_slot = empty.instruction(3)?.b;
    let runnable_slot = nonempty.instruction(3)?.b;
    Some(TaskControlRunPlan {
        suspended_runnable: function.captures.get_number(ops[2].b)?,
        running: function.captures.get_number(running_slot)?,
        runnable: function.captures.get_number(runnable_slot)?,
    })
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

fn linked_schedule_chain_has_run(
    start: &crate::value::Value,
    plan: &LinkedSchedulePlan<'_>,
    expected: &std::rc::Rc<crate::value::FunctionValue>,
) -> Result<bool, crate::execute::VmError> {
    let mut cursor = start.clone();
    loop {
        let crate::value::Value::Object(object) = &cursor else {
            return Ok(matches!(cursor, crate::value::Value::Null));
        };
        let run = linked_method(&cursor, plan.ready, plan.run_pc)?;
        let crate::value::Value::Function(run) = run else {
            return Ok(false);
        };
        if !std::rc::Rc::ptr_eq(&run, expected) {
            return Ok(false);
        }
        let Some(link) = crate::vm::proven_own_word(object, plan.link) else {
            return Ok(false);
        };
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
