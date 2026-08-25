const WORKER_TASK_FACT_SLOTS: usize = 64;
const WORKER_DATA_LIMIT: usize = 16;

#[derive(Clone, Copy)]
struct WorkerTaskPlan {
    handler_a_slot: u16,
    handler_b_slot: u16,
    data_size_slot: u16,
}

struct WorkerTaskFact {
    function: std::rc::Weak<crate::value::FunctionValue>,
    plan: Option<WorkerTaskPlan>,
}

thread_local! {
    static WORKER_TASK_FACTS: std::cell::RefCell<Vec<Option<WorkerTaskFact>>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

#[inline]
fn is_worker_task_candidate(function: &crate::value::FunctionValue) -> bool {
    function.params == 1
        && function.code.capture_slots().len() == 6
        && function.code.code().is_some_and(|code| code.len() == 7)
}

fn execute_worker_task(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    receiver: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Result<Option<crate::value::Value>, crate::execute::VmError> {
    let Some(plan) = worker_task_fact(function) else {
        return Ok(None);
    };
    let packet = arguments
        .first()
        .cloned()
        .unwrap_or(crate::value::Value::Undefined);
    let crate::value::Value::Object(task) = receiver else {
        return Ok(None);
    };
    if packet.is_nullish() {
        return worker_suspend(function, task);
    }
    let Some(state) = worker_packet_state(function, task, &packet, plan) else {
        return Ok(None);
    };
    apply_worker_state(&state);
    let result = crate::functions::execute_target(
        &state.callee,
        &state.scheduler,
        std::slice::from_ref(&packet),
    )?;
    crate::execution_trace::kernel("worker_task_run_word_slots", false);
    Ok(Some(result))
}

fn worker_suspend(
    function: &crate::value::FunctionValue,
    task: &crate::value::ObjectData,
) -> Result<Option<crate::value::Value>, crate::execute::VmError> {
    let Some(scheduler) = task_scheduler(task) else {
        return Ok(None);
    };
    let code = function.code.code().unwrap();
    let crate::ops::Op::Branch { then_ops, .. } = code.cold_at(4).unwrap() else {
        return Ok(None);
    };
    let Some(callee) = then_ops
        .code()
        .and_then(|code| cached_shape_method(&scheduler, code, 2))
    else {
        return Ok(None);
    };
    let result = crate::functions::execute_target(&callee, &scheduler, &[])?;
    crate::execution_trace::kernel("worker_task_run_word_slots", false);
    Ok(Some(result))
}

struct WorkerState<'a> {
    v1: &'a crate::register_file::SlotWord,
    v2: &'a crate::register_file::SlotWord,
    packet_id: &'a crate::register_file::SlotWord,
    packet_a1: &'a crate::register_file::SlotWord,
    payload: std::rc::Rc<crate::value::ArrayData>,
    values: [f64; WORKER_DATA_LIMIT],
    count: usize,
    next_id: f64,
    next_v2: f64,
    scheduler: crate::value::Value,
    callee: crate::value::Value,
}

fn worker_packet_state<'a>(
    function: &crate::value::FunctionValue,
    task: &'a crate::value::ObjectData,
    packet: &'a crate::value::Value,
    plan: WorkerTaskPlan,
) -> Option<WorkerState<'a>> {
    let crate::value::Value::Object(packet_object) = packet else {
        return None;
    };
    if task.has_replacement() || packet_object.has_replacement() {
        return None;
    }
    let v1 = writable_own_word(task, "v1")?;
    let v2 = writable_own_word(task, "v2")?;
    let handler_a = function.captures.get_number(plan.handler_a_slot)?;
    let handler_b = function.captures.get_number(plan.handler_b_slot)?;
    let next_id = if v1.number()? == handler_a {
        handler_b
    } else {
        handler_a
    };
    let count = exact_worker_count(function.captures.get_number(plan.data_size_slot)?)?;
    let (values, next_v2) = worker_values(v2.number()?, count);
    let payload = worker_payload(packet_object, count)?;
    let scheduler = task_scheduler(task)?;
    let callee = worker_queue_callee(function, &scheduler)?;
    Some(WorkerState {
        v1,
        v2,
        packet_id: writable_own_word(packet_object, "id")?,
        packet_a1: writable_own_word(packet_object, "a1")?,
        payload,
        values,
        count,
        next_id,
        next_v2,
        scheduler,
        callee,
    })
}

