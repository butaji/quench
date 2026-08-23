pub(crate) fn execute(
    registers: &mut Vec<crate::value::Value>,
    op: &crate::ops::Op,
) -> Result<crate::completion::Completion, crate::execute::VmError> {
    let (label, init, test, body, update, post_test, dst, per_iteration) = match op {
        crate::ops::Op::Loop {
            label,
            init,
            test,
            body,
            update,
            post_test,
            dst,
            per_iteration,
        } => (
            label,
            init,
            test,
            body,
            update,
            post_test,
            *dst,
            per_iteration.as_slice(),
        ),
        _ => return Err(crate::execute::VmError::MissingReturn),
    };
    let Some(body) = body.code() else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    let Some(init) = init.code() else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    let Some(test) = test.code() else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    let Some(update) = update.code() else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    run_loop(
        label,
        init,
        test,
        body,
        update,
        (*post_test, dst, per_iteration),
        registers,
    )
}

fn run_loop(
    label: &Option<String>,
    init: crate::machine::CodeView<'_>,
    test: crate::machine::CodeView<'_>,
    body: crate::machine::CodeView<'_>,
    update: crate::machine::CodeView<'_>,
    config: (bool, u16, &[u16]),
    registers: &mut Vec<crate::value::Value>,
) -> Result<crate::completion::Completion, crate::execute::VmError> {
    let (post_test, dst, per_iteration) = config;
    run_fragment(init, registers)?;
    if label.is_none() && !post_test && per_iteration.is_empty() {
        if let Some(fact) = CountedForFact::recognize(test, update) {
            if let Some(completion) = run_counted_for(fact, body, update, dst, registers)? {
                return Ok(completion);
            }
        }
    }
    refresh_per_iteration(per_iteration);
    loop {
        if !post_test && !loop_test(test, registers)? {
            break;
        }
        crate::execute::write_value(registers, dst, crate::value::Value::Undefined);
        match execute_loop_body(registers, label, body)? {
            crate::completion::LoopTransition::Continue(value) => {
                store_loop_value(registers, dst, value)?;
            }
            crate::completion::LoopTransition::Break(value) => {
                store_loop_value(registers, dst, value)?;
                break;
            }
            crate::completion::LoopTransition::Propagate(completion) => {
                return update_empty_from(registers, dst, completion);
            }
        }
        refresh_per_iteration(per_iteration);
        run_fragment(update, registers)?;
        if post_test && !loop_test(test, registers)? {
            break;
        }
    }
    Ok(crate::completion::Completion::Normal)
}

fn run_counted_for(
    fact: CountedForFact,
    body: crate::machine::CodeView<'_>,
    update: crate::machine::CodeView<'_>,
    dst: u16,
    registers: &mut Vec<crate::value::Value>,
) -> Result<Option<crate::completion::Completion>, crate::execute::VmError> {
    let environment = crate::locals::current();
    loop {
        let crate::value::Value::Number(index) = environment.get(fact.slot) else {
            return Ok(None);
        };
        if !counted_comparison(fact.comparison, index, fact.bound) {
            return Ok(Some(crate::completion::Completion::Normal));
        }
        match execute_loop_body(registers, &None, body)? {
            crate::completion::LoopTransition::Continue(value) => {
                store_loop_value(registers, dst, value)?;
            }
            crate::completion::LoopTransition::Break(value) => {
                store_loop_value(registers, dst, value)?;
                return Ok(Some(crate::completion::Completion::Normal));
            }
            crate::completion::LoopTransition::Propagate(completion) => {
                return update_empty_from(registers, dst, completion).map(Some);
            }
        }
        let crate::value::Value::Number(index) = environment.get(fact.slot) else {
            run_fragment(update, registers)?;
            return Ok(None);
        };
        crate::locals::write(fact.slot, crate::value::Value::Number(index + fact.step));
    }
}

macro_rules! counted_comparisons {
    ($($variant:ident => $operator:tt),+ $(,)?) => {
        fn counted_comparison(operator: crate::ops::BinaryOp, lhs: f64, rhs: f64) -> bool {
            match operator {
                $(crate::ops::BinaryOp::$variant => lhs $operator rhs,)+
                _ => false,
            }
        }
    };
}

counted_comparisons! {
    LessThan => <,
    LessEqual => <=,
    GreaterThan => >,
    GreaterEqual => >=,
}

