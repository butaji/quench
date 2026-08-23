struct IteratorFrameResume {
    iterator: Value,
    body_resume: crate::machine::CodeRange,
    resume: crate::machine::CodeRange,
    close_normal: bool,
    repeat: bool,
    slot: u16,
    body: crate::machine::CodeRange,
}

fn iterator_frame_chain(
    generator: &GeneratorData,
    state: &GeneratorState,
) -> Result<Option<Vec<crate::machine::Frame>>, VmError> {
    let index = machine_pc(generator)
        .checked_sub(1)
        .ok_or(VmError::MissingReturn)?;
    let Some(op) = generator.function.ops().get(index) else {
        return Ok(None);
    };
    let resume = parent_resume_range(generator, state);
    let mut frames = Vec::new();
    let registers = registers(generator);
    if collect_for_of_frames(op, resume, &registers, &mut frames)?
        || collect_iterator_frames(op, resume, &registers, &mut frames)?
    {
        return Ok(Some(frames));
    }
    Ok(None)
}

include!("generator_for_of_frames.rs");

fn iterator_frame(
    binding: u16,
    body: crate::machine::CodeRange,
    resume: crate::machine::CodeRange,
    index: usize,
    yield_dst: u16,
    close_normal: bool,
    registers: &[Value],
) -> Result<crate::machine::Frame, VmError> {
    let iterator = crate::execute::read_register(registers, binding)?;
    Ok(iterator_binding_frame(
        iterator,
        binding,
        body,
        range_after_iterator_op(body, index),
        resume,
        (yield_dst, close_normal, false, 0),
    ))
}

fn iterator_binding_frame(
    iterator: Value,
    binding: u16,
    body: crate::machine::CodeRange,
    body_resume: crate::machine::CodeRange,
    resume: crate::machine::CodeRange,
    config: (u16, bool, bool, u16),
) -> crate::machine::Frame {
    crate::machine::Frame::Iterator {
        phase: crate::machine::IteratorPhase::Body,
        iterator,
        binding,
        body,
        body_resume,
        resume,
        yield_dst: config.0,
        close_normal: config.1,
        repeat: config.2,
        slot: config.3,
    }
}

fn range_after_iterator_op(
    range: crate::machine::CodeRange,
    index: usize,
) -> crate::machine::CodeRange {
    crate::machine::CodeRange {
        code: range.code,
        start: range.start.saturating_add(index as u32 + 1),
        end: range.end,
    }
}

fn iterator_frame_resume(generator: &GeneratorData) -> Option<IteratorFrameResume> {
    let frame = generator.machine.borrow().frames.frames.last()?.clone();
    let crate::machine::Frame::Iterator {
        iterator,
        body_resume,
        resume,
        close_normal,
        repeat,
        slot,
        body,
        ..
    } = frame
    else {
        return None;
    };
    Some(IteratorFrameResume {
        iterator,
        body_resume,
        resume,
        close_normal,
        repeat,
        slot,
        body,
    })
}

fn resume_iterator_frame(
    generator: &GeneratorData,
    state: &mut GeneratorState,
    resume: crate::completion::Completion,
) -> Result<Option<crate::completion::Completion>, VmError> {
    let Some(frame) = iterator_frame_resume(generator) else {
        return Ok(None);
    };
    if suspended_iterator_conditional(generator, state).is_some() {
        return resume_iterator_conditional_frame(generator, state, resume, &frame).map(Some);
    }
    if !matches!(resume, crate::completion::Completion::Normal) {
        set_iterator_phase(generator, crate::machine::IteratorPhase::Close);
        return finish_iterator_frame(generator, state, &frame, resume).map(Some);
    }
    set_iterator_phase(generator, crate::machine::IteratorPhase::Continue);
    let store = generator
        .machine
        .borrow()
        .store
        .clone()
        .ok_or(VmError::MissingReturn)?;
    let body = store.get(frame.body_resume).ok_or(VmError::MissingReturn)?;
    let step = execute_with_generator_registers(generator, |registers| {
        crate::execute::execute_completion_step_in_place(body, registers)
    })?;
    let completion = step.completion;
    if completion.is_suspension() {
        advance_frame_after_yield(generator, frame.body_resume, step.next)?;
        set_iterator_phase(generator, crate::machine::IteratorPhase::Body);
        return Ok(Some(completion));
    }
    set_iterator_phase(generator, crate::machine::IteratorPhase::Close);
    finish_iterator_frame(generator, state, &frame, completion).map(Some)
}

