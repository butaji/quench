struct LoopFrameResume {
    label: Option<String>,
    body: crate::machine::CodeRange,
    test: crate::machine::CodeRange,
    update: crate::machine::CodeRange,
    body_resume: crate::machine::CodeRange,
    resume: crate::machine::CodeRange,
    dst: u16,
    yield_dst: u16,
}

fn loop_frame_resume(generator: &GeneratorData) -> Option<LoopFrameResume> {
    let frame = generator.machine.borrow().frames.frames.last()?.clone();
    let crate::machine::Frame::Loop {
        label,
        body,
        test,
        update,
        body_resume,
        resume,
        dst,
        yield_dst,
        post_test: _,
    } = frame
    else {
        return None;
    };
    Some(LoopFrameResume {
        label,
        body,
        test,
        update,
        body_resume,
        resume,
        dst,
        yield_dst,
    })
}

fn resume_loop_frame(
    generator: &GeneratorData,
    state: &mut GeneratorState,
    input: crate::completion::Completion,
) -> Result<Option<crate::completion::Completion>, VmError> {
    let Some(frame) = loop_frame_resume(generator) else {
        return Ok(None);
    };
    if !matches!(input, crate::completion::Completion::Normal) {
        // A throw/return injected at a yield inside a structured try must
        // enter that try's handler before the loop is unwound. Materialize
        // the nested continuation frame on demand; ordinary next() resumes
        // continue through the compact loop path below.
        if matches!(input, crate::completion::Completion::Throw(_))
            && crate::generator::suspended_try(generator, state).is_some()
        {
            crate::generator::push_try_frame(generator, state)?;
            return crate::generator::resume_try_frame(generator, state, input);
        }
        generator.machine.borrow_mut().pop_frame();
        return Ok(Some(input));
    }
    let _private = crate::private_environment::Guard::install_environment(
        generator.function.private_environment.clone(),
    );
    let _home = crate::super_scope::Guard::install(&generator.function, &generator.receiver);
    let _with = crate::with_scope::FunctionGuard::install(&generator.function.with_captures);
    let _locals = crate::locals::EnvironmentGuard::install(machine_environment(generator)?);
    let completion = run_loop_after_yield(generator, &frame)?;
    if completion.is_suspension() {
        return Ok(Some(completion));
    }
    generator.machine.borrow_mut().pop_frame();
    resume_generator_range(generator, state, frame.resume, completion).map(Some)
}

fn run_loop_after_yield(
    generator: &GeneratorData,
    frame: &LoopFrameResume,
) -> Result<crate::completion::Completion, VmError> {
    let mut body = frame.body_resume;
    loop {
        let step = execute_loop_body_range(generator, body)?;
        if step.completion.is_suspension() {
            if let Some(crate::continuation::SuspensionPoint::Yield { src, .. }) = step.suspension {
                update_loop_body_resume(generator, body, step.pc, src)?;
                return Ok(step.completion);
            }
            if let Some((resume, src)) = nested_loop_yield_resume(generator, body) {
                update_loop_body_resume_at(generator, resume, src)?;
                return Ok(step.completion);
            }
            return Err(VmError::MissingReturn);
        }
        match step.completion {
            crate::completion::Completion::Normal
            | crate::completion::Completion::Return(_) => {}
            completion => match completion.into_loop_transition(&frame.label) {
                crate::completion::LoopTransition::Continue(_) => {}
                crate::completion::LoopTransition::Break(value) => {
                    store_loop_value(generator, frame.dst, value)?;
                    return Ok(crate::completion::Completion::Normal);
                }
                crate::completion::LoopTransition::Propagate(completion) => return Ok(completion),
            },
        }

        execute_loop_fragment(generator, frame.update)?;
        let test = execute_loop_test(generator, frame.test)?;
        if !test {
            return Ok(crate::completion::Completion::Normal);
        }
        body = frame.body;
    }
}

fn nested_loop_yield_resume(
    generator: &GeneratorData,
    range: crate::machine::CodeRange,
) -> Option<(crate::machine::CodeRange, u16)> {
    let store = generator.machine.borrow().store.clone()?;
    let code = store.code(range)?;
    find_loop_yield_resume(code, range)
}

