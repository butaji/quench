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
    let crate::value::Value::Object(_) = &target else {
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
    let callee = crate::execute::get_property_result(&target, "checkPriorityAdd")?;
    if !crate::conversion::is_callable(&callee) {
        return Ok(None);
    }
    let task = current_tcb.load();

    queue_count.store(crate::value::Value::Number(queue_count_number + 1.0));
    packet_link.store(crate::value::Value::Null);
    packet_id_slot.store(current_id.load());
    let result = crate::functions::execute_target(&callee, &target, &[task, packet_value])?;
    crate::execution_trace::kernel("scheduler_queue_word_slots", false);
    Ok(Some(result))
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
