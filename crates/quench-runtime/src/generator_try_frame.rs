struct TryFrameResume {
    phase: crate::machine::TryPhase,
    body_resume: crate::machine::CodeRange,
    handler: Option<crate::machine::CodeRange>,
    finalizer: Option<crate::machine::CodeRange>,
    resume: crate::machine::CodeRange,
    yield_dst: u16,
    catch_slot: Option<u16>,
}

fn try_frame_resume(generator: &GeneratorData) -> Option<TryFrameResume> {
    let frame = generator.machine.borrow().frames.frames.last()?.clone();
    let crate::machine::Frame::Try {
        phase,
        body_resume,
        handler,
        finalizer,
        resume,
        yield_dst,
        catch_slot,
        ..
    } = frame
    else {
        return None;
    };
    Some(TryFrameResume {
        phase,
        body_resume,
        handler,
        finalizer,
        resume,
        yield_dst,
        catch_slot,
    })
}

fn resume_try_frame(
    generator: &GeneratorData,
    state: &mut GeneratorState,
    input: crate::completion::Completion,
) -> Result<Option<crate::completion::Completion>, VmError> {
    let Some(frame) = try_frame_resume(generator) else {
        return Ok(None);
    };
    if matches!(frame.phase, crate::machine::TryPhase::Finally) {
        return resume_finalizer_frame(generator, state, &frame, input);
    }
    let (completion, next) = match input {
        crate::completion::Completion::Normal => {
            let step = execute_frame_step(generator, frame.body_resume)?;
            (step.completion, Some(step.next))
        }
        completion => (completion, None),
    };
    if completion.is_suspension() {
        let next = next.ok_or(VmError::MissingReturn)?;
        advance_frame_after_yield(generator, frame.body_resume, next)?;
        return Ok(Some(completion));
    }
    let completion = complete_try_frame(generator, state, &frame, completion)?;
    if completion.is_suspension() {
        return Ok(Some(completion));
    }
    generator.machine.borrow_mut().pop_frame();
    resume_after_try(generator, state, frame.resume, completion).map(Some)
}

fn resume_finalizer_frame(
    generator: &GeneratorData,
    state: &mut GeneratorState,
    frame: &TryFrameResume,
    input: crate::completion::Completion,
) -> Result<Option<crate::completion::Completion>, VmError> {
    let step = execute_generator_range(generator, frame.body_resume, input)?;
    update_range_execution(generator, frame.body_resume, &step);
    state.suspension = step.suspension;
    if step.completion.is_suspension() {
        advance_finalizer_after_yield(generator, frame.body_resume, step.pc)?;
        return Ok(Some(step.completion));
    }
    generator.machine.borrow_mut().pop_frame();
    resume_after_try(generator, state, frame.resume, step.completion).map(Some)
}

fn execute_frame_range(
    generator: &GeneratorData,
    _state: &mut GeneratorState,
    range: crate::machine::CodeRange,
) -> Result<crate::completion::Completion, VmError> {
    Ok(execute_frame_step(generator, range)?.completion)
}

fn execute_frame_step(
    generator: &GeneratorData,
    range: crate::machine::CodeRange,
) -> Result<crate::vm::CompletionStep, VmError> {
    let store = generator
        .machine
        .borrow()
        .store
        .clone()
        .ok_or(VmError::MissingReturn)?;
    let ops = store.get(range).ok_or(VmError::MissingReturn)?;
    execute_with_generator_registers(generator, |registers| {
        crate::execute::execute_completion_step_in_place(ops, registers)
    })
}

fn complete_try_frame(
    generator: &GeneratorData,
    state: &mut GeneratorState,
    frame: &TryFrameResume,
    completion: crate::completion::Completion,
) -> Result<crate::completion::Completion, VmError> {
    let completion = run_try_handler(generator, state, frame, completion)?;
    run_try_finalizer(generator, state, frame, completion)
}

fn run_try_handler(
    generator: &GeneratorData,
    state: &mut GeneratorState,
    frame: &TryFrameResume,
    completion: crate::completion::Completion,
) -> Result<crate::completion::Completion, VmError> {
    let crate::completion::Completion::Throw(value) = completion else {
        return Ok(completion);
    };
    let Some(handler) = frame.handler else {
        return Ok(crate::completion::Completion::Throw(value));
    };
    if let Some(slot) = frame.catch_slot {
        crate::execute::write_value(&mut registers_mut(generator), slot, value.clone());
        crate::locals::write(slot, value);
    }
    execute_frame_range(generator, state, handler)
}

fn run_try_finalizer(
    generator: &GeneratorData,
    _state: &mut GeneratorState,
    frame: &TryFrameResume,
    completion: crate::completion::Completion,
) -> Result<crate::completion::Completion, VmError> {
    let Some(finalizer) = frame.finalizer else {
        return Ok(completion);
    };
    let step = execute_frame_step(generator, finalizer)?;
    if step.completion.is_suspension() {
        advance_finalizer_after_yield(generator, finalizer, step.next)?;
    }
    match step.completion {
        crate::completion::Completion::Normal => Ok(completion),
        abrupt => Ok(abrupt),
    }
}

fn advance_finalizer_after_yield(
    generator: &GeneratorData,
    range: crate::machine::CodeRange,
    next: usize,
) -> Result<(), VmError> {
    let store = generator
        .machine
        .borrow()
        .store
        .clone()
        .ok_or(VmError::MissingReturn)?;
    let ops = store.get(range).ok_or(VmError::MissingReturn)?;
    let Some(crate::ops::Op::Yield { src }) = next.checked_sub(1).and_then(|index| ops.get(index))
    else {
        return Err(VmError::MissingReturn);
    };
    let resume = crate::machine::CodeRange {
        code: range.code,
        start: range.start.saturating_add(next as u32),
        end: range.end,
    };
    generator
        .machine
        .borrow_mut()
        .set_try_finally_resume(resume, *src)
        .then_some(())
        .ok_or(VmError::MissingReturn)
}

fn resume_after_try(
    generator: &GeneratorData,
    state: &mut GeneratorState,
    range: crate::machine::CodeRange,
    completion: crate::completion::Completion,
) -> Result<crate::completion::Completion, VmError> {
    resume_generator_range(generator, state, range, completion)
}

fn install_try_frame_input(generator: &GeneratorData, input: &Value) -> bool {
    let Some(frame) = try_frame_resume(generator) else {
        return false;
    };
    crate::execute::write_value(
        &mut registers_mut(generator),
        frame.yield_dst,
        input.clone(),
    );
    true
}
