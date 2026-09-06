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
    let update_code = update
        .code()
        .ok_or(crate::execute::VmError::MissingReturn)?;
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
    let shape = LoopExecution {
        label,
        body,
        test,
        update,
        dst,
        post_test,
        per_iteration,
    };
    let _ = init_owner.enter_invocation();
    let init_step = execute_loop_fragment_step_with_owner(registers, init, init_owner, 0)?;
    if init_step.completion.is_suspension() {
        return phase_suspension(
            &shape,
            crate::continuation::LoopPhase::Init,
            init,
            init_step,
            registers,
        );
    }
    finish_loop_fragment(&init_step.completion)?;
    refresh_per_iteration(per_iteration);
    loop {
        crate::execution_trace::event(crate::execution_trace::Event::LoopIteration);
        crate::execution_trace::loop_shape_iteration(loop_shape);
        if !post_test {
            let _ = test_owner.enter_invocation();
            let step = execute_loop_fragment_step_with_owner(registers, test, test_owner, 0)?;
            if step.completion.is_suspension() {
                return phase_suspension(
                    &shape,
                    crate::continuation::LoopPhase::Test,
                    test,
                    step,
                    registers,
                );
            }
            if !loop_test_completion(step.completion)? {
                break;
            }
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
                let completion =
                    wrap_body_suspension(completion, body, body_next, suspension_slot, &shape)?;
                return update_empty_from(registers, dst, completion);
            }
        }
        refresh_per_iteration(per_iteration);
        let _ = update_owner.enter_invocation();
        let step = execute_loop_fragment_step_with_owner(registers, update, update_owner, 0)?;
        if step.completion.is_suspension() {
            return phase_suspension(
                &shape,
                crate::continuation::LoopPhase::Update,
                update,
                step,
                registers,
            );
        }
        finish_loop_fragment(&step.completion)?;
        if post_test {
            let _ = test_owner.enter_invocation();
            let step = execute_loop_fragment_step_with_owner(registers, test, test_owner, 0)?;
            if step.completion.is_suspension() {
                return phase_suspension(
                    &shape,
                    crate::continuation::LoopPhase::Test,
                    test,
                    step,
                    registers,
                );
            }
            if !loop_test_completion(step.completion)? {
                break;
            }
        }
    }
    Ok(crate::completion::Completion::Normal)
}

struct LoopExecution<'a> {
    label: &'a Option<String>,
    body: crate::machine::CodeView<'a>,
    test: crate::machine::CodeView<'a>,
    update: crate::machine::CodeView<'a>,
    dst: u16,
    post_test: bool,
    per_iteration: &'a [u16],
}

fn phase_suspension(
    shape: &LoopExecution<'_>,
    phase: crate::continuation::LoopPhase,
    code: crate::machine::CodeView<'_>,
    step: crate::vm::CompletionStep,
    registers: &crate::register_file::RegisterFile,
) -> Result<crate::completion::Completion, crate::execute::VmError> {
    let destination = suspension_slot(code, &step).ok_or(crate::execute::VmError::MissingReturn)?;
    let point = shape.suspension_point(phase, code, step.next, destination);
    let completion = loop_suspension(step.completion, point);
    update_empty_from(registers, shape.dst, completion)
}

impl LoopExecution<'_> {
    fn suspension_point(
        &self,
        phase: crate::continuation::LoopPhase,
        code: crate::machine::CodeView<'_>,
        next: usize,
        destination: u16,
    ) -> crate::continuation::SuspensionPoint {
        crate::continuation::SuspensionPoint::Loop {
            pc: 0,
            label: self.label.clone(),
            body: self.body.range(),
            test: self.test.range(),
            update: self.update.range(),
            phase,
            phase_resume: suffix(code.range(), next),
            dst: self.dst,
            yield_dst: destination,
            post_test: self.post_test,
            per_iteration: self.per_iteration.into(),
        }
    }
}

fn suffix(range: crate::machine::CodeRange, next: usize) -> crate::machine::CodeRange {
    crate::machine::CodeRange {
        code: range.code,
        start: range.start.saturating_add(next as u32),
        end: range.end,
    }
}

fn finish_loop_fragment(
    completion: &crate::completion::Completion,
) -> Result<(), crate::execute::VmError> {
    match completion {
        crate::completion::Completion::Normal | crate::completion::Completion::Return(_) => Ok(()),
        completion => completion.clone().into_vm_error().map(|_| ()),
    }
}

fn loop_test_completion(
    completion: crate::completion::Completion,
) -> Result<bool, crate::execute::VmError> {
    match completion {
        crate::completion::Completion::Return(value) => Ok(crate::execute::is_truthy(&value)),
        crate::completion::Completion::Normal => Ok(false),
        completion => completion
            .into_vm_error()
            .map(|value| crate::execute::is_truthy(&value)),
    }
}

fn wrap_body_suspension(
    completion: crate::completion::Completion,
    body: crate::machine::CodeView<'_>,
    next: usize,
    suspension_slot: Option<u16>,
    shape: &LoopExecution<'_>,
) -> Result<crate::completion::Completion, crate::execute::VmError> {
    if !completion.is_suspension() {
        return Ok(completion);
    }
    let destination = suspension_slot
        .or_else(|| completion.suspension_point().and_then(point_destination))
        .ok_or(crate::execute::VmError::MissingReturn)?;
    let point = crate::continuation::SuspensionPoint::Loop {
        pc: 0,
        label: shape.label.clone(),
        body: body.range(),
        test: shape.test.range(),
        update: shape.update.range(),
        phase: crate::continuation::LoopPhase::Body,
        phase_resume: suffix(body.range(), next),
        dst: shape.dst,
        yield_dst: destination,
        post_test: shape.post_test,
        per_iteration: shape.per_iteration.into(),
    };
    Ok(loop_suspension(completion, point))
}

fn point_destination(point: &crate::continuation::SuspensionPoint) -> Option<u16> {
    match point {
        crate::continuation::SuspensionPoint::Yield { src, .. }
        | crate::continuation::SuspensionPoint::Loop { yield_dst: src, .. }
        | crate::continuation::SuspensionPoint::Branch { yield_dst: src, .. } => Some(*src),
        crate::continuation::SuspensionPoint::YieldStar { dst, .. } => Some(*dst),
        crate::continuation::SuspensionPoint::Nested { inner, .. } => point_destination(inner),
    }
}

fn loop_suspension(
    completion: crate::completion::Completion,
    point: crate::continuation::SuspensionPoint,
) -> crate::completion::Completion {
    match completion {
        crate::completion::Completion::Suspend(promise) => {
            crate::completion::Completion::SuspendAt(promise, point)
        }
        crate::completion::Completion::Yield(value) => {
            crate::completion::Completion::YieldAt(value, point)
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
        crate::completion::Completion::YieldAt(value, inner) => {
            crate::completion::Completion::YieldAt(
                value,
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
