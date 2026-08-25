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
        if let Some(crate::ops::Op::Try { .. }) = generator
            .machine
            .borrow()
            .store
            .as_ref()
            .and_then(|store| store.code(frame.body_resume))
            .and_then(|code| code.cold_at(next.saturating_sub(1)))
        {
            install_nested_try_frame(generator, &frame, frame.body_resume, next)?;
            return Ok(Some(completion));
        }
        advance_frame_after_yield(generator, frame.body_resume, next)?;
        return Ok(Some(completion));
    }
    let (completion, handler_yield) = if matches!(frame.phase, crate::machine::TryPhase::Catch) {
        (completion, None)
    } else {
        run_try_handler(generator, state, &frame, completion)?
    };
    if let Some((range, next)) = handler_yield {
        advance_catch_after_yield(generator, range, next)?;
        return Ok(Some(completion));
    }
    let completion = run_try_finalizer(generator, state, &frame, completion)?;
    if completion.is_suspension() {
        return Ok(Some(completion));
    }
    generator.machine.borrow_mut().pop_frame();
    if let Some(outer) = try_frame_resume(generator) {
        generator
            .machine
            .borrow_mut()
            .advance_frame_resume(frame.resume, outer.yield_dst)
            .then_some(())
            .ok_or(VmError::MissingReturn)?;
        return resume_try_frame(generator, state, completion);
    }
    resume_after_try(generator, state, frame.resume, completion).map(Some)
}

fn install_nested_try_frame(
    generator: &GeneratorData,
    parent: &TryFrameResume,
    parent_range: crate::machine::CodeRange,
    next: usize,
) -> Result<(), VmError> {
    let store = generator
        .machine
        .borrow()
        .store
        .clone()
        .ok_or(VmError::MissingReturn)?;
    let code = store.code(parent_range).ok_or(VmError::MissingReturn)?;
    let Some(crate::ops::Op::Try {
        body,
        handler,
        finalizer,
        catch_slot,
        ..
    }) = code.cold_at(next.saturating_sub(1))
    else {
        return Err(VmError::MissingReturn);
    };
    let body_code = body.code().ok_or(VmError::MissingReturn)?;
    let Some((yield_index, crate::ops::Op::Yield { src })) =
        body_code.cold_ops().find(|(_, op)| matches!(op, crate::ops::Op::Yield { .. }))
    else {
        return Err(VmError::MissingReturn);
    };
    let suffix = body_code
        .slice(yield_index.saturating_add(1), body_code.len())
        .ok_or(VmError::MissingReturn)?;
    let body_resume = crate::machine::CodeRange {
        code: body.range.code,
        start: body.range.end.saturating_sub(suffix.len() as u32),
        end: body.range.end,
    };
    let resume = crate::machine::CodeRange {
        code: parent_range.code,
        start: parent_range.start.saturating_add(next as u32),
        end: parent_range.end,
    };
    generator
        .machine
        .borrow_mut()
        .advance_frame_resume(resume, parent.yield_dst)
        .then_some(())
        .ok_or(VmError::MissingReturn)?;
    try_push_frame(
        &mut generator.machine.borrow_mut(),
        crate::machine::Frame::Try {
            phase: crate::machine::TryPhase::Body,
            body: body.range,
            handler: handler.as_ref().map(|body| body.range),
            finalizer: finalizer.as_ref().map(|body| body.range),
            body_resume,
            resume,
            yield_dst: *src,
            catch_slot: *catch_slot,
        },
    )
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
    let completion = if matches!(step.completion, crate::completion::Completion::Normal) {
        generator
            .pending_completion
            .borrow_mut()
            .take()
            .unwrap_or(step.completion)
    } else {
        generator.pending_completion.borrow_mut().take();
        step.completion
    };
    resume_after_try(generator, state, frame.resume, completion).map(Some)
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
    let code = store.code(range).ok_or(VmError::MissingReturn)?;
    execute_with_generator_registers(generator, |registers| {
        crate::vm::execute_code_completion_step_in_place(code, registers)
    })
}

fn run_try_handler(
    generator: &GeneratorData,
    _state: &mut GeneratorState,
    frame: &TryFrameResume,
    completion: crate::completion::Completion,
) -> Result<(
    crate::completion::Completion,
    Option<(crate::machine::CodeRange, usize)>,
), VmError> {
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
    let step = execute_frame_step(generator, handler)?;
    let resume = step
        .completion
        .is_suspension()
        .then_some((handler, step.next));
    Ok((step.completion, resume))
}

fn advance_catch_after_yield(
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
    let code = store.code(range).ok_or(VmError::MissingReturn)?;
    let Some(crate::ops::Op::Yield { src }) = next.checked_sub(1).and_then(|index| code.cold_at(index))
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
        .set_try_catch_resume(resume, *src)
        .then_some(())
        .ok_or(VmError::MissingReturn)
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
        generator
            .pending_completion
            .replace(Some(completion.clone()));
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
    let code = store.code(range).ok_or(VmError::MissingReturn)?;
    let Some(crate::ops::Op::Yield { src }) = next.checked_sub(1).and_then(|index| code.cold_at(index))
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