fn resume_iterator_conditional_frame(
    generator: &GeneratorData,
    state: &mut GeneratorState,
    resume: crate::completion::Completion,
    frame: &IteratorFrameResume,
) -> Result<crate::completion::Completion, VmError> {
    let Some(suspension) = suspended_iterator_conditional(generator, state) else {
        return Ok(resume);
    };
    if !matches!(resume, crate::completion::Completion::Normal) {
        set_iterator_phase(generator, crate::machine::IteratorPhase::Close);
        return finish_iterator_frame(generator, state, frame, resume);
    }
    let completion = execute_with_generator_registers(generator, |registers| {
        crate::execute::execute_completion_in_place(
            &suspension.branch[suspension.yield_index + 1..],
            registers,
        )
    })?;
    let crate::completion::Completion::Return(value) = completion else {
        set_iterator_phase(generator, crate::machine::IteratorPhase::Close);
        return finish_iterator_frame(generator, state, frame, completion);
    };
    write_conditional_result(suspension.conditional, &mut registers_mut(generator), value)?;
    let completion = execute_with_generator_registers(generator, |registers| {
        crate::execute::execute_completion_in_place(
            &suspension.body[suspension.body_index + 1..],
            registers,
        )
    })?;
    set_iterator_phase(generator, crate::machine::IteratorPhase::Close);
    let completion = close_iterator_frame(frame, completion)?;
    generator.machine.borrow_mut().pop_frame();
    resume_after_iterator(generator, state, frame.resume, completion)
}

fn finish_iterator_frame(
    generator: &GeneratorData,
    state: &mut GeneratorState,
    frame: &IteratorFrameResume,
    completion: crate::completion::Completion,
) -> Result<crate::completion::Completion, VmError> {
    if frame.repeat && matches!(completion, crate::completion::Completion::Normal) {
        return continue_for_of(generator, state, frame);
    }
    let completion = close_iterator_frame(frame, completion)?;
    crate::loops::take_live_for_of();
    generator.machine.borrow_mut().pop_frame();
    resume_after_iterator(generator, state, frame.resume, completion)
}

fn continue_for_of(
    generator: &GeneratorData,
    state: &mut GeneratorState,
    frame: &IteratorFrameResume,
) -> Result<crate::completion::Completion, VmError> {
    let store = generator
        .machine
        .borrow()
        .store
        .clone()
        .ok_or(VmError::MissingReturn)?;
    let body = store.get(frame.body).ok_or(VmError::MissingReturn)?;
    loop {
        let next = crate::collections::iterator::step_value(&frame.iterator)?;
        let Some(value) = next else {
            crate::loops::take_live_for_of();
            generator.machine.borrow_mut().pop_frame();
            return resume_after_iterator(
                generator,
                state,
                frame.resume,
                crate::completion::Completion::Normal,
            );
        };
        let step = execute_with_generator_registers(generator, |registers| {
            crate::locals::write(frame.slot, value.clone());
            crate::execute::execute_completion_step_in_place(body, registers)
        })?;
        if step.completion.is_suspension() {
            let _ = advance_frame_after_yield(generator, frame.body, step.next);
            set_iterator_phase(generator, crate::machine::IteratorPhase::Body);
            return Ok(step.completion);
        }
        if !matches!(step.completion, crate::completion::Completion::Normal) {
            return finish_iterator_frame(generator, state, frame, step.completion);
        }
    }
}

fn resume_after_iterator(
    generator: &GeneratorData,
    state: &mut GeneratorState,
    range: crate::machine::CodeRange,
    completion: crate::completion::Completion,
) -> Result<crate::completion::Completion, VmError> {
    resume_generator_range(generator, state, range, completion)
}

fn set_iterator_phase(generator: &GeneratorData, phase: crate::machine::IteratorPhase) {
    generator.machine.borrow_mut().set_iterator_phase(phase);
}

fn close_iterator_frame(
    frame: &IteratorFrameResume,
    completion: crate::completion::Completion,
) -> Result<crate::completion::Completion, VmError> {
    if matches!(completion, crate::completion::Completion::Normal) && !frame.close_normal {
        return Ok(completion);
    }
    crate::collections::iterator::close(frame.iterator.clone(), completion)
}

fn install_iterator_frame_input(generator: &GeneratorData, input: &Value) -> bool {
    let frames = generator.machine.borrow().frames.frames.clone();
    for frame in frames.iter().rev() {
        let crate::machine::Frame::Iterator { yield_dst, .. } = frame else {
            continue;
        };
        if *yield_dst == 0 {
            continue;
        }
        crate::execute::write_value(&mut registers_mut(generator), *yield_dst, input.clone());
        return true;
    }
    false
}

fn suspended_iterator_binding<'a>(
    generator: &'a GeneratorData,
    state: &GeneratorState,
) -> Option<(&'a Op, &'a [Op], usize)> {
    let op @ Op::IteratorBinding { body, .. } = generator
        .function
        .ops()
        .get(machine_pc(generator).checked_sub(1)?)?
    else {
        return None;
    };
    let body = body.ops()?;
    let index = state
        .nested
        .checked_sub(1)
        .filter(|index| *index < body.len())
        .or_else(|| body.iter().position(|op| matches!(op, Op::Yield { .. })))?;
    Some((op, body, index))
}

