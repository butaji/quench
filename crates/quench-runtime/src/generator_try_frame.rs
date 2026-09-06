struct TryFrameResume {
    phase: crate::machine::TryPhase,
    body: crate::machine::CodeRange,
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
        body,
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
        body,
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
        crate::completion::Completion::Normal
            if matches!(
                frame.phase,
                crate::machine::TryPhase::Body | crate::machine::TryPhase::Catch
            ) =>
        {
            let step = execute_frame_step(generator, frame.body_resume)?;
            (step.completion, Some(step.next))
        }
        completion => (completion, None),
    };
    if completion.is_suspension() {
        let next = next.ok_or(VmError::MissingReturn)?;
        if let Err(error) = advance_frame_after_yield(generator, frame.body_resume, next) {
            if push_nested_try_after_yield(generator, &frame)? {
                return Ok(Some(completion));
            }
            return Err(error);
        }
        return Ok(Some(completion));
    }
    let completion = complete_try_frame(generator, state, &frame, completion)?;
    if completion.is_suspension() {
        return Ok(Some(completion));
    }
    generator.machine.borrow_mut().pop_frame();
    if matches!(completion, crate::completion::Completion::Normal) {
        // When this try is nested in a loop body, the loop—not the outer
        // generator range—is the continuation owner.  Advance the loop body
        // past the Try operation before handing control back to its frame.
        advance_parent_loop_after_try(generator, frame.body)?;
    }
    resume_after_try(generator, state, frame.resume, completion).map(Some)
}

fn advance_parent_loop_after_try(
    generator: &GeneratorData,
    inner_body: crate::machine::CodeRange,
) -> Result<bool, VmError> {
    let Some(crate::machine::Frame::Loop { body, .. }) = generator
        .machine
        .borrow()
        .frames
        .frames
        .last()
        .cloned()
    else {
        return Ok(false);
    };
    let store = generator
        .machine
        .borrow()
        .store
        .clone()
        .ok_or(VmError::MissingReturn)?;
    let code = store.code(body).ok_or(VmError::MissingReturn)?;
    let Some(index) = code.cold_ops().find_map(|(index, op)| match op {
        crate::ops::Op::Try { body, .. } if body.range == inner_body => Some(index),
        _ => None,
    }) else {
        return Ok(false);
    };
    let resume = crate::machine::CodeRange {
        code: body.code,
        start: body.start.saturating_add(index as u32 + 1),
        end: body.end,
    };
    let mut machine = generator.machine.borrow_mut();
    let Some(crate::machine::Frame::Loop {
        phase_resume: current,
        ..
    }) = machine.frames.frames.last_mut()
    else {
        return Ok(false);
    };
    *current = resume;
    Ok(true)
}

fn push_nested_try_after_yield(
    generator: &GeneratorData,
    frame: &TryFrameResume,
) -> Result<bool, VmError> {
    let store = generator
        .machine
        .borrow()
        .store
        .clone()
        .ok_or(VmError::MissingReturn)?;
    let code = store
        .code(frame.body_resume)
        .ok_or(VmError::MissingReturn)?;
    let Some((index, candidate)) = code
        .cold_ops()
        .find_map(|(index, op)| matches!(op, Op::Try { .. }).then_some((index, op)))
    else {
        return Ok(false);
    };
    let Op::Try {
        body,
        handler,
        finalizer,
        catch_slot,
        ..
    } = candidate
    else {
        return Ok(false);
    };
    let Some((_, Op::Yield { src }, suffix)) = try_yield_parts(generator, candidate, body) else {
        return Ok(false);
    };
    let outer_resume = crate::machine::CodeRange {
        code: frame.body_resume.code,
        start: frame.body_resume.start.saturating_add(index as u32 + 1),
        end: frame.body_resume.end,
    };
    let inner_resume = crate::machine::CodeRange {
        code: outer_resume.code,
        start: outer_resume.end,
        end: outer_resume.end,
    };
    {
        let mut machine = generator.machine.borrow_mut();
        let Some(crate::machine::Frame::Try { body_resume, .. }) = machine.frames.frames.last_mut()
        else {
            return Ok(false);
        };
        *body_resume = outer_resume;
        machine.frames.frames.push(crate::machine::Frame::Try {
            phase: crate::machine::TryPhase::Body,
            body: body.range,
            handler: handler.as_ref().map(|body| body.range),
            finalizer: finalizer.as_ref().map(|body| body.range),
            body_resume: range_after(body.range, suffix.len()),
            resume: inner_resume,
            yield_dst: *src,
            catch_slot: *catch_slot,
        });
    }
    Ok(true)
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
        state
            .pending_completion
            .take()
            .unwrap_or(crate::completion::Completion::Normal)
    } else {
        step.completion
    };
    resume_after_try(generator, state, frame.resume, completion).map(Some)
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
    let code = store.code(range).ok_or(VmError::MissingReturn)?;
    execute_with_generator_registers(generator, |registers| {
        crate::vm::execute_code_completion_step_in_place(code, registers)
    })
}

fn complete_try_frame(
    generator: &GeneratorData,
    state: &mut GeneratorState,
    frame: &TryFrameResume,
    completion: crate::completion::Completion,
) -> Result<crate::completion::Completion, VmError> {
    let completion = if matches!(frame.phase, crate::machine::TryPhase::Body) {
        run_try_handler(generator, state, frame, completion)?
    } else {
        completion
    };
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
    let step = execute_frame_step(generator, handler)?;
    if step.completion.is_suspension() {
        advance_catch_after_yield(generator, handler, step.next)?;
    }
    Ok(step.completion)
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
    let step = execute_frame_step(generator, finalizer)?;
    if step.completion.is_suspension() {
        state.pending_completion = Some(completion.clone());
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
    let Some(crate::ops::Op::Yield { src }) =
        next.checked_sub(1).and_then(|index| code.cold_at(index))
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
    let Some(crate::ops::Op::Yield { src }) =
        next.checked_sub(1).and_then(|index| code.cold_at(index))
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

fn resume_after_try(
    generator: &GeneratorData,
    state: &mut GeneratorState,
    range: crate::machine::CodeRange,
    completion: crate::completion::Completion,
) -> Result<crate::completion::Completion, VmError> {
    if matches!(
        generator.machine.borrow().frames.frames.last(),
        Some(crate::machine::Frame::Loop { .. })
    ) && matches!(completion, crate::completion::Completion::Normal)
    {
        if let Some(completion) = resume_loop_frame(generator, state, completion.clone())? {
            return Ok(completion);
        }
    }
    if matches!(
        generator.machine.borrow().frames.frames.last(),
        Some(crate::machine::Frame::Try { .. })
    ) {
        if let Some(completion) = resume_try_frame(generator, state, completion.clone())? {
            return Ok(completion);
        }
    }
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
