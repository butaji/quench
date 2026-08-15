struct TryFrameResume {
    phase: crate::machine::TryPhase,
    body_resume: crate::machine::CodeRange,
    handler: Option<crate::machine::CodeRange>,
    finalizer: Option<crate::machine::CodeRange>,
    resume: crate::machine::CodeRange,
    yield_dst: u16,
    catch_slot: Option<u16>,
    pending: Option<crate::completion::Completion>,
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
        pending,
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
        pending,
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
    let (completion, next) = match input {
        crate::completion::Completion::Normal => {
            let step = execute_frame_step(generator, frame.body_resume)?;
            (step.completion, Some(step.next))
        }
        completion => (completion, None),
    };
    if completion.is_suspension() {
        let next = next.ok_or(VmError::MissingReturn)?;
        if !push_nested_try_frame(generator, frame.body_resume, next)? {
            advance_frame_after_yield(generator, frame.body_resume, next)?;
        }
        return Ok(Some(completion));
    }
    let completion = if let Some(pending) = frame.pending {
        if matches!(completion, crate::completion::Completion::Normal) {
            pending
        } else {
            completion
        }
    } else if matches!(frame.phase, crate::machine::TryPhase::Catch) {
        run_try_finalizer(generator, state, &frame, completion)?
    } else {
        complete_try_frame(generator, state, &frame, completion)?
    };
    if completion.is_suspension() {
        return Ok(Some(completion));
    }
    generator.machine.borrow_mut().pop_frame();
    resume_after_try(generator, state, frame.resume, completion).map(Some)
}

fn push_nested_try_frame(
    generator: &GeneratorData,
    range: crate::machine::CodeRange,
    next: usize,
) -> Result<bool, VmError> {
    let store = generator
        .machine
        .borrow()
        .store
        .clone()
        .ok_or(VmError::MissingReturn)?;
    let ops = store.get(range).ok_or(VmError::MissingReturn)?;
    let Some(crate::ops::Op::Try {
        body,
        handler,
        finalizer,
        catch_slot,
    }) = next.checked_sub(1).and_then(|index| ops.get(index))
    else {
        return Ok(false);
    };
    let Some(body_ops) = body.ops() else {
        return Ok(false);
    };
    let Some((yield_index, crate::ops::Op::Yield { src })) = body_ops
        .iter()
        .enumerate()
        .find(|(_, op)| matches!(op, crate::ops::Op::Yield { .. }))
    else {
        return Ok(false);
    };
    let body_resume = crate::machine::CodeRange {
        code: body.range.code,
        start: body.range.start.saturating_add(yield_index as u32 + 1),
        end: body.range.end,
    };
    let resume = crate::machine::CodeRange {
        code: range.code,
        start: range.start.saturating_add(next as u32),
        end: range.end,
    };
    let outer_resume = store
        .get(resume)
        .and_then(|suffix| {
            suffix
                .iter()
                .position(|op| matches!(op, crate::ops::Op::Yield { .. }))
                .map(|index| crate::machine::CodeRange {
                    code: resume.code,
                    start: resume.start.saturating_add(index as u32 + 1),
                    end: resume.end,
                })
        })
        .unwrap_or(resume);
    let mut machine = generator.machine.borrow_mut();
    if let Some(crate::machine::Frame::Try { body_resume, .. }) = machine.frames.frames.last_mut() {
        *body_resume = outer_resume;
    }
    machine
        .try_push_frame(crate::machine::Frame::Try {
            phase: crate::machine::TryPhase::Body,
            body: body.range,
            handler: handler.as_ref().map(|code| code.range),
            finalizer: finalizer.as_ref().map(|code| code.range),
            body_resume,
            resume,
            yield_dst: *src,
            catch_slot: *catch_slot,
            pending: None,
        })
        .map_err(|_| VmError::EvalError("continuation frame stack overflow".to_string()))?;
    Ok(true)
}

fn execute_frame_range(
    generator: &GeneratorData,
    _state: &mut GeneratorState,
    range: crate::machine::CodeRange,
) -> Result<crate::vm::CompletionStep, VmError> {
    execute_frame_step(generator, range)
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
    let (completion, handler_next) = run_try_handler(generator, state, frame, completion)?;
    if let (Some(handler), Some(next)) = (frame.handler, handler_next) {
        advance_frame_after_yield(generator, handler, next)?;
        set_try_phase(generator, crate::machine::TryPhase::Catch);
        return Ok(completion);
    }
    run_try_finalizer(generator, state, frame, completion)
}

fn run_try_handler(
    generator: &GeneratorData,
    state: &mut GeneratorState,
    frame: &TryFrameResume,
    completion: crate::completion::Completion,
) -> Result<(crate::completion::Completion, Option<usize>), VmError> {
    let crate::completion::Completion::Throw(value) = completion else {
        return Ok((completion, None));
    };
    let Some(handler) = frame.handler else {
        return Ok((crate::completion::Completion::Throw(value), None));
    };
    if let Some(slot) = frame.catch_slot {
        crate::execute::write_value(&mut registers_mut(generator), slot, value.clone());
        crate::locals::write(slot, value);
    }
    let step = execute_frame_range(generator, state, handler)?;
    let next = step.completion.is_suspension().then_some(step.next);
    Ok((step.completion, next))
}

fn run_try_finalizer(
    generator: &GeneratorData,
    state: &mut GeneratorState,
    frame: &TryFrameResume,
    completion: crate::completion::Completion,
) -> Result<crate::completion::Completion, VmError> {
    let Some(finalizer) = frame.finalizer else {
        return Ok(completion);
    };
    let step = execute_frame_range(generator, state, finalizer)?;
    if step.completion.is_suspension() {
        advance_frame_after_yield(generator, finalizer, step.next)?;
        set_try_pending(generator, completion.clone());
    }
    match step.completion {
        crate::completion::Completion::Normal => Ok(completion),
        abrupt => Ok(abrupt),
    }
}

fn set_try_pending(generator: &GeneratorData, completion: crate::completion::Completion) {
    let mut machine = generator.machine.borrow_mut();
    if let Some(crate::machine::Frame::Try { pending, .. }) = machine.frames.frames.last_mut() {
        *pending = Some(completion);
    }
}

fn set_try_phase(generator: &GeneratorData, phase: crate::machine::TryPhase) {
    let mut machine = generator.machine.borrow_mut();
    if let Some(crate::machine::Frame::Try { phase: current, .. }) =
        machine.frames.frames.last_mut()
    {
        *current = phase;
    }
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