fn resume_suspended_iterator_binding(
    generator: &GeneratorData,
    state: &mut GeneratorState,
    resume: crate::completion::Completion,
) -> Result<Option<crate::completion::Completion>, VmError> {
    if let Some(completion) = resume_iterator_frame(generator, state, resume.clone())? {
        return Ok(Some(completion));
    }
    if let Some(completion) = resume_iterator_conditional(generator, state, resume.clone())? {
        return Ok(Some(completion));
    }
    let Some((op, body, index)) = suspended_iterator_binding(generator, state) else {
        return Ok(None);
    };
    if !matches!(resume, crate::completion::Completion::Normal) {
        state.nested = 0;
        return close_iterator_binding(op, &registers(generator), resume).map(Some);
    }
    let step = execute_with_generator_registers(generator, |registers| {
        crate::execute::execute_completion_step_in_place(&body[index + 1..], registers)
    })?;
    let completion = step.completion;
    if matches!(completion, crate::completion::Completion::Yield(_)) {
        state.nested = index + step.next + 1;
        return Ok(Some(completion));
    }
    state.nested = 0;
    close_iterator_binding(op, &registers(generator), completion).map(Some)
}

fn resume_iterator_conditional(
    generator: &GeneratorData,
    state: &mut GeneratorState,
    resume: crate::completion::Completion,
) -> Result<Option<crate::completion::Completion>, VmError> {
    let Some(suspension) = suspended_iterator_conditional(generator, state) else {
        return Ok(None);
    };
    if !matches!(resume, crate::completion::Completion::Normal) {
        return close_iterator_binding(suspension.binding, &registers(generator), resume).map(Some);
    }
    let completion = execute_with_generator_registers(generator, |registers| {
        crate::execute::execute_completion_in_place(
            &suspension.branch[suspension.yield_index + 1..],
            registers,
        )
    })?;
    let crate::completion::Completion::Return(value) = completion else {
        return Ok(Some(completion));
    };
    write_conditional_result(suspension.conditional, &mut registers_mut(generator), value)?;
    let completion = execute_with_generator_registers(generator, |registers| {
        crate::execute::execute_completion_in_place(
            &suspension.body[suspension.body_index + 1..],
            registers,
        )
    })?;
    close_iterator_binding(suspension.binding, &registers(generator), completion).map(Some)
}

fn write_conditional_result(
    conditional: &Op,
    registers: &mut Vec<Value>,
    value: Value,
) -> Result<(), VmError> {
    let Op::Conditional { dst, .. } = conditional else {
        return Err(VmError::MissingReturn);
    };
    crate::execute::write_value(registers, *dst, value);
    Ok(())
}

fn close_iterator_binding(
    op: &Op,
    registers: &[Value],
    completion: crate::completion::Completion,
) -> Result<crate::completion::Completion, VmError> {
    let Op::IteratorBinding {
        iterator,
        close_normal,
        ..
    } = op
    else {
        return Err(VmError::MissingReturn);
    };
    if matches!(completion, crate::completion::Completion::Normal) && !close_normal {
        return Ok(completion);
    }
    crate::collections::iterator::close(
        crate::execute::read_register(registers, *iterator)?,
        completion,
    )
}

fn install_iterator_binding_input(
    generator: &GeneratorData,
    state: &mut GeneratorState,
    input: &Value,
) -> bool {
    if install_iterator_frame_input(generator, input) {
        return true;
    }
    let Some((_, body, index)) = suspended_iterator_binding(generator, state) else {
        return false;
    };
    let Some(Op::Yield { src }) = body.get(index) else {
        return false;
    };
    crate::execute::write_value(&mut registers_mut(generator), *src, input.clone());
    true
}

fn suspended_iterator_conditional<'a>(
    generator: &'a GeneratorData,
    _state: &GeneratorState,
) -> Option<IteratorConditional<'a>> {
    let binding @ Op::IteratorBinding { body, .. } = generator
        .function
        .ops()
        .get(machine_pc(generator).checked_sub(1)?)?
    else {
        return None;
    };
    let body = body.ops()?;
    let (
        body_index,
        conditional @ Op::Conditional {
            condition,
            consequent,
            alternate,
            ..
        },
    ) = body
        .iter()
        .enumerate()
        .find(|(_, op)| matches!(op, Op::Conditional { .. }))?
    else {
        return None;
    };
    let test = crate::execute::read_register(&registers(generator), *condition).ok()?;
    let branch = if crate::execute::is_truthy(&test) {
        consequent
    } else {
        alternate
    }
    .ops()?;
    let yield_index = branch
        .iter()
        .position(|op| matches!(op, Op::Yield { .. }))?;
    Some(IteratorConditional {
        binding,
        conditional,
        body,
        body_index,
        branch,
        yield_index,
    })
}

fn install_iterator_conditional_input(
    generator: &GeneratorData,
    state: &mut GeneratorState,
    input: &Value,
) -> bool {
    let Some(suspension) = suspended_iterator_conditional(generator, state) else {
        return false;
    };
    let Some(Op::Yield { src }) = suspension.branch.get(suspension.yield_index) else {
        return false;
    };
    crate::execute::write_value(&mut registers_mut(generator), *src, input.clone());
    true
}
struct IteratorConditional<'a> {
    binding: &'a Op,
    conditional: &'a Op,
    body: &'a [Op],
    body_index: usize,
    branch: &'a [Op],
    yield_index: usize,
}
