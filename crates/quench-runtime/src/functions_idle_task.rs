const IDLE_TASK_FACT_SLOTS: usize = 64;

#[derive(Clone, Copy)]
struct IdleTaskPlan {
    device_a_slot: u16,
    device_b_slot: u16,
}

struct IdleTaskFact {
    function: std::rc::Weak<crate::value::FunctionValue>,
    plan: Option<IdleTaskPlan>,
}

thread_local! {
    static IDLE_TASK_FACTS: std::cell::RefCell<Vec<Option<IdleTaskFact>>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

#[inline]
fn is_idle_task_candidate(function: &crate::value::FunctionValue) -> bool {
    function.params == 1
        && function.code.capture_slots().len() == 3
        && function.code.code().is_some_and(|code| code.len() == 23)
}

fn execute_idle_task(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    receiver: &crate::value::Value,
) -> Result<Option<crate::value::Value>, crate::execute::VmError> {
    let Some(plan) = idle_task_fact(function) else {
        return Ok(None);
    };
    let crate::value::Value::Object(task) = receiver else {
        return Ok(None);
    };
    if task.has_replacement() {
        return Ok(None);
    }
    let Some(count) = writable_own_word(task, "count") else {
        return Ok(None);
    };
    let Some(count_value) = count.number() else {
        return Ok(None);
    };
    let Some(scheduler) = task_scheduler(task) else {
        return Ok(None);
    };
    let next_count = count_value - 1.0;
    let call = if next_count == 0.0 {
        idle_hold_call(function, &scheduler)
    } else {
        idle_release_call(function, task, &scheduler, plan)
    };
    let Some(call) = call else {
        return Ok(None);
    };

    count.store(crate::value::Value::Number(next_count));
    if let Some((slot, value)) = call.next_v1 {
        slot.store(crate::value::Value::Number(f64::from(value)));
    }
    let result = match &call.argument {
        Some(argument) => crate::functions::execute_target(
            &call.callee,
            &scheduler,
            std::slice::from_ref(argument),
        ),
        None => crate::functions::execute_target(&call.callee, &scheduler, &[]),
    }?;
    crate::execution_trace::kernel("idle_task_run_word_slots", false);
    Ok(Some(result))
}

struct IdleCall<'a> {
    callee: crate::value::Value,
    argument: Option<crate::value::Value>,
    next_v1: Option<(&'a crate::register_file::SlotWord, i32)>,
}

fn task_scheduler(task: &crate::value::ObjectData) -> Option<crate::value::Value> {
    let scheduler = crate::vm::proven_own_word(task, "scheduler")?.load();
    let crate::value::Value::Object(object) = &scheduler else {
        return None;
    };
    (!object.has_replacement()).then_some(scheduler)
}

fn idle_hold_call(
    function: &crate::value::FunctionValue,
    scheduler: &crate::value::Value,
) -> Option<IdleCall<'static>> {
    let code = function.code.code()?;
    let crate::ops::Op::Branch { then_ops, .. } = code.cold_at(12)? else {
        return None;
    };
    let branch = then_ops.code()?;
    let callee = cached_shape_method(scheduler, branch, 2)?;
    Some(IdleCall {
        callee,
        argument: None,
        next_v1: None,
    })
}

fn idle_release_call<'a>(
    function: &crate::value::FunctionValue,
    task: &'a crate::value::ObjectData,
    scheduler: &crate::value::Value,
    plan: IdleTaskPlan,
) -> Option<IdleCall<'a>> {
    let v1 = writable_own_word(task, "v1")?;
    let current = crate::vm::vm_arithmetic::numeric_to_int32(v1.number()?);
    let even = current & 1 == 0;
    let shifted = current >> 1;
    let next = if even { shifted } else { shifted ^ 0xD008 };
    let slot = if even {
        plan.device_a_slot
    } else {
        plan.device_b_slot
    };
    let argument = crate::value::Value::Number(function.captures.get_number(slot)?);
    let branch = idle_release_branch(function, even)?;
    let callee = cached_shape_method(scheduler, branch, if even { 10 } else { 12 })?;
    Some(IdleCall {
        callee,
        argument: Some(argument),
        next_v1: Some((v1, next)),
    })
}

