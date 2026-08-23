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
    let Some(body) = body.ops() else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    let Some(init) = init.ops() else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    let Some(test) = test.ops() else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    let Some(update) = update.ops() else {
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
    init: &[Op],
    test: &[Op],
    body: &[Op],
    update: &[Op],
    config: (bool, u16, &[u16]),
    registers: &mut Vec<crate::value::Value>,
) -> Result<crate::completion::Completion, crate::execute::VmError> {
    let (post_test, dst, per_iteration) = config;
    run_fragment(init, registers)?;
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
    test: &[Op],
    registers: &mut Vec<crate::value::Value>,
) -> Result<bool, crate::execute::VmError> {
    match crate::execute::execute_completion_in_place(test, registers)? {
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
    ops: &[crate::ops::Op],
    registers: &mut Vec<crate::value::Value>,
) -> Result<(), crate::execute::VmError> {
    if ops.is_empty() {
        return Ok(());
    }
    match crate::execute::execute_completion_in_place(ops, registers)? {
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