fn find_loop_yield_resume(
    code: crate::machine::CodeView<'_>,
    range: crate::machine::CodeRange,
) -> Option<(crate::machine::CodeRange, u16)> {
    for (index, op) in code.cold_ops() {
        if let crate::ops::Op::Yield { src } = op {
            return Some((crate::machine::CodeRange {
                code: range.code,
                start: range.start.saturating_add(index as u32 + 1),
                end: range.end,
            }, *src));
        }
        let branch = match op {
            crate::ops::Op::Try { body, .. }
            | crate::ops::Op::Loop { body, .. }
            | crate::ops::Op::ForOf { body, .. }
            | crate::ops::Op::ForIn { body, .. } => Some(body),
            crate::ops::Op::Conditional { consequent, alternate, .. } => {
                if let Some(found) = find_loop_yield_in_function(consequent) {
                    return Some(found);
                }
                Some(alternate)
            }
            _ => None,
        };
        if let Some(branch) = branch.and_then(find_loop_yield_in_function) {
            return Some(branch);
        }
    }
    None
}

fn find_loop_yield_in_function(
    function: &crate::machine::FunctionCode,
) -> Option<(crate::machine::CodeRange, u16)> {
    let range = function.range;
    function.code().and_then(|code| find_loop_yield_resume(code, range))
}

fn execute_loop_body_range(
    generator: &GeneratorData,
    range: crate::machine::CodeRange,
) -> Result<crate::vm::GeneratorStep, VmError> {
    let store = generator
        .machine
        .borrow()
        .store
        .clone()
        .ok_or(VmError::MissingReturn)?;
    let code = store.code(range).ok_or(VmError::MissingReturn)?;
    let environment = machine_environment(generator)?;
    execute_with_generator_registers(generator, |registers| {
        crate::vm::execute_generator_code_step(
            code,
            registers,
            environment,
            0,
            crate::completion::Completion::Normal,
        )
    })
}

fn execute_loop_fragment(
    generator: &GeneratorData,
    range: crate::machine::CodeRange,
) -> Result<(), VmError> {
    let store = generator
        .machine
        .borrow()
        .store
        .clone()
        .ok_or(VmError::MissingReturn)?;
    let code = store.code(range).ok_or(VmError::MissingReturn)?;
    let completion = execute_with_generator_registers(generator, |registers| {
        crate::vm::execute_code_completion_in_current_frame(code, registers)
    })?;
    match completion {
        crate::completion::Completion::Normal | crate::completion::Completion::Return(_) => Ok(()),
        completion => completion.into_vm_error().map(|_| ()),
    }
}

fn execute_loop_test(
    generator: &GeneratorData,
    range: crate::machine::CodeRange,
) -> Result<bool, VmError> {
    let store = generator
        .machine
        .borrow()
        .store
        .clone()
        .ok_or(VmError::MissingReturn)?;
    let code = store.code(range).ok_or(VmError::MissingReturn)?;
    let completion = execute_with_generator_registers(generator, |registers| {
        crate::vm::execute_code_completion_in_current_frame(code, registers)
    })?;
    match completion {
        crate::completion::Completion::Return(value) => Ok(crate::execute::is_truthy(&value)),
        crate::completion::Completion::Normal => Ok(false),
        completion => completion
            .into_vm_error()
            .map(|value| crate::execute::is_truthy(&value)),
    }
}

fn store_loop_value(
    generator: &GeneratorData,
    dst: u16,
    value: Option<Value>,
) -> Result<(), VmError> {
    if let Some(value) = value {
        crate::execute::write_value(&mut registers_mut(generator), dst, value);
    }
    Ok(())
}

fn update_loop_body_resume(
    generator: &GeneratorData,
    range: crate::machine::CodeRange,
    next: usize,
    yield_dst: u16,
) -> Result<(), VmError> {
    let resume = crate::machine::CodeRange {
        code: range.code,
        start: range.start.saturating_add(next as u32),
        end: range.end,
    };
    let machine = generator.machine.borrow_mut();
    let Some(crate::machine::Frame::Loop {
        body_resume,
        yield_dst: destination,
        ..
    }) = machine.frames.frames.last_mut()
    else {
        return Err(VmError::MissingReturn);
    };
    *body_resume = resume;
    *destination = yield_dst;
    Ok(())
}

fn update_loop_body_resume_at(
    generator: &GeneratorData,
    resume: crate::machine::CodeRange,
    yield_dst: u16,
) -> Result<(), VmError> {
    let machine = generator.machine.borrow_mut();
    let Some(crate::machine::Frame::Loop {
        body_resume,
        yield_dst: destination,
        ..
    }) = machine.frames.frames.last_mut()
    else {
        return Err(VmError::MissingReturn);
    };
    *body_resume = resume;
    *destination = yield_dst;
    Ok(())
}

fn install_loop_frame_input(generator: &GeneratorData, input: &Value) -> bool {
    let Some(frame) = loop_frame_resume(generator) else {
        return false;
    };
    crate::execute::write_value(&mut registers_mut(generator), frame.yield_dst, input.clone());
    true
}
