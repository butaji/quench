fn execute_generator_step(
    generator: &GeneratorData,
    _state: &mut GeneratorState,
    completion: crate::completion::Completion,
) -> Result<crate::vm::GeneratorStep, VmError> {
    let _private_environment = crate::private_environment::Guard::install_environment(
        generator.function.private_environment.clone(),
    );
    let _home = crate::super_scope::Guard::install(&generator.function, &generator.receiver);
    let _with_scope = crate::with_scope::FunctionGuard::install(&generator.function.with_captures);
    let (store, pc, mut registers) = take_machine_execution(generator)?;
    let ops = store.get(generator.function.code.range).ok_or(VmError::MissingReturn)?;
    let result = crate::vm::execute_generator_step(
        ops, &mut registers, machine_environment(generator)?, pc, completion,
    );
    restore_machine_execution(generator, registers, result)
}

fn take_machine_execution(
    generator: &GeneratorData,
) -> Result<(std::rc::Rc<crate::machine::CodeStore>, usize, Vec<Value>), VmError> {
    let mut machine = generator.machine.borrow_mut();
    machine.pop_await_frame();
    let store = machine.store.clone().ok_or(VmError::MissingReturn)?;
    let registers = machine.take_registers();
    Ok((store, machine.pc as usize, registers))
}

fn restore_machine_execution(
    generator: &GeneratorData,
    registers: Vec<Value>,
    result: Result<crate::vm::GeneratorStep, VmError>,
) -> Result<crate::vm::GeneratorStep, VmError> {
    let mut machine = generator.machine.borrow_mut();
    machine.restore_registers(registers);
    if let Ok(step) = &result {
        machine.record_completion(step.completion.clone());
    }
    result
}

fn execute_with_generator_registers<T>(
    generator: &GeneratorData,
    execute: impl FnOnce(&mut Vec<Value>) -> Result<T, VmError>,
) -> Result<T, VmError> {
    let mut registers = generator.machine.borrow_mut().take_registers();
    let result = execute(&mut registers);
    generator.machine.borrow_mut().restore_registers(registers);
    result
}

fn resume_generator_range(
    generator: &GeneratorData,
    state: &mut GeneratorState,
    range: crate::machine::CodeRange,
    completion: crate::completion::Completion,
) -> Result<crate::completion::Completion, VmError> {
    if !matches!(completion, crate::completion::Completion::Normal) {
        return Ok(completion);
    }
    let _private = crate::private_environment::Guard::install_environment(generator.function.private_environment.clone());
    let _home = crate::super_scope::Guard::install(&generator.function, &generator.receiver);
    let _with = crate::with_scope::FunctionGuard::install(&generator.function.with_captures);
    let step = execute_generator_range(generator, range, completion)?;
    update_range_execution(generator, range, &step);
    state.suspension = step.suspension;
    Ok(step.completion)
}

fn execute_generator_range(
    generator: &GeneratorData,
    range: crate::machine::CodeRange,
    completion: crate::completion::Completion,
) -> Result<crate::vm::GeneratorStep, VmError> {
    let store = generator.machine.borrow().store.clone().ok_or(VmError::MissingReturn)?;
    let ops = store.get(range).ok_or(VmError::MissingReturn)?;
    execute_with_generator_registers(generator, |registers| {
        crate::vm::execute_generator_step(ops, registers, machine_environment(generator)?, 0, completion)
    })
}

fn update_range_execution(
    generator: &GeneratorData,
    range: crate::machine::CodeRange,
    step: &crate::vm::GeneratorStep,
) {
    if range.code != generator.function.code_id() {
        return;
    }
    let offset = range.start.saturating_sub(generator.function.code.range.start) as usize;
    set_machine_pc(generator, offset.saturating_add(step.pc));
}

fn update_await_frame(
    generator: &GeneratorData,
    _state: &GeneratorState,
    completion: &crate::completion::Completion,
) -> Result<(), VmError> {
    if !matches!(completion, crate::completion::Completion::Suspend(_)) {
        return Ok(());
    }
    let pc = machine_pc(generator) as u32;
    try_push_frame(
        &mut generator.machine.borrow_mut(),
        crate::machine::Frame::Await {
            phase: 0,
            resume: crate::machine::CodeRange {
                code: generator.function.code_id(),
                start: pc,
                end: pc + 1,
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
        generator.function.ops().get(machine_pc(generator).wrapping_sub(1))
    else {
        return Ok(());
    };
    let iterator = crate::execute::read_register(&registers(generator), *binding)?;
    let resume = crate::machine::CodeRange {
        code: generator.function.code_id(),
        start: generator.function.code.range.start.saturating_add(machine_pc(generator) as u32),
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
        generator.function.ops().get(machine_pc(generator).wrapping_sub(1))
    else {
        return Ok(());
    };
    let test = crate::execute::read_register(&registers(generator), *condition)?;
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
    let Some(Op::PrivateScope { body, .. }) = generator.function.ops().get(machine_pc(generator).wrapping_sub(1)) else { return Ok(()); };
    let Some(environment) = state.private_environment.clone() else { return Ok(()); };
    let body_resume = crate::machine::CodeRange { code: body.range.code, start: body.range.start.saturating_add(index as u32 + 1), end: body.range.end };
    let resume = parent_resume_range(generator, state);
    try_push_frame(&mut generator.machine.borrow_mut(), crate::machine::Frame::Private {
        phase: crate::machine::PrivatePhase::Body,
        environment,
        body_resume,
        resume,
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
    next: usize,
) -> Result<(), VmError> {
    let store = generator.machine.borrow().store.clone().ok_or(VmError::MissingReturn)?;
    let ops = store.get(range).ok_or(VmError::MissingReturn)?;
    let Some(Op::Yield { src }) = next.checked_sub(1).and_then(|index| ops.get(index)) else {
        return Err(VmError::MissingReturn);
    };
    let resume = crate::machine::CodeRange {
        code: range.code,
        start: range.start.saturating_add(next as u32),
        end: range.end,
    };
    generator.machine.borrow_mut().advance_frame_resume(resume, *src)
        .then_some(())
        .ok_or(VmError::MissingReturn)
}

fn parent_resume_range(generator: &GeneratorData, _state: &GeneratorState) -> crate::machine::CodeRange {
    crate::machine::CodeRange {
        code: generator.function.code_id(),
        start: generator.function.code.range.start.saturating_add(machine_pc(generator) as u32),
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
    generator
        .machine
        .borrow_mut()
        .record_completion(completion.clone());
    complete_step(generator, state, completion)
}