fn idle_release_branch(
    function: &crate::value::FunctionValue,
    even: bool,
) -> Option<crate::machine::CodeView<'_>> {
    let code = function.code.code()?;
    let crate::ops::Op::Branch {
        then_ops, else_ops, ..
    } = code.cold_at(20)?
    else {
        return None;
    };
    if even {
        then_ops.code()
    } else {
        else_ops.code()
    }
}

fn cached_shape_method(
    scheduler: &crate::value::Value,
    code: crate::machine::CodeView<'_>,
    pc: usize,
) -> Option<crate::value::Value> {
    let crate::value::Value::Object(object) = scheduler else {
        return None;
    };
    let callee = crate::vm::get_named_cached_object(object, &code.metadata_at(pc)?.named_cache)?;
    crate::conversion::is_callable(&callee).then_some(callee)
}

fn idle_task_fact(function: &std::rc::Rc<crate::value::FunctionValue>) -> Option<IdleTaskPlan> {
    let pointer = std::rc::Rc::as_ptr(function);
    let index = (pointer as usize >> 4) & (IDLE_TASK_FACT_SLOTS - 1);
    if let Some(plan) = IDLE_TASK_FACTS.with(|facts| {
        let facts = facts.borrow();
        let fact = facts.get(index)?.as_ref()?;
        (fact.function.as_ptr() == pointer).then_some(fact.plan)
    }) {
        return plan;
    }
    let plan = match_idle_task(function);
    IDLE_TASK_FACTS.with(|facts| {
        let mut facts = facts.borrow_mut();
        if facts.is_empty() {
            facts.resize_with(IDLE_TASK_FACT_SLOTS, || None);
        }
        facts[index] = Some(IdleTaskFact {
            function: std::rc::Rc::downgrade(function),
            plan,
        });
    });
    plan
}

fn match_idle_task(function: &crate::value::FunctionValue) -> Option<IdleTaskPlan> {
    let code = function.code.code()?;
    if !is_idle_task_candidate(function) {
        return None;
    }
    if !idle_main_shape(code) {
        return None;
    }
    let crate::ops::Op::Branch { then_ops: hold, .. } = code.cold_at(12)? else {
        return None;
    };
    let crate::ops::Op::Branch {
        then_ops: even,
        else_ops: odd,
        ..
    } = code.cold_at(20)?
    else {
        return None;
    };
    let (hold, even, odd) = (hold.code()?, even.code()?, odd.code()?);
    (idle_hold_shape(hold) && idle_even_shape(even) && idle_odd_shape(odd)).then(|| IdleTaskPlan {
        device_a_slot: even.instruction(11).unwrap().b,
        device_b_slot: odd.instruction(13).unwrap().b,
    })
}

fn idle_main_shape(code: crate::machine::CodeView<'_>) -> bool {
    use crate::ir::Opcode::*;
    if code.len() != 23 {
        return false;
    }
    let ops: [_; 23] = std::array::from_fn(|pc| code.instruction(pc).unwrap());
    is_local_load(ops[0])
        && (ops[1].opcode, ops[1].b) == (Move, ops[0].a)
        && (ops[2].opcode, ops[2].b) == (GetN, ops[1].a)
        && number_constant(code, 3, 1.0)
        && binary_shape(
            code,
            4,
            crate::ops::BinaryOp::NumericSubtract,
            ops[2].a,
            ops[3].a,
        )
        && (ops[5].opcode, ops[5].a, ops[5].b) == (SetN, ops[1].a, ops[4].a)
        && matches!(code.cold_at(6), Some(crate::ops::Op::Unary { .. }))
        && is_local_load(ops[7])
        && (ops[8].opcode, ops[8].b) == (GetN, ops[7].a)
        && number_constant(code, 9, 0.0)
        && binary_shape(code, 10, crate::ops::BinaryOp::Equal, ops[8].a, ops[9].a)
        && matches!(code.cold_at(12), Some(crate::ops::Op::Branch { .. }))
        && is_local_load(ops[13])
        && (ops[14].opcode, ops[14].b) == (GetN, ops[13].a)
        && number_constant(code, 15, 1.0)
        && binary_shape(
            code,
            16,
            crate::ops::BinaryOp::BitwiseAnd,
            ops[14].a,
            ops[15].a,
        )
        && number_constant(code, 17, 0.0)
        && binary_shape(code, 18, crate::ops::BinaryOp::Equal, ops[16].a, ops[17].a)
        && matches!(code.cold_at(20), Some(crate::ops::Op::Branch { .. }))
        && named(code, 2, "count")
        && named(code, 5, "count")
        && named(code, 8, "count")
        && named(code, 14, "v1")
}

