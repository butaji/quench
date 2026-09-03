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
    let Some(op) = generator.function.code.code().and_then(|code| code.cold_at(index)) else {
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
    registers: &crate::register_file::RegisterFile,
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
    let _private = crate::private_environment::Guard::install_environment(
        generator.function.private_environment.clone(),
    );
    let _home = crate::super_scope::Guard::install(&generator.function, &generator.receiver);
    let _with = crate::with_scope::FunctionGuard::install(&generator.function.with_captures);
    let _locals = crate::locals::EnvironmentGuard::install(machine_environment(generator)?);
    let store = generator
        .machine
        .borrow()
        .store
        .clone()
        .ok_or(VmError::MissingReturn)?;
    let frame_body = store.code(frame.body).ok_or(VmError::MissingReturn)?;
    if frame.repeat && code_needs_nested_resume(frame_body, true) {
        return resume_for_of_repeat_frame(generator, state, resume, &frame).map(Some);
    }
    if !frame.repeat && code_needs_nested_resume(frame_body, false) {
        return resume_iterator_nested_frame(generator, state, resume, &frame).map(Some);
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
    let body = store.code(frame.body_resume).ok_or(VmError::MissingReturn)?;
    let step = execute_with_generator_registers(generator, |registers| {
        crate::vm::execute_code_completion_step_in_place(body, registers)
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

struct ForOfConditional<'a> {
    branch: crate::machine::CodeView<'a>,
    yield_index: usize,
}

fn suspended_for_of_conditional<'a>(
    generator: &'a GeneratorData,
    _state: &GeneratorState,
) -> Option<ForOfConditional<'a>> {
    let Op::ForOf { body, .. } = generator
        .function
        .code
        .code()?
        .cold_at(machine_pc(generator).checked_sub(1)?)?
    else {
        return None;
    };
    let body = body.code()?;
    let (
        _body_index,
        Op::Conditional {
            condition,
            consequent,
            alternate,
            ..
        },
    ) = body.find_cold(conditional_contains_yield)?
    else {
        return None;
    };
    let test = crate::execute::read_register(&registers(generator), *condition).ok()?;
    let branch = if crate::execute::is_truthy(&test) {
        consequent
    } else {
        alternate
    }
    .code()?;
    let yield_index = branch.position_cold(|op| matches!(op, Op::Yield { .. }))?;
    Some(ForOfConditional {
        branch,
        yield_index,
    })
}

fn resume_for_of_repeat_frame(
    generator: &GeneratorData,
    state: &mut GeneratorState,
    resume: crate::completion::Completion,
    frame: &IteratorFrameResume,
) -> Result<crate::completion::Completion, VmError> {
    if !matches!(resume, crate::completion::Completion::Normal) {
        set_iterator_phase(generator, crate::machine::IteratorPhase::Close);
        return finish_iterator_frame(generator, state, frame, resume);
    }
    let store = generator
        .machine
        .borrow()
        .store
        .clone()
        .ok_or(VmError::MissingReturn)?;
    let body = store.code(frame.body).ok_or(VmError::MissingReturn)?;
    let completion = execute_with_generator_registers(generator, |registers| {
        resume_nested_code(body, registers)
    })?;
    set_iterator_phase(generator, crate::machine::IteratorPhase::Close);
    finish_iterator_frame(generator, state, frame, completion)
}

fn resume_iterator_nested_frame(
    generator: &GeneratorData,
    state: &mut GeneratorState,
    resume: crate::completion::Completion,
    frame: &IteratorFrameResume,
) -> Result<crate::completion::Completion, VmError> {
    if !matches!(resume, crate::completion::Completion::Normal) {
        set_iterator_phase(generator, crate::machine::IteratorPhase::Close);
        return finish_iterator_frame(generator, state, frame, resume);
    }
    let store = generator
        .machine
        .borrow()
        .store
        .clone()
        .ok_or(VmError::MissingReturn)?;
    let body = store.code(frame.body).ok_or(VmError::MissingReturn)?;
    let completion = execute_with_generator_registers(generator, |registers| {
        resume_nested_code(body, registers)
    })?;
    set_iterator_phase(generator, crate::machine::IteratorPhase::Close);
    finish_iterator_frame(generator, state, frame, completion)
}

fn resume_nested_code(
    view: crate::machine::CodeView<'_>,
    registers: &mut crate::register_file::RegisterFile,
) -> Result<crate::completion::Completion, VmError> {
    let Some((index, op)) = view.cold_ops().find(|(_, op)| op_contains_yield(op)) else {
        return crate::vm::execute_code_completion_in_current_frame(view, registers);
    };
    match op {
        Op::Yield { .. } => {
            let suffix = view.slice(index + 1, view.len()).ok_or(VmError::MissingReturn)?;
            crate::vm::execute_code_completion_in_current_frame(suffix, registers)
        }
        Op::Conditional {
            dst,
            condition,
            consequent,
            alternate,
        } => {
            let branch = if crate::execute::is_truthy(&crate::execute::read_register(registers, *condition)?) {
                consequent
            } else {
                alternate
            };
            let branch = branch.code().ok_or(VmError::MissingReturn)?;
            let completion = resume_nested_code(branch, registers)?;
            let crate::completion::Completion::Return(value) = completion else {
                return Ok(completion);
            };
            crate::execute::write_value(registers, *dst, value);
            let suffix = view.slice(index + 1, view.len()).ok_or(VmError::MissingReturn)?;
            crate::vm::execute_code_completion_in_current_frame(suffix, registers)
        }
        Op::IteratorBinding {
            iterator,
            body,
            close_normal,
        } => {
            let iterator_value = crate::execute::read_register(registers, *iterator)?;
            let body = body.code().ok_or(VmError::MissingReturn)?;
            let completion = resume_nested_code(body, registers)?;
            let completion = if matches!(completion, crate::completion::Completion::Normal)
                && !close_normal
            {
                completion
            } else {
                crate::collections::iterator::close(iterator_value, completion)?
            };
            if !matches!(completion, crate::completion::Completion::Normal) {
                return Ok(completion);
            }
            let suffix = view.slice(index + 1, view.len()).ok_or(VmError::MissingReturn)?;
            crate::vm::execute_code_completion_in_current_frame(suffix, registers)
        }
        Op::Try {
            body,
            handler,
            finalizer,
            ..
        } => {
            let nested = if body.code().is_some_and(code_view_contains_yield) {
                body.code()
            } else if handler
                .as_ref()
                .and_then(|body| body.code())
                .is_some_and(code_view_contains_yield)
            {
                handler.as_ref().and_then(|body| body.code())
            } else {
                finalizer.as_ref().and_then(|body| body.code())
            }
            .ok_or(VmError::MissingReturn)?;
            let completion = resume_nested_code(nested, registers)?;
            if !matches!(completion, crate::completion::Completion::Normal) {
                return Ok(completion);
            }
            let suffix = view.slice(index + 1, view.len()).ok_or(VmError::MissingReturn)?;
            crate::vm::execute_code_completion_in_current_frame(suffix, registers)
        }
        _ => Err(VmError::MissingReturn),
    }
}

fn op_contains_yield(op: &Op) -> bool {
    match op {
        Op::Yield { .. } | Op::YieldStar { .. } | Op::Await { .. } => true,
        Op::Conditional {
            consequent,
            alternate,
            ..
        } => consequent.code().is_some_and(code_view_contains_yield)
            || alternate.code().is_some_and(code_view_contains_yield),
        Op::IteratorBinding { body, .. } | Op::ForOf { body, .. } | Op::ForIn { body, .. } => {
            body.code().is_some_and(code_view_contains_yield)
        }
        Op::Try {
            body,
            handler,
            finalizer,
            ..
        } => body.code().is_some_and(code_view_contains_yield)
            || handler
                .as_ref()
                .and_then(|body| body.code())
                .is_some_and(code_view_contains_yield)
            || finalizer
                .as_ref()
                .and_then(|body| body.code())
                .is_some_and(code_view_contains_yield),
        _ => false,
    }
}

fn code_view_contains_yield(view: crate::machine::CodeView<'_>) -> bool {
    view.cold_ops().any(|(_, op)| op_contains_yield(op))
}

fn code_needs_nested_resume(view: crate::machine::CodeView<'_>, repeat: bool) -> bool {
    view.cold_ops().any(|(_, op)| {
        (matches!(op, Op::Conditional { .. } | Op::IteratorBinding { .. })
            || (!repeat && matches!(op, Op::Try { .. })))
            && op_contains_yield(op)
    })
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
    let body = store.code(frame.body).ok_or(VmError::MissingReturn)?;
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
            reset_yield_star_iterators(body, registers);
            crate::locals::write(frame.slot, value.clone());
            crate::vm::execute_generator_code_step(
                body,
                registers,
                machine_environment(generator)?,
                0,
                crate::completion::Completion::Normal,
            )
        })?;
        if step.completion.is_suspension() {
            if !push_for_of_body_frame(generator, frame, &body)? {
                advance_frame_after_yield(generator, frame.body, step.pc)?;
            }
            set_iterator_phase(generator, crate::machine::IteratorPhase::Body);
            return Ok(step.completion);
        }
        if !matches!(step.completion, crate::completion::Completion::Normal) {
            return finish_iterator_frame(generator, state, frame, step.completion);
        }
    }
}

