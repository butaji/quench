fn execute_generator_step(
    generator: &GeneratorData,
    state: &mut GeneratorState,
    completion: crate::completion::Completion,
) -> Result<crate::vm::GeneratorStep, VmError> {
    let _private_environment = crate::private_environment::Guard::install_environment(
        generator.function.private_environment.clone(),
    );
    let _home = crate::super_scope::Guard::install(&generator.function, &generator.receiver);
    let _with_scope = crate::with_scope::FunctionGuard::isolate();
    let mut machine = generator.machine.borrow_mut();
    machine.pop_await_frame();
    let store = machine.store.clone().ok_or(VmError::MissingReturn)?;
    let ops = store.get(generator.function.code.range).ok_or(VmError::MissingReturn)?;
    let input = completion;
    let mut generated = None;
    let completion = machine.step(input.clone(), |_| {
        let step = crate::vm::execute_generator_step(
            ops,
            &mut state.registers,
            state.environment.clone(),
            state.pc,
            input,
        )?;
        let completion = step.completion.clone();
        generated = Some(step);
        Ok(completion)
    })?;
    let Some(mut step) = generated else {
        return Err(VmError::MissingReturn);
    };
    step.completion = completion;
    Ok(step)
}

fn ensure_try_frame(generator: &GeneratorData, state: &GeneratorState) -> Result<(), VmError> {
    if suspended_try(generator, state).is_none()
        || generator.machine.borrow().frame_count() != 0
    {
        return Ok(());
    }
    push_try_frame(generator, state)
}

fn ensure_control_frame(generator: &GeneratorData, state: &GeneratorState) -> Result<(), VmError> {
    if (suspended_conditional(generator, state).is_none()
        && suspended_private_scope(generator, state).is_none())
        || generator.machine.borrow().frame_count() != 0
    {
        return Ok(());
    }
    try_push_frame(
        &mut generator.machine.borrow_mut(),
        crate::machine::Frame::Control {
            phase: 0,
            body: crate::machine::CodeRange {
                code: generator.function.code_id(),
                start: state.pc.saturating_sub(1) as u32,
                end: state.pc as u32,
            },
        },
    )
}

fn update_await_frame(
    generator: &GeneratorData,
    state: &GeneratorState,
    completion: &crate::completion::Completion,
) -> Result<(), VmError> {
    if !matches!(completion, crate::completion::Completion::Suspend(_)) {
        return Ok(());
    }
    try_push_frame(
        &mut generator.machine.borrow_mut(),
        crate::machine::Frame::Await {
            phase: 0,
            resume: crate::machine::CodeRange {
                code: generator.function.code_id(),
                start: state.pc as u32,
                end: state.pc as u32 + 1,
            },
        },
    )
}

fn push_iterator_frame(generator: &GeneratorData, state: &GeneratorState) -> Result<(), VmError> {
    if generator.machine.borrow().frame_count() != 0 {
        return Ok(());
    }
    let Some((_, body_ops, index)) = suspended_iterator_binding(generator, state)
    else {
        return Ok(());
    };
    let Some(Op::Yield { src }) = body_ops.get(index) else {
        return Ok(());
    };
    let Some(Op::IteratorBinding { iterator: binding, body, close_normal }) =
        generator.function.ops().get(state.pc.wrapping_sub(1))
    else {
        return Ok(());
    };
    let iterator = crate::execute::read_register(&state.registers, *binding)?;
    let resume = crate::machine::CodeRange {
        code: generator.function.code_id(),
        start: generator.function.code.range.start.saturating_add(state.pc as u32),
        end: generator.function.code.range.end,
    };
    try_push_frame(
        &mut generator.machine.borrow_mut(),
        crate::machine::Frame::Iterator {
            phase: crate::machine::IteratorPhase::Body,
            iterator,
            binding: *binding,
            body: body.range,
            body_resume: crate::machine::CodeRange {
                code: body.range.code,
                start: body.range.start.saturating_add(index as u32 + 1),
                end: body.range.end,
            },
            resume,
            yield_dst: *src,
            close_normal: *close_normal,
        },
    )
}

