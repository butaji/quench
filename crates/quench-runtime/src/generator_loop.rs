struct LoopFrameResume {
    body: crate::machine::CodeRange,
    body_resume: crate::machine::CodeRange,
    test: crate::machine::CodeRange,
    update: crate::machine::CodeRange,
    resume: crate::machine::CodeRange,
    post_test: bool,
}

fn push_loop_frame(generator: &GeneratorData, state: &GeneratorState) -> Result<bool, VmError> {
    if generator.machine.borrow().frame_count() != 0 {
        return Ok(false);
    }
    let index = machine_pc(generator).checked_sub(1).ok_or(VmError::MissingReturn)?;
    let Some(Op::Loop { test, body, update, post_test, .. }) = generator.function.ops().get(index) else {
        return Ok(false);
    };
    let Some(body_ops) = body.ops() else { return Ok(false); };
    let Some(yield_index) = body_ops.iter().position(|op| matches!(op, Op::Yield { .. })) else {
        return Ok(false);
    };
    let body_resume = range_after_yield(body.range, yield_index);
    let frame = crate::machine::Frame::Loop {
        phase: crate::machine::LoopPhase::Body,
        body: body.range,
        body_resume,
        test: test.range,
        update: update.range,
        resume: parent_resume_range(generator, state),
        post_test: *post_test,
    };
    try_push_frame(&mut generator.machine.borrow_mut(), frame)?;
    Ok(true)
}

fn range_after_yield(range: crate::machine::CodeRange, index: usize) -> crate::machine::CodeRange {
    crate::machine::CodeRange {
        code: range.code,
        start: range.start.saturating_add(index as u32 + 1),
        end: range.end,
    }
}

fn loop_frame_resume(generator: &GeneratorData) -> Option<LoopFrameResume> {
    let frame = generator.machine.borrow().frames.frames.last()?.clone();
    let crate::machine::Frame::Loop { body, body_resume, test, update, resume, post_test, .. } = frame else {
        return None;
    };
    Some(LoopFrameResume { body, body_resume, test, update, resume, post_test })
}

fn resume_loop_frame(
    generator: &GeneratorData,
    state: &mut GeneratorState,
    completion: crate::completion::Completion,
) -> Result<Option<crate::completion::Completion>, VmError> {
    let Some(frame) = loop_frame_resume(generator) else { return Ok(None); };
    if !matches!(completion, crate::completion::Completion::Normal) {
        generator.machine.borrow_mut().pop_frame();
        return Ok(Some(completion));
    }
    generator.machine.borrow_mut().set_loop_phase(crate::machine::LoopPhase::Continue);
    let store = generator.machine.borrow().store.clone().ok_or(VmError::MissingReturn)?;
    let body = store.get(frame.body_resume).ok_or(VmError::MissingReturn)?;
    let step = execute_loop_with_context(generator, |registers| {
        crate::execute::execute_completion_step_in_place(body, registers)
    })?;
    if step.completion.is_suspension() {
        let next = crate::machine::CodeRange {
            code: frame.body_resume.code,
            start: frame.body_resume.start.saturating_add(step.next as u32),
            end: frame.body_resume.end,
        };
        generator.machine.borrow_mut().advance_loop_resume(next);
        return Ok(Some(step.completion));
    }
    if !matches!(step.completion, crate::completion::Completion::Normal) {
        generator.machine.borrow_mut().pop_frame();
        return Ok(Some(step.completion));
    }
    finish_loop_frame(generator, state, frame)
}

fn finish_loop_frame(
    generator: &GeneratorData,
    state: &mut GeneratorState,
    frame: LoopFrameResume,
) -> Result<Option<crate::completion::Completion>, VmError> {
    let store = generator.machine.borrow().store.clone().ok_or(VmError::MissingReturn)?;
    execute_range(generator, store.get(frame.update).ok_or(VmError::MissingReturn)?)?;
    let test = execute_range_value(generator, store.get(frame.test).ok_or(VmError::MissingReturn)?)?;
    if !crate::execute::is_truthy(&test) {
        generator.machine.borrow_mut().pop_frame();
        return resume_generator_range(generator, state, frame.resume, crate::completion::Completion::Normal).map(Some);
    }
    let body = store.get(frame.body).ok_or(VmError::MissingReturn)?;
    let step = execute_loop_with_context(generator, |registers| {
        crate::execute::execute_completion_step_in_place(body, registers)
    })?;
    if step.completion.is_suspension() {
        let next = crate::machine::CodeRange {
            code: frame.body.code,
            start: frame.body.start.saturating_add(step.next as u32),
            end: frame.body.end,
        };
        generator.machine.borrow_mut().advance_loop_resume(next);
    }
    let _ = frame.post_test;
    Ok(Some(step.completion))
}

fn execute_range(generator: &GeneratorData, ops: &[Op]) -> Result<(), VmError> {
    execute_loop_with_context(generator, |registers| {
        let completion = crate::execute::execute_completion_in_place(ops, registers)?;
        matches!(completion, crate::completion::Completion::Normal)
            .then_some(())
            .ok_or(VmError::MissingReturn)
    })
}

fn execute_range_value(generator: &GeneratorData, ops: &[Op]) -> Result<Value, VmError> {
    execute_loop_with_context(generator, |registers| {
        crate::execute::execute_in_place(ops, registers)
    })
}

fn execute_loop_with_context<T>(
    generator: &GeneratorData,
    execute: impl FnOnce(&mut Vec<Value>) -> Result<T, VmError>,
) -> Result<T, VmError> {
    let _private = crate::private_environment::Guard::install_environment(
        generator.function.private_environment.clone(),
    );
    let _home = crate::super_scope::Guard::install(&generator.function, &generator.receiver);
    let _with = crate::with_scope::FunctionGuard::install(&generator.function.with_captures);
    let environment = machine_environment(generator)?;
    let _environment = crate::locals::EnvironmentGuard::install(environment);
    execute_with_generator_registers(generator, execute)
}