fn reset_yield_star_iterators(
    view: crate::machine::CodeView<'_>,
    registers: &mut crate::register_file::RegisterFile,
) {
    for (_, op) in view.cold_ops() {
        match op {
            Op::YieldStar { iterator, .. } => {
                crate::execute::write_value(registers, *iterator, Value::Undefined);
            }
            Op::Conditional {
                consequent,
                alternate,
                ..
            } => {
                if let Some(branch) = consequent.code() {
                    reset_yield_star_iterators(branch, registers);
                }
                if let Some(branch) = alternate.code() {
                    reset_yield_star_iterators(branch, registers);
                }
            }
            Op::Try {
                body,
                handler,
                finalizer,
                ..
            } => {
                if let Some(body) = body.code() {
                    reset_yield_star_iterators(body, registers);
                }
                if let Some(handler) = handler.as_ref().and_then(|body| body.code()) {
                    reset_yield_star_iterators(handler, registers);
                }
                if let Some(finalizer) = finalizer.as_ref().and_then(|body| body.code()) {
                    reset_yield_star_iterators(finalizer, registers);
                }
            }
            Op::IteratorBinding { body, .. }
            | Op::ForOf { body, .. }
            | Op::ForIn { body, .. } => {
                if let Some(body) = body.code() {
                    reset_yield_star_iterators(body, registers);
                }
            }
            _ => {}
        }
    }
}

