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
    let frames = &generator.machine.borrow().frames.frames;
    let frame = match frames.last()? {
        crate::machine::Frame::Await { .. } => frames.get(frames.len().checked_sub(2)?)?.clone(),
        frame => frame.clone(),
    };
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
    if matches!(
        generator.machine.borrow().frames.frames.last(),
        Some(crate::machine::Frame::Await { .. })
    ) {
        generator.machine.borrow_mut().pop_await_frame();
    }
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
        if matches!(
            completion,
            crate::completion::Completion::Suspend(_)
                | crate::completion::Completion::SuspendAt(_, _)
        ) {
            try_push_frame(
                &mut generator.machine.borrow_mut(),
                crate::machine::Frame::Await {
                    phase: 0,
                    resume: generator.function.code.range,
                    destination: frame.yield_dst,
                },
            )?;
        }
        return Ok(Some(completion));
    }
    generator.machine.borrow_mut().pop_frame();
    // A nested loop owns the next continuation.  Resume the enclosing loop
    // from its already-recorded post-inner-loop body range before returning
    // to the parent function; otherwise the inner loop's completion skips the
    // remaining outer iterations.
    if matches!(
        generator.machine.borrow().frames.frames.last(),
        Some(crate::machine::Frame::Loop { .. })
    ) {
        return resume_loop_frame(generator, state, completion);
    }
    if matches!(
        generator.machine.borrow().frames.frames.last(),
        Some(crate::machine::Frame::Try { .. })
    ) {
        if let Some(completion) = crate::generator::resume_try_frame(generator, state, completion.clone())? {
            return Ok(Some(completion));
        }
    }
    resume_generator_range(generator, state, frame.resume, completion).map(Some)
}

fn run_loop_after_yield(
    generator: &GeneratorData,
    frame: &LoopFrameResume,
) -> Result<crate::completion::Completion, VmError> {
    let body = frame.body;
    let mut resume = frame.body_resume;
    loop {
        let pc = resume.start.saturating_sub(body.start) as usize;
        let step = execute_loop_body_range(generator, body, pc)?;
        if step.completion.is_suspension() {
            if let Some(crate::continuation::SuspensionPoint::Yield { src, .. }) = step.suspension {
                update_loop_body_resume(generator, body, step.pc, src)?;
                return Ok(step.completion);
            }
            // Nested structured operations carry their own exact point and
            // leave this frame's post-operation resume range unchanged.
            // Never recover a continuation by scanning syntax for an await.
            return Ok(step.completion);
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
        resume = frame.body;
    }
}

fn execute_loop_body_range(
    generator: &GeneratorData,
    range: crate::machine::CodeRange,
    pc: usize,
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
            pc,
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
    let mut machine = generator.machine.borrow_mut();
    let index = machine.frames.frames.iter().rposition(|frame| {
        matches!(frame, crate::machine::Frame::Loop { body, .. } if *body == range)
    }).ok_or(VmError::MissingReturn)?;
    let crate::machine::Frame::Loop { body_resume, yield_dst: destination, .. } =
        &mut machine.frames.frames[index] else { return Err(VmError::MissingReturn) };
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
