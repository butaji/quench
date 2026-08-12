struct PrivateFrameResume {
    environment: crate::private_environment::PrivateEnvironment,
    body_resume: crate::machine::CodeRange,
    resume: crate::machine::CodeRange,
    yield_dst: u16,
}

fn private_frame_resume(generator: &GeneratorData) -> Option<PrivateFrameResume> {
    let frame = generator.machine.borrow().frames.frames.last()?.clone();
    let crate::machine::Frame::Private { environment, body_resume, resume, yield_dst, .. } = frame else {
        return None;
    };
    Some(PrivateFrameResume { environment, body_resume, resume, yield_dst })
}

fn resume_private_frame(
    generator: &GeneratorData,
    state: &mut GeneratorState,
    input: crate::completion::Completion,
) -> Result<Option<crate::completion::Completion>, VmError> {
    let Some(frame) = private_frame_resume(generator) else { return Ok(None); };
    if !matches!(input, crate::completion::Completion::Normal) {
        generator.machine.borrow_mut().pop_frame();
        state.private_environment = None;
        return Ok(Some(input));
    }
    let _scope = crate::private_environment::Guard::install_environment(frame.environment);
    let completion = execute_private_suffix(generator, state, frame.body_resume)?;
    if completion.is_suspension() {
        advance_private_frame(generator, frame.body_resume)?;
        return Ok(Some(completion));
    }
    generator.machine.borrow_mut().pop_frame();
    state.private_environment = None;
    resume_after_private(generator, state, frame.resume, completion).map(Some)
}

fn advance_private_frame(
    generator: &GeneratorData,
    range: crate::machine::CodeRange,
) -> Result<(), VmError> {
    advance_frame_after_yield(generator, range)
}

fn execute_private_suffix(
    generator: &GeneratorData, _state: &mut GeneratorState, range: crate::machine::CodeRange,
) -> Result<crate::completion::Completion, VmError> {
    let store = generator.machine.borrow().store.clone().ok_or(VmError::MissingReturn)?;
    let ops = store.get(range).ok_or(VmError::MissingReturn)?;
    crate::execute::execute_completion_in_place(ops, &mut registers_mut(generator))
}

fn resume_after_private(
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

fn install_private_frame_input(generator: &GeneratorData, registers: &mut Vec<Value>, input: &Value) -> bool {
    let Some(frame) = private_frame_resume(generator) else { return false; };
    crate::execute::write_value(registers, frame.yield_dst, input.clone());
    true
}