fn push_for_of_body_frame(
    generator: &GeneratorData,
    frame: &IteratorFrameResume,
    body: &crate::machine::CodeView<'_>,
) -> Result<bool, VmError> {
    let Some((index, op)) = body.cold_ops().find(|(_, op)| {
        matches!(op, Op::YieldStar { .. }) || try_contains_yield(op)
    }) else {
        return Ok(false);
    };
    if let Op::YieldStar {
        dst,
        source,
        iterator,
    } = op
    {
        let iterator_value = crate::execute::read_register(&registers(generator), *iterator)?;
        let iterator_value = if matches!(iterator_value, Value::Undefined) {
            let source = crate::execute::read_register(&registers(generator), *source)?;
            let iterator_value = crate::collections::iterator::delegate_start(source)?;
            crate::execute::write_value(
                &mut registers_mut(generator),
                *iterator,
                iterator_value.clone(),
            );
            iterator_value
        } else {
            iterator_value
        };
        let mut machine = generator.machine.borrow_mut();
        try_push_frame(
            &mut machine,
            crate::machine::Frame::Delegate {
                phase: 0,
                iterator: iterator_value,
                destination: *dst,
            },
        )?;
        return Ok(true);
    }
    let mut frames = Vec::new();
    if !collect_try_frames(
        op,
        range_after_iterator_op(frame.body, index),
        &registers(generator),
        &mut frames,
    )? {
        return Ok(false);
    }
    let mut machine = generator.machine.borrow_mut();
    for nested in frames {
        try_push_frame(&mut machine, nested)?;
    }
    Ok(true)
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
) -> Option<(&'a Op, crate::machine::CodeView<'a>, usize)> {
    let op @ Op::IteratorBinding { body, .. } = generator
        .function
        .code
        .code()?
        .cold_at(machine_pc(generator).checked_sub(1)?)?
    else {
        return None;
    };
    let body = body.code()?;
    let index = state
        .nested
        .checked_sub(1)
        .filter(|index| *index < body.len())
        .or_else(|| body.position_cold(|op| matches!(op, Op::Yield { .. })))?;
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
        crate::vm::execute_code_completion_step_in_place(
            body.slice(index + 1, body.len()).ok_or(VmError::MissingReturn)?,
            registers,
        )
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
        crate::vm::execute_code_completion_in_current_frame(
            suspension.branch.slice(suspension.yield_index + 1, suspension.branch.len()).ok_or(VmError::MissingReturn)?,
            registers,
        )
    })?;
    let crate::completion::Completion::Return(value) = completion else {
        return Ok(Some(completion));
    };
    write_conditional_result(suspension.conditional, &mut registers_mut(generator), value)?;
    let completion = execute_with_generator_registers(generator, |registers| {
        crate::vm::execute_code_completion_in_current_frame(
            suspension.body.slice(suspension.body_index + 1, suspension.body.len()).ok_or(VmError::MissingReturn)?,
            registers,
        )
    })?;
    close_iterator_binding(suspension.binding, &registers(generator), completion).map(Some)
}