#[derive(Clone, Copy)]
struct CountedForFact {
    slot: u16,
    bound: f64,
    comparison: crate::ops::BinaryOp,
    step: f64,
}

impl CountedForFact {
    fn recognize(
        test: crate::machine::CodeView<'_>,
        update: crate::machine::CodeView<'_>,
    ) -> Option<Self> {
        if test.len() != 4 {
            return None;
        }
        let Op::LoadBinding { dst: index, slot, dynamic: false, .. } = test.cold_at(0)? else { return None };
        let Op::Const { dst: bound_register, value: crate::ops::Constant::Number(bound) } = test.cold_at(1)? else { return None };
        let Op::Binary { dst: condition, operator: comparison, lhs, rhs } = test.cold_at(2)? else { return None };
        let returned = test.instruction(3)?;
        if returned.opcode != crate::ir::Opcode::Return || returned.a != *condition {
            return None;
        }
        if lhs != index || rhs != bound_register {
            return None;
        }
        let step = recognize_counted_update(update, *slot)?;
        Some(Self { slot: *slot, bound: *bound, comparison: *comparison, step })
    }
}

fn recognize_counted_update(update: crate::machine::CodeView<'_>, slot: u16) -> Option<f64> {
    let checked = match update.len() {
        5 => false,
        6 => matches!(update.cold_at(3), Some(Op::CheckInitialized { slot: checked, .. }) if *checked == slot),
        _ => return None,
    };
    if update.len() == 6 && !checked {
        return None;
    }
    let load = update.instruction(0)?;
    if load.opcode != crate::ir::Opcode::LoadLocal || load.b != slot {
        return None;
    }
    let Op::Const { dst: step_register, value: crate::ops::Constant::Number(step) } = update.cold_at(1)? else { return None };
    let Op::Binary { dst: next, operator: crate::ops::BinaryOp::NumericAdd, lhs, rhs } = update.cold_at(2)? else { return None };
    let store_pc = if checked { 4 } else { 3 };
    let Op::StoreLocal { slot: stored_slot, src: stored } = update.cold_at(store_pc)? else { return None };
    let returned = update.instruction(store_pc + 1)?;
    (*stored_slot == slot && load.a == *lhs && *step_register == *rhs
        && *next == *stored && returned.opcode == crate::ir::Opcode::Return && returned.a == *next)
        .then_some(*step)
}

fn store_loop_value(
    registers: &mut Vec<crate::value::Value>,
    dst: u16,
    value: Option<crate::value::Value>,
) -> Result<(), crate::execute::VmError> {
    let Some(value) = value else {
        return Ok(());
    };
    crate::execute::write_value(registers, dst, value);
    Ok(())
}

fn update_empty_from(
    registers: &[crate::value::Value],
    dst: u16,
    completion: crate::completion::Completion,
) -> Result<crate::completion::Completion, crate::execute::VmError> {
    let value = crate::execute::read_register(registers, dst)?;
    Ok(completion.update_empty(value))
}

fn loop_test(
    test: crate::machine::CodeView<'_>,
    registers: &mut Vec<crate::value::Value>,
) -> Result<bool, crate::execute::VmError> {
    match crate::vm::execute_code_completion_in_current_frame(test, registers)? {
        crate::completion::Completion::Return(value) => Ok(crate::execute::is_truthy(&value)),
        crate::completion::Completion::Normal => Ok(false),
        completion => completion
            .into_vm_error()
            .map(|value| crate::execute::is_truthy(&value)),
    }
}

/// Run a loop fragment. An empty fragment (no init/update, e.g. a `while`
/// loop) is a no-op; a non-empty fragment must return normally.
fn run_fragment(
    ops: crate::machine::CodeView<'_>,
    registers: &mut Vec<crate::value::Value>,
) -> Result<(), crate::execute::VmError> {
    if ops.is_empty() {
        return Ok(());
    }
    match crate::vm::execute_code_completion_in_current_frame(ops, registers)? {
        // Loop fragments use Return as their local value carrier. They are
        // not function boundaries, so consume that marker while preserving
        // the current lexical environment for the next fragment.
        crate::completion::Completion::Normal | crate::completion::Completion::Return(_) => Ok(()),
        completion => completion.into_vm_error().map(|_| ()),
    }
}

fn refresh_per_iteration(slots: &[u16]) {
    let environment = crate::locals::current();
    for &slot in slots {
        let value = environment.get(slot);
        let _ = environment.replace_slot(slot, value);
    }
}
