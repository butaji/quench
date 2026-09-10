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
        try_push_frame(
            &mut generator.machine.borrow_mut(),
            crate::machine::Frame::Await {
                phase: 0,
                resume: generator.function.code.range,
                destination: loop_frame_resume(generator)
                    .map(|current| current.yield_dst)
                    .unwrap_or(frame.yield_dst),
            },
        )?;
        return Ok(Some(completion));
    }
    generator.machine.borrow_mut().pop_frame();
    resume_generator_range(generator, state, frame.resume, completion).map(Some)
}

fn run_loop_after_yield(
    generator: &GeneratorData,
    frame: &LoopFrameResume,
) -> Result<crate::completion::Completion, VmError> {
    // Branch jumps are encoded relative to the complete loop body. Retain
    // that coordinate space across suspension and resume from an explicit PC;
    // slicing at body_resume would reinterpret those jump targets from zero.
    let mut body_pc = frame
        .body_resume
        .start
        .checked_sub(frame.body.start)
        .ok_or(VmError::MissingReturn)? as usize;
    loop {
        let step = execute_loop_body_range(generator, frame.body, body_pc)?;
        if step.completion.is_suspension() {
            if let Some(src) = loop_suspension_destination(generator, frame.body, &step) {
                update_loop_body_resume(generator, frame.body, step.pc, src)?;
                return Ok(step.completion);
            }
            return Err(VmError::MissingReturn);
        }
        match step.completion {
            crate::completion::Completion::Normal => {}
            crate::completion::Completion::Return(value) => {
                return Ok(crate::completion::Completion::Return(value));
            }
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
        body_pc = 0;
    }
}

fn loop_suspension_destination(
    generator: &GeneratorData,
    body: crate::machine::CodeRange,
    step: &crate::vm::GeneratorStep,
) -> Option<u16> {
    if let Some(crate::continuation::SuspensionPoint::Yield { src, .. }) = step.suspension {
        return Some(src);
    }
    let store = generator.machine.borrow().store.clone()?;
    let code = store.code(body)?;
    let candidate = code.cold_at(step.pc.saturating_sub(1));
    if let Some(crate::ops::Op::Branch { condition, then_ops, else_ops }) = candidate {
        let truthy = crate::execute::read_register(&registers(generator), *condition)
            .ok()
            .is_some_and(|value| crate::execute::is_truthy(&value));
        let selected = if truthy { then_ops } else { else_ops };
        if let Some((_, crate::ops::Op::Await { dst, .. })) = selected
            .code()
            .and_then(|view| view.find_cold(|op| matches!(op, crate::ops::Op::Await { .. })))
        {
            return Some(*dst);
        }
    }
    match candidate? {
        crate::ops::Op::Await { dst, .. } => Some(*dst),
        _ => None,
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
