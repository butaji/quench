struct TryFrameResume {
    body_resume: crate::machine::CodeRange,
    handler: Option<crate::machine::CodeRange>,
    finalizer: Option<crate::machine::CodeRange>,
    resume: crate::machine::CodeRange,
    yield_dst: u16,
    catch_slot: Option<u16>,
}

fn try_frame_resume(generator: &GeneratorData) -> Option<TryFrameResume> {
    let frame = generator.machine.borrow().frames.frames.last()?.clone();
    let crate::machine::Frame::Try { body_resume, handler, finalizer, resume, yield_dst, catch_slot, .. } = frame else {
        return None;
    };
    Some(TryFrameResume { body_resume, handler, finalizer, resume, yield_dst, catch_slot })
}

fn resume_try_frame(
    generator: &GeneratorData,
    state: &mut GeneratorState,
    input: crate::completion::Completion,
) -> Result<Option<crate::completion::Completion>, VmError> {
    let Some(frame) = try_frame_resume(generator) else {
        return Ok(None);
    };
    let completion = match input {
        crate::completion::Completion::Normal => execute_frame_range(generator, state, frame.body_resume)?,
        completion => completion,
    };
    if completion.is_suspension() {
        advance_frame_after_yield(generator, frame.body_resume)?;
        return Ok(Some(completion));
    }
    let completion = complete_try_frame(generator, state, &frame, completion)?;
    if completion.is_suspension() {
        return Ok(Some(completion));
    }
    generator.machine.borrow_mut().pop_frame();
    resume_after_try(generator, state, frame.resume, completion).map(Some)
}

fn execute_frame_range(
    generator: &GeneratorData, _state: &mut GeneratorState, range: crate::machine::CodeRange,
) -> Result<crate::completion::Completion, VmError> {
    let store = generator.machine.borrow().store.clone().ok_or(VmError::MissingReturn)?;
    let ops = store.get(range).ok_or(VmError::MissingReturn)?;
    crate::execute::execute_completion_in_place(ops, &mut registers_mut(generator))
}

fn complete_try_frame(
    generator: &GeneratorData, state: &mut GeneratorState, frame: &TryFrameResume,
    completion: crate::completion::Completion,
) -> Result<crate::completion::Completion, VmError> {
    let completion = run_try_handler(generator, state, frame, completion)?;
    run_try_finalizer(generator, state, frame, completion)
}

fn run_try_handler(
    generator: &GeneratorData, state: &mut GeneratorState, frame: &TryFrameResume,
    completion: crate::completion::Completion,
) -> Result<crate::completion::Completion, VmError> {
    let crate::completion::Completion::Throw(value) = completion else { return Ok(completion); };
    let Some(handler) = frame.handler else { return Ok(crate::completion::Completion::Throw(value)); };
    if let Some(slot) = frame.catch_slot {
        crate::execute::write_value(&mut registers_mut(generator), slot, value.clone());
        crate::locals::write(slot, value);
    }
    execute_frame_range(generator, state, handler)
}

fn run_try_finalizer(
    generator: &GeneratorData, state: &mut GeneratorState, frame: &TryFrameResume,
    completion: crate::completion::Completion,
) -> Result<crate::completion::Completion, VmError> {
    let Some(finalizer) = frame.finalizer else { return Ok(completion); };
    match execute_frame_range(generator, state, finalizer)? {
        crate::completion::Completion::Normal => Ok(completion),
        abrupt => Ok(abrupt),
    }
}

fn resume_after_try(
    generator: &GeneratorData, state: &mut GeneratorState, range: crate::machine::CodeRange,
    completion: crate::completion::Completion,
) -> Result<crate::completion::Completion, VmError> {
    if !matches!(completion, crate::completion::Completion::Normal) { return Ok(completion); }
    let start = range.start.saturating_sub(generator.function.code.range.start) as usize;
    let step = crate::vm::execute_generator_step(generator.function.ops(), &mut registers_mut(generator), state.environment.clone(), start, completion)?;
    set_machine_pc(generator, step.pc);
    state.suspension = step.suspension;
    Ok(step.completion)
}

fn install_try_frame_input(generator: &GeneratorData, registers: &mut Vec<Value>, input: &Value) -> bool {
    let Some(frame) = try_frame_resume(generator) else { return false; };
    crate::execute::write_value(registers, frame.yield_dst, input.clone());
    true
}
