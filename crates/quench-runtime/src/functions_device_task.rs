const DEVICE_TASK_FACT_SLOTS: usize = 64;

struct DeviceTaskFact {
    function: std::rc::Weak<crate::value::FunctionValue>,
    admitted: bool,
}

thread_local! {
    static DEVICE_TASK_FACTS: std::cell::RefCell<Vec<Option<DeviceTaskFact>>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

#[inline]
fn is_device_task_candidate(function: &crate::value::FunctionValue) -> bool {
    function.params == 1
        && function.code.capture_slots().len() == 3
        && function.code.code().is_some_and(|code| code.len() == 7)
}

fn execute_device_task(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    receiver: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Result<Option<crate::value::Value>, crate::execute::VmError> {
    if !device_task_fact(function) {
        return Ok(None);
    }
    let crate::value::Value::Object(task) = receiver else {
        return Ok(None);
    };
    if task.has_replacement() {
        return Ok(None);
    }
    let Some(v1) = writable_own_word(task, "v1") else {
        return Ok(None);
    };
    let Some(scheduler) = task_scheduler(task) else {
        return Ok(None);
    };
    execute_device_task_words(function, arguments, v1, &scheduler)
}

fn execute_device_task_words(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    arguments: &[crate::value::Value],
    v1: &crate::register_file::SlotWord,
    scheduler: &crate::value::Value,
) -> Result<Option<crate::value::Value>, crate::execute::VmError> {
    let packet = arguments
        .first()
        .cloned()
        .unwrap_or(crate::value::Value::Undefined);
    let Some(call) = device_task_call(function, scheduler, v1, &packet) else {
        return Ok(None);
    };

    let result = call.execute(v1, scheduler)?;
    crate::execution_trace::kernel("device_task_run_word_slots", false);
    Ok(Some(result))
}

enum DeviceTaskCall {
    Suspend(SchedulerSuspendWordTransition),
    Invoke {
        callee: crate::value::Value,
        argument: Option<crate::value::Value>,
        store: Option<crate::value::Value>,
    },
}

impl DeviceTaskCall {
    fn execute(
        self,
        v1: &crate::register_file::SlotWord,
        scheduler: &crate::value::Value,
    ) -> Result<crate::value::Value, crate::execute::VmError> {
        match self {
            Self::Suspend(transition) => {
                crate::execution_trace::kernel("device_suspend_word_transition", false);
                Ok(transition.execute())
            }
            Self::Invoke {
                callee,
                argument,
                store,
            } => {
                if let Some(value) = store {
                    v1.store(value);
                }
                match argument {
                    Some(argument) => {
                        crate::functions::execute_target(&callee, scheduler, &[argument])
                    }
                    None => crate::functions::execute_target(&callee, scheduler, &[]),
                }
            }
        }
    }
}

fn device_task_call(
    function: &crate::value::FunctionValue,
    scheduler: &crate::value::Value,
    v1: &crate::register_file::SlotWord,
    packet: &crate::value::Value,
) -> Option<DeviceTaskCall> {
    let code = function.code.code()?;
    let crate::ops::Op::Branch {
        then_ops, else_ops, ..
    } = code.cold_at(4)?
    else {
        return None;
    };
    if !packet.is_nullish() {
        return device_hold_call(scheduler, else_ops.code()?, packet.clone());
    }
    let queued = v1.load();
    if queued.is_nullish() {
        device_suspend_call(scheduler, then_ops.code()?)
    } else {
        device_queue_call(scheduler, then_ops.code()?, queued)
    }
}

fn device_suspend_call(
    scheduler: &crate::value::Value,
    code: crate::machine::CodeView<'_>,
) -> Option<DeviceTaskCall> {
    let crate::ops::Op::Branch { then_ops, .. } = code.cold_at(5)? else {
        return None;
    };
    let callee = cached_shape_method(scheduler, then_ops.code()?, 2)?;
    Some(DeviceTaskCall::Suspend(scheduler_suspend_word_transition(
        &callee, scheduler,
    )?))
}

fn device_queue_call(
    scheduler: &crate::value::Value,
    code: crate::machine::CodeView<'_>,
    packet: crate::value::Value,
) -> Option<DeviceTaskCall> {
    let callee = cached_shape_method(scheduler, code, 16)?;
    Some(DeviceTaskCall::Invoke {
        callee,
        argument: Some(packet),
        store: Some(crate::value::Value::Null),
    })
}

fn device_hold_call(
    scheduler: &crate::value::Value,
    code: crate::machine::CodeView<'_>,
    packet: crate::value::Value,
) -> Option<DeviceTaskCall> {
    let callee = cached_shape_method(scheduler, code, 7)?;
    Some(DeviceTaskCall::Invoke {
        callee,
        argument: None,
        store: Some(packet),
    })
}

fn device_task_fact(function: &std::rc::Rc<crate::value::FunctionValue>) -> bool {
    let pointer = std::rc::Rc::as_ptr(function);
    let index = (pointer as usize >> 4) & (DEVICE_TASK_FACT_SLOTS - 1);
    if let Some(admitted) = DEVICE_TASK_FACTS.with(|facts| {
        let facts = facts.borrow();
        let fact = facts.get(index)?.as_ref()?;
        (fact.function.as_ptr() == pointer).then_some(fact.admitted)
    }) {
        return admitted;
    }
    let admitted = match_device_task(function);
    DEVICE_TASK_FACTS.with(|facts| {
        let mut facts = facts.borrow_mut();
        if facts.is_empty() {
            facts.resize_with(DEVICE_TASK_FACT_SLOTS, || None);
        }
        facts[index] = Some(DeviceTaskFact {
            function: std::rc::Rc::downgrade(function),
            admitted,
        });
    });
    admitted
}

fn match_device_task(function: &crate::value::FunctionValue) -> bool {
    let Some(code) = function.code.code() else {
        return false;
    };
    if !is_device_task_candidate(function) || !device_main_shape(code) {
        return false;
    }
    let Some(crate::ops::Op::Branch {
        then_ops, else_ops, ..
    }) = code.cold_at(4)
    else {
        return false;
    };
    let (Some(then_code), Some(else_code)) = (then_ops.code(), else_ops.code()) else {
        return false;
    };
    device_null_shape(then_code) && device_packet_shape(else_code)
}

fn device_main_shape(code: crate::machine::CodeView<'_>) -> bool {
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

fn device_null_shape(code: crate::machine::CodeView<'_>) -> bool {
    use crate::ir::Opcode::*;
    if code.len() != 21 {
        return false;
    }
    let ops: [_; 21] = std::array::from_fn(|pc| code.instruction(pc).unwrap());
    is_local_load(ops[0])
        && (ops[1].opcode, ops[1].b) == (GetN, ops[0].a)
        && matches!(code.constant_at(2), Some((_, crate::ops::Constant::Null)))
        && binary_shape(code, 3, crate::ops::BinaryOp::Equal, ops[1].a, ops[2].a)
        && matches!(code.cold_at(5), Some(crate::ops::Op::Branch { .. }))
        && is_local_load(ops[6])
        && (ops[7].opcode, ops[7].b) == (GetN, ops[6].a)
        && ops[8].opcode == InitLocal
        && is_local_load(ops[9])
        && ops[10].opcode == Move
        && ops[11].opcode == Move
        && matches!(code.constant_at(12), Some((_, crate::ops::Constant::Null)))
        && (ops[13].opcode, ops[13].a, ops[13].b) == (SetN, ops[11].a, ops[12].a)
        && is_local_load(ops[14])
        && (ops[15].opcode, ops[15].b) == (GetN, ops[14].a)
        && (ops[16].opcode, ops[16].b) == (GetN, ops[15].a)
        && is_local_load(ops[17])
        && ops[18].opcode == CallN
        && ops[18].flags == 1
        && ops[19].opcode == Return
        && named(code, 1, "v1")
        && named(code, 7, "v1")
        && named(code, 13, "v1")
        && named(code, 15, "scheduler")
        && named(code, 16, "queue")
        && device_suspend_shape(code.cold_at(5))
}

fn device_suspend_shape(op: Option<&crate::ops::Op>) -> bool {
    let Some(crate::ops::Op::Branch {
        then_ops, else_ops, ..
    }) = op
    else {
        return false;
    };
    let (Some(then_code), Some(else_code)) = (then_ops.code(), else_ops.code()) else {
        return false;
    };
    then_code.len() == 5
        && else_code.is_empty()
        && then_code.instruction(0).is_some_and(is_local_load)
        && then_code
            .instruction(1)
            .is_some_and(|op| op.opcode == crate::ir::Opcode::GetN)
        && then_code
            .instruction(2)
            .is_some_and(|op| op.opcode == crate::ir::Opcode::CallN && op.flags == 0)
        && named(then_code, 1, "scheduler")
        && named(then_code, 2, "suspendCurrent")
}

fn device_packet_shape(code: crate::machine::CodeView<'_>) -> bool {
    use crate::ir::Opcode::*;
    if code.len() != 10 {
        return false;
    }
    let ops: [_; 10] = std::array::from_fn(|pc| code.instruction(pc).unwrap());
    is_local_load(ops[0])
        && ops[1].opcode == Move
        && ops[2].opcode == Move
        && is_local_load(ops[3])
        && (ops[4].opcode, ops[4].a, ops[4].b) == (SetN, ops[2].a, ops[3].a)
        && is_local_load(ops[5])
        && (ops[6].opcode, ops[6].b) == (GetN, ops[5].a)
        && ops[7].opcode == CallN
        && ops[7].flags == 0
        && ops[8].opcode == Return
        && named(code, 4, "v1")
        && named(code, 6, "scheduler")
        && named(code, 7, "holdCurrent")
}