fn write_conditional_result(
    conditional: &Op,
    registers: &mut crate::register_file::RegisterFile,
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
    registers: &crate::register_file::RegisterFile,
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
    if let Some(suspension) = suspended_for_of_conditional(generator, state) {
        if let Some(Op::Yield { src }) = suspension.branch.cold_at(suspension.yield_index) {
            crate::execute::write_value(&mut registers_mut(generator), *src, input.clone());
            return true;
        }
    }
    let Some((_, body, index)) = suspended_iterator_binding(generator, state) else {
        return false;
    };
    let Some(Op::Yield { src }) = body.cold_at(index) else {
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
        .code
        .code()?
        .cold_at(machine_pc(generator).checked_sub(1)?)?
    else {
        return None;
    };
    let body = body.code()?;
    let (
        body_index,
        conditional @ Op::Conditional {
            condition,
            consequent,
            alternate,
            ..
        },
    ) = body.find_cold(|op| matches!(op, Op::Conditional { .. }))?
    else {
        return None;
    };
    let test = crate::execute::read_register(&registers(generator), *condition).ok()?;
    let branch = if crate::execute::is_truthy(&test) {
        consequent
    } else {
        alternate
    }
    .code()?;
    let yield_index = branch.position_cold(|op| matches!(op, Op::Yield { .. }))?;
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
    let Some(Op::Yield { src }) = suspension.branch.cold_at(suspension.yield_index) else {
        return false;
    };
    crate::execute::write_value(&mut registers_mut(generator), *src, input.clone());
    true
}
struct IteratorConditional<'a> {
    binding: &'a Op,
    conditional: &'a Op,
    body: crate::machine::CodeView<'a>,
    body_index: usize,
    branch: crate::machine::CodeView<'a>,
    yield_index: usize,
}