fn exact_worker_count(value: f64) -> Option<usize> {
    (value.is_finite() && value >= 0.0 && value.fract() == 0.0 && value <= WORKER_DATA_LIMIT as f64)
        .then_some(value as usize)
}

fn worker_values(mut value: f64, count: usize) -> ([f64; WORKER_DATA_LIMIT], f64) {
    let mut values = [0.0; WORKER_DATA_LIMIT];
    for slot in &mut values[..count] {
        value += 1.0;
        if value > 26.0 {
            value = 1.0;
        }
        *slot = value;
    }
    (values, value)
}

fn worker_payload(
    packet: &crate::value::ObjectData,
    count: usize,
) -> Option<std::rc::Rc<crate::value::ArrayData>> {
    let value = crate::vm::proven_own_word(packet, "a2")?.load();
    let crate::value::Value::Array(array) = value else {
        return None;
    };
    (crate::locals::array_word_is_current(&array)
        && array.header_length() == count
        && ((array.is_packed_ordinary() && array.is_numeric_packed())
            || (array.is_holey() && array.physical_len() == 0)))
        .then_some(array)
}

fn worker_queue_callee(
    function: &crate::value::FunctionValue,
    scheduler: &crate::value::Value,
) -> Option<crate::value::Value> {
    let code = function.code.code()?;
    let crate::ops::Op::Branch { else_ops, .. } = code.cold_at(4)? else {
        return None;
    };
    cached_shape_method(scheduler, else_ops.code()?, 21)
}

fn apply_worker_state(state: &WorkerState<'_>) {
    if state.payload.is_holey() {
        for (index, value) in state.values[..state.count].iter().copied().enumerate() {
            assert!(state.payload.append_preallocated_f64(index, value));
        }
    } else {
        let mut payload = state.payload.numeric_kernel_words_mut().unwrap();
        payload[..state.count].copy_from_slice(&state.values[..state.count]);
    }
    state.v1.store(crate::value::Value::Number(state.next_id));
    state.v2.store(crate::value::Value::Number(state.next_v2));
    state
        .packet_id
        .store(crate::value::Value::Number(state.next_id));
    state.packet_a1.store(crate::value::Value::Number(0.0));
}

fn worker_task_fact(function: &std::rc::Rc<crate::value::FunctionValue>) -> Option<WorkerTaskPlan> {
    let pointer = std::rc::Rc::as_ptr(function);
    let index = (pointer as usize >> 4) & (WORKER_TASK_FACT_SLOTS - 1);
    if let Some(plan) = WORKER_TASK_FACTS.with(|facts| {
        let facts = facts.borrow();
        let fact = facts.get(index)?.as_ref()?;
        (fact.function.as_ptr() == pointer).then_some(fact.plan)
    }) {
        return plan;
    }
    let plan = match_worker_task(function);
    WORKER_TASK_FACTS.with(|facts| {
        let mut facts = facts.borrow_mut();
        if facts.is_empty() {
            facts.resize_with(WORKER_TASK_FACT_SLOTS, || None);
        }
        facts[index] = Some(WorkerTaskFact {
            function: std::rc::Rc::downgrade(function),
            plan,
        });
    });
    plan
}

fn match_worker_task(function: &crate::value::FunctionValue) -> Option<WorkerTaskPlan> {
    let code = function.code.code()?;
    if !is_worker_task_candidate(function) || !worker_main_shape(code) {
        return None;
    }
    let crate::ops::Op::Branch {
        then_ops, else_ops, ..
    } = code.cold_at(4)?
    else {
        return None;
    };
    let (suspend, work) = (then_ops.code()?, else_ops.code()?);
    if !worker_suspend_shape(suspend) || !worker_work_shape(work) {
        return None;
    }
    let crate::ops::Op::Loop {
        test, body, update, ..
    } = work.cold_at(18)?
    else {
        return None;
    };
    let (test, body, update) = (test.code()?, body.code()?, update.code()?);
    if !worker_loop_shape(test, body, update) {
        return None;
    }
    let crate::ops::Op::Branch {
        then_ops: id_b,
        else_ops: id_a,
        ..
    } = work.cold_at(5)?
    else {
        return None;
    };
    Some(WorkerTaskPlan {
        handler_a_slot: id_a.code()?.instruction(3)?.b,
        handler_b_slot: id_b.code()?.instruction(3)?.b,
        data_size_slot: test.instruction(1)?.b,
    })
}

