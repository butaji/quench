const SCHEDULER_HOLD_FACT_SLOTS: usize = 32;

struct SchedulerHoldFact {
    function: std::rc::Weak<crate::value::FunctionValue>,
    admitted: bool,
}

thread_local! {
    static SCHEDULER_HOLD_FACTS: std::cell::RefCell<Vec<Option<SchedulerHoldFact>>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

#[inline]
fn is_scheduler_hold_candidate(function: &crate::value::FunctionValue) -> bool {
    function.params == 0
        && function.code.capture_slots().len() == 1
        && function.code.code().is_some_and(|code| code.len() == 16)
}

fn execute_scheduler_hold(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    receiver: &crate::value::Value,
) -> Result<Option<crate::value::Value>, crate::execute::VmError> {
    if !scheduler_hold_fact(function) {
        return Ok(None);
    }
    let crate::value::Value::Object(scheduler) = receiver else {
        return Ok(None);
    };
    if scheduler.has_replacement() {
        return Ok(None);
    }
    let Some(hold_count) = writable_own_word(scheduler, "holdCount") else {
        return Ok(None);
    };
    let Some(next_count) = hold_count.number().map(|value| value + 1.0) else {
        return Ok(None);
    };
    let code = function.code.code().unwrap();
    let Some(current) = scheduler_current_tcb(scheduler) else {
        return Ok(None);
    };
    let current_value = crate::value::Value::Object(std::rc::Rc::clone(&current));
    let Some(callee) = cached_shape_method(&current_value, code, 9) else {
        return Ok(None);
    };
    if crate::vm::proven_own_word(&current, "link").is_none() {
        return Ok(None);
    }

    let crate::value::Value::Function(mark) = &callee else {
        return Ok(None);
    };
    let Some((state, next_state)) = state_bitwise_word_transition(
        mark,
        &current,
        crate::ops::BinaryOp::BitwiseOr,
    ) else {
        return Ok(None);
    };
    hold_count.store(crate::value::Value::Number(next_count));
    // SAFETY: admission retains `current`, proves its ordinary own state
    // word, and performs no shape mutation before this store.
    unsafe { &*state }.store(crate::value::Value::Number(next_state));
    let Some(link) = crate::vm::proven_own_word(&current, "link").map(|slot| slot.load()) else {
        return Ok(None);
    };
    crate::execution_trace::kernel("scheduler_hold_word_slots", false);
    Ok(Some(link))
}

fn scheduler_current_tcb(
    scheduler: &crate::value::ObjectData,
) -> Option<std::rc::Rc<crate::value::ObjectData>> {
    let value = crate::vm::proven_own_word(scheduler, "currentTcb")?.load();
    let crate::value::Value::Object(current) = value else {
        return None;
    };
    (!current.has_replacement()).then_some(current)
}

fn scheduler_hold_fact(function: &std::rc::Rc<crate::value::FunctionValue>) -> bool {
    let pointer = std::rc::Rc::as_ptr(function);
    let index = (pointer as usize >> 4) & (SCHEDULER_HOLD_FACT_SLOTS - 1);
    if let Some(admitted) = SCHEDULER_HOLD_FACTS.with(|facts| {
        let facts = facts.borrow();
        let fact = facts.get(index)?.as_ref()?;
        (fact.function.as_ptr() == pointer).then_some(fact.admitted)
    }) {
        return admitted;
    }
    let admitted = match_scheduler_hold(function);
    SCHEDULER_HOLD_FACTS.with(|facts| {
        let mut facts = facts.borrow_mut();
        if facts.is_empty() {
            facts.resize_with(SCHEDULER_HOLD_FACT_SLOTS, || None);
        }
        facts[index] = Some(SchedulerHoldFact {
            function: std::rc::Rc::downgrade(function),
            admitted,
        });
    });
    admitted
}

fn match_scheduler_hold(function: &crate::value::FunctionValue) -> bool {
    let Some(code) = function.code.code() else {
        return false;
    };
    if !is_scheduler_hold_candidate(function) {
        return false;
    }
    let ops: [_; 16] = std::array::from_fn(|pc| code.instruction(pc).unwrap());
    use crate::ir::Opcode::*;
    is_local_load(ops[0])
        && (ops[1].opcode, ops[1].b) == (Move, ops[0].a)
        && (ops[2].opcode, ops[2].b) == (GetN, ops[1].a)
        && number_constant(code, 3, 1.0)
        && binary_shape(
            code,
            4,
            crate::ops::BinaryOp::NumericAdd,
            ops[2].a,
            ops[3].a,
        )
        && (ops[5].opcode, ops[5].a, ops[5].b) == (SetN, ops[1].a, ops[4].a)
        && matches!(code.cold_at(6), Some(crate::ops::Op::Unary { .. }))
        && is_local_load(ops[7])
        && (ops[8].opcode, ops[8].b) == (GetN, ops[7].a)
        && ops[9].opcode == CallN
        && ops[9].flags == 0
        && ops[9].b == ops[8].a
        && is_local_load(ops[10])
        && (ops[11].opcode, ops[11].b) == (GetN, ops[10].a)
        && (ops[12].opcode, ops[12].b) == (GetN, ops[11].a)
        && (ops[13].opcode, ops[13].a) == (Return, ops[12].a)
        && ops[14].opcode == LoadConst
        && (ops[15].opcode, ops[15].a) == (Return, ops[14].a)
        && named(code, 2, "holdCount")
        && named(code, 5, "holdCount")
        && named(code, 8, "currentTcb")
        && named(code, 9, "markAsHeld")
        && named(code, 11, "currentTcb")
        && named(code, 12, "link")
}
