// Ordinary loop execution. Reduction describes the loop and the VM executes
// its fragments with the same completion machinery used by every other code
// path. There is no source-shape or benchmark recognizer in this module.

pub(crate) fn execute(
    registers: &mut crate::register_file::RegisterFile,
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
            *post_test,
            *dst,
            per_iteration.as_slice(),
        ),
        _ => return Err(crate::execute::VmError::MissingReturn),
    };
    let body_code = body.code().ok_or(crate::execute::VmError::MissingReturn)?;
    let init_code = init.code().ok_or(crate::execute::VmError::MissingReturn)?;
    let test_code = test.code().ok_or(crate::execute::VmError::MissingReturn)?;
    let update_code = update.code().ok_or(crate::execute::VmError::MissingReturn)?;
    run_loop(
        label,
        init_code,
        test_code,
        body_code,
        update_code,
        (init, test, body, update),
        (post_test, dst, per_iteration),
        registers,
    )
}

fn run_loop(
    label: &Option<String>,
    init: crate::machine::CodeView<'_>,
    test: crate::machine::CodeView<'_>,
    body: crate::machine::CodeView<'_>,
    update: crate::machine::CodeView<'_>,
    owners: (
        &crate::machine::FunctionCode,
        &crate::machine::FunctionCode,
        &crate::machine::FunctionCode,
        &crate::machine::FunctionCode,
    ),
    config: (bool, u16, &[u16]),
    registers: &mut crate::register_file::RegisterFile,
) -> Result<crate::completion::Completion, crate::execute::VmError> {
    stacker::maybe_grow(64 * 1024 * 1024, 256 * 1024 * 1024, || {
        run_loop_inner(label, init, test, body, update, owners, config, registers)
    })
}

fn run_loop_inner(
    label: &Option<String>,
    init: crate::machine::CodeView<'_>,
    test: crate::machine::CodeView<'_>,
    body: crate::machine::CodeView<'_>,
    update: crate::machine::CodeView<'_>,
    owners: (
        &crate::machine::FunctionCode,
        &crate::machine::FunctionCode,
        &crate::machine::FunctionCode,
        &crate::machine::FunctionCode,
    ),
    config: (bool, u16, &[u16]),
    registers: &mut crate::register_file::RegisterFile,
) -> Result<crate::completion::Completion, crate::execute::VmError> {
    crate::execution_trace::event(crate::execution_trace::Event::LoopEntry);
    let loop_shape = crate::execution_trace::loop_shape(body);
    let (post_test, dst, per_iteration) = config;
    let (init_owner, test_owner, body_owner, update_owner) = owners;
    let _ = init_owner.enter_invocation();
    run_fragment(init, init_owner, registers)?;
    refresh_per_iteration(per_iteration);
    loop {
        crate::execution_trace::event(crate::execution_trace::Event::LoopIteration);
        crate::execution_trace::loop_shape_iteration(loop_shape);
        let _ = test_owner.enter_invocation();
        if !post_test && !loop_test(test, test_owner, registers)? {
            break;
        }
        crate::execute::write_value(registers, dst, crate::value::Value::Undefined);
        let _ = body_owner.enter_invocation();
        let (transition, body_next, suspension_slot) =
            execute_loop_body_step_with_owner(registers, label, body, body_owner)?;
        match transition {
            crate::completion::LoopTransition::Continue(value) => {
                store_loop_value(registers, dst, value)?;
            }
            crate::completion::LoopTransition::Break(value) => {
                store_loop_value(registers, dst, value)?;
                break;
            }
            crate::completion::LoopTransition::Propagate(completion) => {
                let completion = loop_suspension(
                    completion,
                    promise_loop_point(
                        body,
                        body_next,
                        suspension_slot,
                        label,
                        test,
                        update,
                        dst,
                        post_test,
                    ),
                );
                return update_empty_from(registers, dst, completion);
            }
        }
        refresh_per_iteration(per_iteration);
        let _ = update_owner.enter_invocation();
        run_fragment(update, update_owner, registers)?;
        let _ = test_owner.enter_invocation();
        if post_test && !loop_test(test, test_owner, registers)? {
            break;
        }
    }
    Ok(crate::completion::Completion::Normal)
}

fn promise_loop_point(
    body: crate::machine::CodeView<'_>,
    next: usize,
    suspension_slot: Option<u16>,
    label: &Option<String>,
    test: crate::machine::CodeView<'_>,
    update: crate::machine::CodeView<'_>,
    dst: u16,
    post_test: bool,
) -> crate::continuation::SuspensionPoint {
    crate::continuation::SuspensionPoint::Loop {
        pc: 0,
        label: label.clone(),
        body: body.range(),
        test: test.range(),
        update: update.range(),
        body_resume: crate::machine::CodeRange {
            code: body.range().code,
            start: body.range().start.saturating_add(next as u32),
            end: body.range().end,
        },
        dst,
        yield_dst: suspension_slot.unwrap_or_else(|| suspended_destination(body, next)),
        post_test,
    }
}

fn suspended_destination(body: crate::machine::CodeView<'_>, next: usize) -> u16 {
    next.checked_sub(1)
        .and_then(|pc| body.cold_at(pc))
        .and_then(|op| match op {
            crate::ops::Op::Await { dst, .. } | crate::ops::Op::Yield { src: dst } => Some(*dst),
            _ => None,
        })
        .unwrap_or(0)
}

fn loop_suspension(
    completion: crate::completion::Completion,
    point: crate::continuation::SuspensionPoint,
) -> crate::completion::Completion {
    match completion {
        crate::completion::Completion::Suspend(promise) => {
            crate::completion::Completion::SuspendAt(promise, point)
        }
        crate::completion::Completion::SuspendAt(promise, inner) => {
            crate::completion::Completion::SuspendAt(
                promise,
                crate::continuation::SuspensionPoint::Nested {
                    inner: Box::new(inner),
                    outer: Box::new(point),
                },
            )
        }
        other => other,
    }
}

fn store_loop_value(
    registers: &mut crate::register_file::RegisterFile,
    dst: u16,
    value: Option<crate::value::Value>,
) -> Result<(), crate::execute::VmError> {
    if let Some(value) = value {
        crate::execute::write_value(registers, dst, value);
    }
    Ok(())
}

fn update_empty_from(
    registers: &crate::register_file::RegisterFile,
    dst: u16,
    completion: crate::completion::Completion,
) -> Result<crate::completion::Completion, crate::execute::VmError> {
    let value = crate::execute::read_register(registers, dst)?;
    Ok(completion.update_empty(value))
}

fn loop_test(
    test: crate::machine::CodeView<'_>,
    owner: &crate::machine::FunctionCode,
    registers: &mut crate::register_file::RegisterFile,
) -> Result<bool, crate::execute::VmError> {
    let result = match crate::vm::execute_code_completion_with_owner(test, owner, registers)? {
        crate::completion::Completion::Return(value) => Ok(crate::execute::is_truthy(&value)),
        crate::completion::Completion::Normal => Ok(false),
        completion => completion
            .into_vm_error()
            .map(|value| crate::execute::is_truthy(&value)),
    }?;
    Ok(result)
}

fn run_fragment(
    ops: crate::machine::CodeView<'_>,
    owner: &crate::machine::FunctionCode,
    registers: &mut crate::register_file::RegisterFile,
) -> Result<(), crate::execute::VmError> {
    crate::execution_trace::event(crate::execution_trace::Event::FragmentEntry);
    if ops.is_empty() {
        return Ok(());
    }
    match crate::vm::execute_code_completion_with_owner(ops, owner, registers)? {
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