fn worker_main_shape(code: crate::machine::CodeView<'_>) -> bool {
    use crate::ir::Opcode::*;
    if code.len() != 7 {
        return false;
    }
    let ops: [_; 7] = std::array::from_fn(|pc| code.instruction(pc).unwrap());
    is_local_load(ops[0])
        && matches!(code.constant_at(1), Some((_, crate::ops::Constant::Null)))
        && binary_shape(code, 2, crate::ops::BinaryOp::Equal, ops[0].a, ops[1].a)
        && matches!(code.cold_at(4), Some(crate::ops::Op::Branch { .. }))
        && ops[6].opcode == Return
}

fn worker_suspend_shape(code: crate::machine::CodeView<'_>) -> bool {
    code.len() == 5
        && code.instruction(0).is_some_and(is_local_load)
        && code
            .instruction(1)
            .is_some_and(|op| op.opcode == crate::ir::Opcode::GetN)
        && code
            .instruction(2)
            .is_some_and(|op| op.opcode == crate::ir::Opcode::CallN && op.flags == 0)
        && named(code, 1, "scheduler")
        && named(code, 2, "suspendCurrent")
}

fn worker_work_shape(code: crate::machine::CodeView<'_>) -> bool {
    use crate::ir::Opcode::*;
    if code.len() != 26 {
        return false;
    }
    let ops: [_; 26] = std::array::from_fn(|pc| code.instruction(pc).unwrap());
    is_local_load(ops[0])
        && (ops[1].opcode, ops[1].b) == (GetN, ops[0].a)
        && is_local_load(ops[2])
        && binary_shape(code, 3, crate::ops::BinaryOp::Equal, ops[1].a, ops[2].a)
        && matches!(code.cold_at(5), Some(crate::ops::Op::Branch { .. }))
        && is_local_load(ops[6])
        && ops[7].opcode == Move
        && ops[8].opcode == Move
        && is_local_load(ops[9])
        && (ops[10].opcode, ops[10].b) == (GetN, ops[9].a)
        && (ops[11].opcode, ops[11].b) == (SetN, ops[10].a)
        && is_local_load(ops[12])
        && ops[13].opcode == Move
        && ops[14].opcode == Move
        && number_constant(code, 15, 0.0)
        && (ops[16].opcode, ops[16].b) == (SetN, ops[15].a)
        && matches!(code.cold_at(18), Some(crate::ops::Op::Loop { .. }))
        && is_local_load(ops[19])
        && (ops[20].opcode, ops[20].b) == (GetN, ops[19].a)
        && (ops[21].opcode, ops[21].b) == (GetN, ops[20].a)
        && is_local_load(ops[22])
        && ops[23].opcode == CallN
        && ops[23].flags == 1
        && ops[24].opcode == Return
        && named(code, 1, "v1")
        && named(code, 10, "v1")
        && named(code, 11, "id")
        && named(code, 16, "a1")
        && named(code, 20, "scheduler")
        && named(code, 21, "queue")
}

fn worker_loop_shape(
    test: crate::machine::CodeView<'_>,
    body: crate::machine::CodeView<'_>,
    update: crate::machine::CodeView<'_>,
) -> bool {
    use crate::ir::Opcode::*;
    if test.len() != 4 || body.len() != 24 || update.len() != 2 {
        return false;
    }
    let test_ops: [_; 4] = std::array::from_fn(|pc| test.instruction(pc).unwrap());
    let body_ops: [_; 24] = std::array::from_fn(|pc| body.instruction(pc).unwrap());
    is_local_load(test_ops[0])
        && is_local_load(test_ops[1])
        && binary_shape(
            test,
            2,
            crate::ops::BinaryOp::LessThan,
            test_ops[0].a,
            test_ops[1].a,
        )
        && is_local_load(body_ops[0])
        && body_ops[1].opcode == Move
        && (body_ops[2].opcode, body_ops[2].b) == (GetN, body_ops[1].a)
        && number_constant(body, 3, 1.0)
        && binary_shape(
            body,
            4,
            crate::ops::BinaryOp::NumericAdd,
            body_ops[2].a,
            body_ops[3].a,
        )
        && (body_ops[5].opcode, body_ops[5].b) == (SetN, body_ops[4].a)
        && matches!(body.cold_at(6), Some(crate::ops::Op::Unary { .. }))
        && body_ops[22].opcode == ASetI
        && named(body, 2, "v2")
        && named(body, 5, "v2")
        && named(body, 16, "a2")
        && named(body, 21, "v2")
        && update
            .instruction(0)
            .is_some_and(|op| op.opcode == UpdateLocal)
        && update.instruction(1).is_some_and(|op| op.opcode == Return)
}