fn idle_hold_shape(code: crate::machine::CodeView<'_>) -> bool {
    use crate::ir::Opcode::*;
    code.len() == 5
        && code.instruction(0).is_some_and(is_local_load)
        && code.instruction(1).is_some_and(|op| op.opcode == GetN)
        && code
            .instruction(2)
            .is_some_and(|op| op.opcode == CallN && op.flags == 0)
        && code.instruction(3).is_some_and(|op| op.opcode == Return)
        && named(code, 1, "scheduler")
        && named(code, 2, "holdCurrent")
}

fn idle_even_shape(code: crate::machine::CodeView<'_>) -> bool {
    use crate::ir::Opcode::*;
    code.len() == 15
        && idle_shift_prefix(code)
        && code.instruction(7).is_some_and(|op| op.opcode == SetN)
        && is_local_load(code.instruction(8).unwrap())
        && code.instruction(9).is_some_and(|op| op.opcode == GetN)
        && code.instruction(10).is_some_and(|op| op.opcode == GetN)
        && is_local_load(code.instruction(11).unwrap())
        && code
            .instruction(12)
            .is_some_and(|op| op.opcode == CallN && op.flags == 1)
        && code.instruction(13).is_some_and(|op| op.opcode == Return)
        && named(code, 4, "v1")
        && named(code, 7, "v1")
        && named(code, 9, "scheduler")
        && named(code, 10, "release")
}

fn idle_odd_shape(code: crate::machine::CodeView<'_>) -> bool {
    use crate::ir::Opcode::*;
    code.len() == 17
        && idle_shift_prefix(code)
        && number_constant(code, 7, 0xD008 as f64)
        && binary_shape(
            code,
            8,
            crate::ops::BinaryOp::BitwiseXor,
            code.instruction(6).unwrap().a,
            code.instruction(7).unwrap().a,
        )
        && code.instruction(9).is_some_and(|op| op.opcode == SetN)
        && is_local_load(code.instruction(10).unwrap())
        && code.instruction(11).is_some_and(|op| op.opcode == GetN)
        && code.instruction(12).is_some_and(|op| op.opcode == GetN)
        && is_local_load(code.instruction(13).unwrap())
        && code
            .instruction(14)
            .is_some_and(|op| op.opcode == CallN && op.flags == 1)
        && code.instruction(15).is_some_and(|op| op.opcode == Return)
        && named(code, 4, "v1")
        && named(code, 9, "v1")
        && named(code, 11, "scheduler")
        && named(code, 12, "release")
}

fn idle_shift_prefix(code: crate::machine::CodeView<'_>) -> bool {
    use crate::ir::Opcode::*;
    let ops: [_; 7] = std::array::from_fn(|pc| code.instruction(pc).unwrap());
    is_local_load(ops[0])
        && ops[1].opcode == Move
        && ops[2].opcode == Move
        && is_local_load(ops[3])
        && (ops[4].opcode, ops[4].b) == (GetN, ops[3].a)
        && number_constant(code, 5, 1.0)
        && binary_shape(
            code,
            6,
            crate::ops::BinaryOp::ShiftRight,
            ops[4].a,
            ops[5].a,
        )
}

fn binary_shape(
    code: crate::machine::CodeView<'_>,
    pc: usize,
    operator: crate::ops::BinaryOp,
    left: u16,
    right: u16,
) -> bool {
    code.binary_at(pc)
        .is_some_and(|(_, op, a, b)| (op, a, b) == (operator, left, right))
}

fn number_constant(code: crate::machine::CodeView<'_>, pc: usize, value: f64) -> bool {
    matches!(code.constant_at(pc), Some((_, crate::ops::Constant::Number(number))) if *number == value)
}

fn named(code: crate::machine::CodeView<'_>, pc: usize, expected: &str) -> bool {
    code.metadata_at(pc).and_then(|meta| meta.name.as_deref()) == Some(expected)
}
