const SCHEDULER_QUEUE_FACT_SLOTS: usize = 64;

#[derive(Clone)]
struct SchedulerQueueFact {
    function: std::rc::Weak<crate::value::FunctionValue>,
    admitted: bool,
}

thread_local! {
    static SCHEDULER_QUEUE_FACTS: std::cell::RefCell<Vec<Option<SchedulerQueueFact>>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

#[inline]
fn is_scheduler_queue_candidate(function: &crate::value::FunctionValue) -> bool {
    function.params == 1 && function.code.code().is_some_and(|code| code.len() == 38)
}

fn execute_scheduler_queue(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    receiver: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Result<Option<crate::value::Value>, crate::execute::VmError> {
    if !scheduler_queue_fact(function) {
        return Ok(None);
    }
    let crate::value::Value::Object(scheduler) = receiver else {
        return Ok(None);
    };
    let Some(packet_value) = arguments.first() else {
        return Ok(None);
    };
    let packet_value = crate::locals::resolved_replacement(packet_value.clone());
    let crate::value::Value::Object(packet) = &packet_value else {
        return Ok(None);
    };
    if scheduler.has_replacement() || packet.has_replacement() {
        return Ok(None);
    }

    let Some(blocks) = crate::vm::proven_own_word(scheduler, "blocks") else {
        return Ok(None);
    };
    let Some(packet_id) = crate::vm::proven_own_word(packet, "id").and_then(|word| word.number())
    else {
        return Ok(None);
    };
    if !packet_id.is_finite()
        || packet_id.fract() != 0.0
        || packet_id < 0.0
        || packet_id > usize::MAX as f64
    {
        return Ok(None);
    }
    let blocks = blocks.load();
    let crate::value::Value::Array(blocks) = crate::locals::resolved_replacement(blocks) else {
        return Ok(None);
    };
    if !crate::locals::array_word_is_current(&blocks) {
        return Ok(None);
    }
    let Some(target) = blocks.dense_value_at(packet_id as usize) else {
        return Ok(None);
    };
    if matches!(
        target,
        crate::value::Value::Null | crate::value::Value::Undefined
    ) {
        return Ok(Some(target));
    }
    let target = crate::locals::resolved_replacement(target);
    let crate::value::Value::Object(target_object) = &target else {
        return Ok(None);
    };

    let Some(queue_count) = writable_own_word(scheduler, "queueCount") else {
        return Ok(None);
    };
    let Some(queue_count_number) = queue_count.number() else {
        return Ok(None);
    };
    let Some(packet_link) = writable_own_word(packet, "link") else {
        return Ok(None);
    };
    let Some(packet_id_slot) = writable_own_word(packet, "id") else {
        return Ok(None);
    };
    let Some(current_id) = crate::vm::proven_own_word(scheduler, "currentId") else {
        return Ok(None);
    };
    let Some(current_tcb) = crate::vm::proven_own_word(scheduler, "currentTcb") else {
        return Ok(None);
    };
    let Some(code) = function.code.code() else {
        return Ok(None);
    };
    let Some(check_priority_metadata) = code.metadata_at(30) else {
        return Ok(None);
    };
    let Some(callee) =
        crate::vm::get_named_cached_object(target_object, &check_priority_metadata.named_cache)
    else {
        return Ok(None);
    };
    let crate::value::Value::Function(check_priority_add) = &callee else {
        return Ok(None);
    };
    let task = current_tcb.load();

    let Some(transition) =
        check_priority_transition(check_priority_add, target_object, &target, &task, packet)?
    else {
        return Ok(None);
    };

    queue_count.store(crate::value::Value::Number(queue_count_number + 1.0));
    packet_link.store(crate::value::Value::Null);
    packet_id_slot.store(current_id.load());
    let result = transition.apply(packet_link, packet_value.clone());
    crate::execution_trace::kernel("scheduler_queue_check_priority_word_slots", false);
    Ok(Some(result))
}

enum CheckPriorityTransition<R> {
    Empty {
        queue: *const crate::register_file::SlotWord,
        state: *const crate::register_file::SlotWord,
        next_state: f64,
        result: R,
    },
    Append {
        queue: *const crate::register_file::SlotWord,
        tail_link: *const crate::register_file::SlotWord,
        head: crate::value::Value,
        result: R,
    },
}

impl<R> CheckPriorityTransition<R> {
    fn apply(
        self,
        packet_link: &crate::register_file::SlotWord,
        packet: crate::value::Value,
    ) -> R {
        match self {
            Self::Empty {
                queue,
                state,
                next_state,
                result,
            } => {
                // SAFETY: admission owns `target` and proved both slots before
                // any mutation. Word stores cannot move the object's storage.
                unsafe { &*queue }.store(packet);
                unsafe { &*state }.store(crate::value::Value::Number(next_state));
                result
            }
            Self::Append {
                queue,
                tail_link,
                head,
                result,
            } => {
                // `Scheduler.queue` has already established packet.link=null;
                // the admitted Packet.addTo body would repeat that same store.
                packet_link.store(crate::value::Value::Null);
                unsafe { &*tail_link }.store(packet);
                unsafe { &*queue }.store(head);
                result
            }
        }
    }
}

fn check_priority_transition(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    target: &crate::value::ObjectData,
    target_value: &crate::value::Value,
    task_value: &crate::value::Value,
    packet: &crate::value::ObjectData,
) -> Result<Option<CheckPriorityTransition<crate::value::Value>>, crate::execute::VmError> {
    let Some((ready, occupied)) = match_check_priority_add(function) else {
        return Ok(None);
    };
    let Some(queue) = writable_own_word(target, "queue") else {
        return Ok(None);
    };
    let head = queue.load();
    if head.is_nullish() {
        return Ok(check_priority_empty(
            ready,
            target,
            target_value,
            task_value,
            queue,
        ));
    }
    check_priority_append(occupied, packet, task_value, queue, head)
}

fn check_priority_append(
    occupied: crate::machine::CodeView<'_>,
    packet: &crate::value::ObjectData,
    task: &crate::value::Value,
    queue: &crate::register_file::SlotWord,
    head: crate::value::Value,
) -> Result<Option<CheckPriorityTransition<crate::value::Value>>, crate::execute::VmError> {
    let crate::value::Value::Object(head_object) = &head else {
        return Ok(None);
    };
    let Some(add_to_metadata) = occupied.metadata_at(4) else {
        return Ok(None);
    };
    let Some(add_to) = crate::vm::get_named_cached_object(packet, &add_to_metadata.named_cache)
    else {
        return Ok(None);
    };
    let crate::value::Value::Function(add_to) = add_to else {
        return Ok(None);
    };
    if !packet_add_fact(&add_to) {
        return Ok(None);
    }
    let Some(tail_link) =
        packet_tail_link(std::rc::Rc::as_ptr(head_object), std::ptr::from_ref(packet))
    else {
        return Ok(None);
    };
    Ok(Some(CheckPriorityTransition::Append {
        queue: std::ptr::from_ref(queue),
        tail_link,
        head,
        result: task.clone(),
    }))
}

fn check_priority_empty(
    ready: crate::machine::CodeView<'_>,
    target: &crate::value::ObjectData,
    target_value: &crate::value::Value,
    task_value: &crate::value::Value,
    queue: &crate::register_file::SlotWord,
) -> Option<CheckPriorityTransition<crate::value::Value>> {
    let crate::value::Value::Object(task) = task_value else {
        return None;
    };
    if task.has_replacement() {
        return None;
    }
    let (state, next_state) = runnable_state_transition(ready, target)?;
    let target_priority = crate::vm::proven_own_word(target, "priority")?.number()?;
    let task_priority = crate::vm::proven_own_word(task, "priority")?.number()?;
    let result = if target_priority > task_priority {
        target_value.clone()
    } else {
        task_value.clone()
    };
    Some(CheckPriorityTransition::Empty {
        queue: std::ptr::from_ref(queue),
        state,
        next_state,
        result,
    })
}

fn runnable_state_transition(
    ready: crate::machine::CodeView<'_>,
    target: &crate::value::ObjectData,
) -> Option<(*const crate::register_file::SlotWord, f64)> {
    let mark = crate::vm::get_named_cached_object(target, &ready.metadata_at(6)?.named_cache)?;
    let crate::value::Value::Function(mark) = mark else {
        return None;
    };
    state_bitwise_word_transition(&mark, target, crate::ops::BinaryOp::BitwiseOr)
}

fn match_check_priority_add(
    function: &crate::value::FunctionValue,
) -> Option<(crate::machine::CodeView<'_>, crate::machine::CodeView<'_>)> {
    let code = function.code.code()?;
    if function.params != 2 || code.len() != 10 || !named(code, 1, "queue") {
        return None;
    }
    let crate::ops::Op::Branch {
        then_ops, else_ops, ..
    } = code.cold_at(5)?
    else {
        return None;
    };
    let ready = then_ops.code()?;
    let occupied = else_ops.code()?;
    let crate::ops::Op::Branch { .. } = ready.cold_at(13)? else {
        return None;
    };
    (ready.len() == 15
        && occupied.len() == 10
        && named(ready, 4, "queue")
        && named(ready, 6, "markAsRunnable")
        && named(ready, 8, "priority")
        && named(ready, 10, "priority")
        && named(occupied, 4, "addTo")
        && named(occupied, 6, "queue")
        && named(occupied, 8, "queue"))
    .then_some((ready, occupied))
}

fn scheduler_queue_fact(function: &std::rc::Rc<crate::value::FunctionValue>) -> bool {
    let pointer = std::rc::Rc::as_ptr(function);
    let index = (pointer as usize >> 4) & (SCHEDULER_QUEUE_FACT_SLOTS - 1);
    if let Some(admitted) = SCHEDULER_QUEUE_FACTS.with(|facts| {
        let facts = facts.borrow();
        let fact = facts.get(index)?.as_ref()?;
        (fact.function.as_ptr() == pointer).then_some(fact.admitted)
    }) {
        return admitted;
    }
    let admitted = match_scheduler_queue(function);
    SCHEDULER_QUEUE_FACTS.with(|facts| {
        let mut facts = facts.borrow_mut();
        if facts.is_empty() {
            facts.resize_with(SCHEDULER_QUEUE_FACT_SLOTS, || None);
        }
        facts[index] = Some(SchedulerQueueFact {
            function: std::rc::Rc::downgrade(function),
            admitted,
        });
    });
    admitted
}

fn match_scheduler_queue(function: &crate::value::FunctionValue) -> bool {
    let Some(code) = function.code.code() else {
        return false;
    };
    if function.params != 1 || code.len() != 38 {
        return false;
    }
    use crate::ir::Opcode::*;
    let ops: [_; 38] = std::array::from_fn(|pc| code.instruction(pc).unwrap());
    is_local_load(ops[0])
        && (ops[1].opcode, ops[1].b) == (GetN, ops[0].a)
        && is_local_load(ops[2])
        && (ops[3].opcode, ops[3].b) == (GetN, ops[2].a)
        && (ops[4].opcode, ops[4].b, ops[4].c) == (AGetI, ops[1].a, ops[3].a)
        && ops[5].opcode == InitLocal
        && is_local_load(ops[11])
        && ops[12].opcode == Move
        && (ops[13].opcode, ops[13].b) == (GetN, ops[12].a)
        && ops[15].opcode == Binary
        && (ops[16].opcode, ops[16].a, ops[16].b) == (SetN, ops[12].a, ops[15].a)
        && is_local_load(ops[18])
        && ops[19].opcode == Move
        && ops[20].opcode == Move
        && (ops[22].opcode, ops[22].a) == (SetN, ops[20].a)
        && is_local_load(ops[23])
        && ops[24].opcode == Move
        && ops[25].opcode == Move
        && is_local_load(ops[26])
        && (ops[27].opcode, ops[27].b) == (GetN, ops[26].a)
        && (ops[28].opcode, ops[28].a, ops[28].b) == (SetN, ops[25].a, ops[27].a)
        && is_local_load(ops[29])
        && (ops[30].opcode, ops[30].b) == (GetN, ops[29].a)
        && is_local_load(ops[31])
        && (ops[32].opcode, ops[32].b) == (GetN, ops[31].a)
        && is_local_load(ops[33])
        && (ops[34].opcode, ops[34].flags, ops[34].b, ops[34].c) == (CallN, 2, ops[29].a, ops[30].a)
        && (ops[35].opcode, ops[35].a) == (Return, ops[34].a)
        && code.metadata_at(1).and_then(|meta| meta.name.as_deref()) == Some("blocks")
        && code.metadata_at(3).and_then(|meta| meta.name.as_deref()) == Some("id")
        && code.metadata_at(13).and_then(|meta| meta.name.as_deref()) == Some("queueCount")
        && code.metadata_at(16).and_then(|meta| meta.name.as_deref()) == Some("queueCount")
        && code.metadata_at(22).and_then(|meta| meta.name.as_deref()) == Some("link")
        && code.metadata_at(27).and_then(|meta| meta.name.as_deref()) == Some("currentId")
        && code.metadata_at(28).and_then(|meta| meta.name.as_deref()) == Some("id")
        && code.metadata_at(30).and_then(|meta| meta.name.as_deref()) == Some("checkPriorityAdd")
        && code.metadata_at(32).and_then(|meta| meta.name.as_deref()) == Some("currentTcb")
}