fn push_branch_frame(generator: &GeneratorData, state: &GeneratorState) -> Result<(), VmError> {
    if generator.machine.borrow().frame_count() != 0 {
        return Ok(());
    }
    let Some(Op::Conditional { dst, condition, consequent, alternate }) =
        generator.function.ops().get(state.pc.wrapping_sub(1))
    else {
        return Ok(());
    };
    let test = crate::execute::read_register(&state.registers, *condition)?;
    let branch = if crate::execute::is_truthy(&test) { consequent } else { alternate };
    let Some(ops) = branch.ops() else { return Ok(()); };
    let Some((index, Op::Yield { src })) = ops.iter().enumerate().find(|(_, op)| matches!(op, Op::Yield { .. })) else {
        return Ok(());
    };
    let resume = parent_resume_range(generator, state);
    let branch_resume = crate::machine::CodeRange {
        code: branch.range.code,
        start: branch.range.start.saturating_add(index as u32 + 1),
        end: branch.range.end,
    };
    try_push_frame(&mut generator.machine.borrow_mut(), crate::machine::Frame::Branch {
        phase: crate::machine::BranchPhase::Body,
        branch_resume,
        resume,
        dst: *dst,
        yield_dst: *src,
    })
}

fn push_try_frame(generator: &GeneratorData, state: &GeneratorState) -> Result<(), VmError> {
    if generator.machine.borrow().frame_count() != 0 {
        return Ok(());
    }
    let Some((Op::Try { body, handler, finalizer, catch_slot }, Op::Yield { src }, suffix)) =
        suspended_try(generator, state)
    else {
        return Ok(());
    };
    let body_resume = range_after(body.range, suffix.len());
    let resume = parent_resume_range(generator, state);
    try_push_frame(&mut generator.machine.borrow_mut(), crate::machine::Frame::Try {
        phase: crate::machine::TryPhase::Body,
        body: body.range,
        handler: handler.as_ref().map(|body| body.range),
        finalizer: finalizer.as_ref().map(|body| body.range),
        body_resume,
        resume,
        yield_dst: *src,
        catch_slot: *catch_slot,
    })
}

fn push_private_frame(generator: &GeneratorData, state: &GeneratorState) -> Result<(), VmError> {
    if generator.machine.borrow().frame_count() != 0 { return Ok(()); }
    let Some((_, body_ops, index)) = suspended_private_scope(generator, state) else { return Ok(()); };
    let Some(Op::Yield { src }) = body_ops.get(index) else { return Ok(()); };
    let Some(Op::PrivateScope { body, .. }) = generator.function.ops().get(state.pc.wrapping_sub(1)) else { return Ok(()); };
    let Some(environment) = state.private_environment.clone() else { return Ok(()); };
    let body_resume = crate::machine::CodeRange { code: body.range.code, start: body.range.start.saturating_add(index as u32 + 1), end: body.range.end };
    try_push_frame(&mut generator.machine.borrow_mut(), crate::machine::Frame::Private {
        phase: crate::machine::PrivatePhase::Body,
        environment,
        body_resume,
        resume: parent_resume_range(generator, state),
        yield_dst: *src,
    })
}

fn range_after(range: crate::machine::CodeRange, suffix_len: usize) -> crate::machine::CodeRange {
    crate::machine::CodeRange {
        code: range.code,
        start: range.end.saturating_sub(suffix_len as u32),
        end: range.end,
    }
}

fn advance_frame_after_yield(
    generator: &GeneratorData,
    range: crate::machine::CodeRange,
) -> Result<(), VmError> {
    let store = generator.machine.borrow().store.clone().ok_or(VmError::MissingReturn)?;
    let ops = store.get(range).ok_or(VmError::MissingReturn)?;
    let Some((index, Op::Yield { src })) = ops.iter().enumerate().find(|(_, op)| matches!(op, Op::Yield { .. })) else {
        return Err(VmError::MissingReturn);
    };
    let resume = crate::machine::CodeRange {
        code: range.code,
        start: range.start.saturating_add(index as u32 + 1),
        end: range.end,
    };
    generator.machine.borrow_mut().advance_frame_resume(resume, *src)
        .then_some(())
        .ok_or(VmError::MissingReturn)
}

fn parent_resume_range(generator: &GeneratorData, state: &GeneratorState) -> crate::machine::CodeRange {
    crate::machine::CodeRange {
        code: generator.function.code_id(),
        start: generator.function.code.range.start.saturating_add(state.pc as u32),
        end: generator.function.code.range.end,
    }
}

fn try_push_frame(machine: &mut crate::machine::Machine, frame: crate::machine::Frame) -> Result<(), VmError> {
    machine
        .try_push_frame(frame)
        .map_err(|_| VmError::EvalError("continuation frame stack overflow".to_string()))
}

fn resume_machine_frame(
    generator: &GeneratorData,
    state: &GeneratorState,
    completion: crate::completion::Completion,
) -> Result<Value, VmError> {
    let should_pop = !completion.is_suspension();
    generator
        .machine
        .borrow_mut()
        .record_completion(completion.clone());
    if should_pop {
        generator.machine.borrow_mut().pop_frame();
    }
    complete_step(generator, state, completion)
}
