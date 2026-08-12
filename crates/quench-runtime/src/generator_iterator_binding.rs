struct IteratorFrameResume {
    iterator: Value,
    body_resume: crate::machine::CodeRange,
    resume: crate::machine::CodeRange,
    yield_dst: u16,
    close_normal: bool,
}

fn iterator_frame_chain(
    generator: &GeneratorData,
    state: &GeneratorState,
) -> Result<Option<Vec<crate::machine::Frame>>, VmError> {
    let index = machine_pc(generator).checked_sub(1).ok_or(VmError::MissingReturn)?;
    let Some(binding @ Op::IteratorBinding { .. }) = generator.function.ops().get(index) else {
        return Ok(None);
    };
    let resume = parent_resume_range(generator, state);
    let mut frames = Vec::new();
    let registers = registers(generator);
    if collect_iterator_frames(binding, resume, &registers, &mut frames)? {
        return Ok(Some(frames));
    }
    Ok(None)
}

fn collect_iterator_frames(
    binding: &Op,
    resume: crate::machine::CodeRange,
    registers: &[Value],
    frames: &mut Vec<crate::machine::Frame>,
) -> Result<bool, VmError> {
    let Op::IteratorBinding { iterator, body, close_normal } = binding else {
        return Ok(false);
    };
    let Some(ops) = body.ops() else {
        return Err(VmError::MissingReturn);
    };
    for (index, op) in ops.iter().enumerate() {
        if let Op::Yield { src } = op {
            frames.push(iterator_frame(*iterator, body.range, resume, index, *src, *close_normal, registers)?);
            return Ok(true);
        }
        if matches!(op, Op::IteratorBinding { .. }) {
            let next = range_after_iterator_op(body.range, index);
            let frame = iterator_frame(*iterator, body.range, resume, index, 0, *close_normal, registers)?;
            frames.push(frame);
            if collect_iterator_frames(op, next, registers, frames)? {
                return Ok(true);
            }
            frames.pop();
        }
    }
    Ok(false)
}

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
    Ok(crate::machine::Frame::Iterator {
        phase: crate::machine::IteratorPhase::Body,
        iterator,
        binding,
        body,
        body_resume: range_after_iterator_op(body, index),
        resume,
        yield_dst,
        close_normal,
    })
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
    let crate::machine::Frame::Iterator { iterator, body_resume, resume, yield_dst, close_normal, .. } = frame else {
        return None;
    };
    Some(IteratorFrameResume { iterator, body_resume, resume, yield_dst, close_normal })
}

fn resume_iterator_frame(
    generator: &GeneratorData,
    state: &mut GeneratorState,
    resume: crate::completion::Completion,
) -> Result<Option<crate::completion::Completion>, VmError> {
    let Some(frame) = iterator_frame_resume(generator) else {
        return Ok(None);
    };
    if !matches!(resume, crate::completion::Completion::Normal) {
        set_iterator_phase(generator, crate::machine::IteratorPhase::Close);
        return finish_iterator_frame(generator, state, &frame, resume).map(Some);
    }
    set_iterator_phase(generator, crate::machine::IteratorPhase::Continue);
    let store = generator.machine.borrow().store.clone().ok_or(VmError::MissingReturn)?;
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

fn finish_iterator_frame(
    generator: &GeneratorData,
    state: &mut GeneratorState,
    frame: &IteratorFrameResume,
    completion: crate::completion::Completion,
) -> Result<crate::completion::Completion, VmError> {
    let completion = close_iterator_frame(frame, completion)?;
    generator.machine.borrow_mut().pop_frame();
    resume_after_iterator(generator, state, frame.resume, completion)
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
    let Some(frame) = iterator_frame_resume(generator) else {
        return false;
    };
    crate::execute::write_value(&mut registers_mut(generator), frame.yield_dst, input.clone());
    true
}

fn suspended_iterator_binding<'a>(
    generator: &'a GeneratorData,
    state: &GeneratorState,
) -> Option<(&'a Op, &'a [Op], usize)> {
    let op @ Op::IteratorBinding { body, .. } = generator.function.ops().get(machine_pc(generator).checked_sub(1)?)? else {
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
    let Some(suspension) = suspended_iterator_conditional(generator, state)
    else {
        return Ok(None);
    };
    if !matches!(resume, crate::completion::Completion::Normal) {
        return close_iterator_binding(suspension.binding, &registers(generator), resume).map(Some);
    }
    let completion = execute_with_generator_registers(generator, |registers| {
        crate::execute::execute_completion_in_place(
            &suspension.branch[suspension.yield_index + 1..], registers,
        )
    })?;
    let crate::completion::Completion::Return(value) = completion else {
        return Ok(Some(completion));
    };
    write_conditional_result(suspension.conditional, &mut registers_mut(generator), value)?;
    let completion = execute_with_generator_registers(generator, |registers| {
        crate::execute::execute_completion_in_place(
            &suspension.body[suspension.body_index + 1..], registers,
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
    let Op::IteratorBinding { iterator, close_normal, .. } = op else {
        return Err(VmError::MissingReturn);
    };
    if matches!(completion, crate::completion::Completion::Normal) && !close_normal {
        return Ok(completion);
    }
    crate::collections::iterator::close(crate::execute::read_register(registers, *iterator)?, completion)
}

fn install_iterator_binding_input(generator: &GeneratorData, state: &mut GeneratorState, input: &Value) -> bool {
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
    let binding @ Op::IteratorBinding { body, .. } = generator.function.ops().get(machine_pc(generator).checked_sub(1)?)? else {
        return None;
    };
    let body = body.ops()?;
    let (body_index, conditional @ Op::Conditional { condition, consequent, alternate, .. }) = body
        .iter()
        .enumerate()
        .find(|(_, op)| matches!(op, Op::Conditional { .. }))?
    else {
        return None;
    };
    let test = crate::execute::read_register(&registers(generator), *condition).ok()?;
    let branch = if crate::execute::is_truthy(&test) { consequent } else { alternate }.ops()?;
    let yield_index = branch.iter().position(|op| matches!(op, Op::Yield { .. }))?;
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
