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
    let _locals = crate::locals::EnvironmentGuard::install(machine_environment(generator)?);
    let machine = generator.machine.borrow_mut();
    machine.pop_await_frame();
    let store = machine.store.clone().ok_or(VmError::MissingReturn)?;
    let ops = store
        .code(generator.function.code.range)
        .ok_or(VmError::MissingReturn)?;
    let pc = machine.pc as usize;
    let resume = completion.clone();
    let mut step_result = None;
    let completion = machine.step(completion, |registers| {
        let step = crate::vm::execute_generator_code_step(
            ops,
            registers,
            machine_environment(generator)?,
            pc,
            resume,
        )?;
        let completion = step.completion.clone();
        step_result = Some(step);
        Ok(completion)
    })?;
    let mut step = step_result.expect("generator VM step must produce a result");
    machine.pc = step.pc as u32;
    step.completion = completion;
    Ok(step)
}

fn execute_with_generator_registers<T>(
    generator: &GeneratorData,
    execute: impl FnOnce(&mut crate::register_file::RegisterFile) -> Result<T, VmError>,
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
    let _private = crate::private_environment::Guard::install_environment(
        generator.function.private_environment.clone(),
    );
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
    let store = generator
        .machine
        .borrow()
        .store
        .clone()
        .ok_or(VmError::MissingReturn)?;
    let ops = store.code(range).ok_or(VmError::MissingReturn)?;
    execute_with_generator_registers(generator, |registers| {
        crate::vm::execute_generator_code_step(
            ops,
            registers,
            machine_environment(generator)?,
            0,
            completion,
        )
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
    let offset = range
        .start
        .saturating_sub(generator.function.code.range.start) as usize;
    set_machine_pc(generator, offset.saturating_add(step.pc));
}

fn update_await_frame(
    generator: &GeneratorData,
    state: &mut GeneratorState,
    completion: &crate::completion::Completion,
) -> Result<(), VmError> {
    if !matches!(
        completion,
        crate::completion::Completion::Suspend(_)
            | crate::completion::Completion::SuspendAt(_, _)
    ) {
        return Ok(());
    }
    if let Some(iterator) = crate::loops::take_pending_async_for_of() {
        let mut iterator = iterator;
        iterator.await_dst = await_destination(generator);
        state.async_for_of = Some(iterator);
    }
    let destination = completion
        .suspension_point()
        .and_then(suspension_destination)
        .unwrap_or_else(|| await_destination(generator));
    try_push_frame(
        &mut generator.machine.borrow_mut(),
        crate::machine::Frame::Await {
            phase: 0,
            // Await is a stack marker; resumption continues from the machine
            // PC, while frame validation still needs a canonical code range.
            resume: generator.function.code.range,
            destination,
        },
    )
}

fn suspension_destination(point: &crate::continuation::SuspensionPoint) -> Option<u16> {
    match point {
        crate::continuation::SuspensionPoint::Loop { yield_dst, .. }
        | crate::continuation::SuspensionPoint::Yield { src: yield_dst, .. } => Some(*yield_dst),
        crate::continuation::SuspensionPoint::YieldStar { dst, .. } => Some(*dst),
        crate::continuation::SuspensionPoint::Nested { inner, .. } => suspension_destination(inner),
    }
}

fn await_destination(generator: &GeneratorData) -> u16 {
    match generator
        .function
        .code
        .code()
        .and_then(|code| code.cold_at(machine_pc(generator).saturating_sub(1)))
    {
        Some(crate::ops::Op::Await { dst, .. }) => *dst,
        _ => 0,
    }
}

fn push_iterator_frame(generator: &GeneratorData, state: &GeneratorState) -> Result<bool, VmError> {
    if generator.machine.borrow().frame_count() != 0 {
        return Ok(false);
    }
    let Some(frames) = iterator_frame_chain(generator, state)? else {
        return Ok(false);
    };
    let mut machine = generator.machine.borrow_mut();
    for frame in frames {
        try_push_frame(&mut machine, frame)?;
    }
    Ok(true)
}

fn push_branch_frame(generator: &GeneratorData, state: &GeneratorState) -> Result<(), VmError> {
    if generator.machine.borrow().frame_count() != 0 {
        return Ok(());
    }
    let Some(Op::Conditional {
        dst,
        condition,
        consequent,
        alternate,
    }) = generator
        .function
        .code
        .code()
        .and_then(|code| code.cold_at(machine_pc(generator).wrapping_sub(1)))
    else {
        return Ok(());
    };
    let test = crate::execute::read_register(&registers(generator), *condition)?;
    let branch = if crate::execute::is_truthy(&test) {
        consequent
    } else {
        alternate
    };
    let Some(ops) = branch.code() else {
        return Ok(());
    };
    let Some((index, Op::Yield { src })) = ops.find_cold(|op| matches!(op, Op::Yield { .. }))
    else {
        return Ok(());
    };
    let resume = parent_resume_range(generator, state);
    let branch_resume = crate::machine::CodeRange {
        code: branch.range.code,
        start: branch.range.start.saturating_add(index as u32 + 1),
        end: branch.range.end,
    };
    try_push_frame(
        &mut generator.machine.borrow_mut(),
        crate::machine::Frame::Branch {
            phase: crate::machine::BranchPhase::Body,
            branch_resume,
            resume,
            dst: *dst,
            yield_dst: *src,
        },
    )
}

fn push_try_frame(generator: &GeneratorData, state: &GeneratorState) -> Result<(), VmError> {
    if matches!(
        generator.machine.borrow().frames.frames.last(),
        Some(crate::machine::Frame::Try { .. })
    ) {
        return Ok(());
    }
    let Some((
        Op::Try {
            body,
            handler,
            finalizer,
            catch_slot,
            ..
        },
        Op::Yield { src },
        suffix,
    )) = suspended_try(generator, state)
    else {
        return Ok(());
    };
    let body_resume = range_after(body.range, suffix.len());
    let resume = parent_resume_range(generator, state);
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

struct InitialTryFrame {
    phase: crate::machine::TryPhase,
    body: crate::machine::CodeRange,
    handler: Option<crate::machine::CodeRange>,
    finalizer: Option<crate::machine::CodeRange>,
    body_resume: crate::machine::CodeRange,
    yield_dst: u16,
    catch_slot: Option<u16>,
}

fn push_initial_try_frames(generator: &GeneratorData) -> Result<bool, VmError> {
    let Some(code) = generator.function.code.code() else {
        return Ok(false);
    };
    let Some(path) = initial_try_path(code, generator.function.code.range) else {
        return Ok(false);
    };
    let frames = path.frames;
    let yield_dst = path.yield_dst;
    let has_frames = !frames.is_empty();
    let outer_resume = parent_resume_range(
        generator,
        &GeneratorState {
            nested: 0,
            private_environment: None,
            suspension: None,
            async_for_of: None,
            pending_completion: None,
        },
    );
    let empty = crate::machine::CodeRange {
        code: outer_resume.code,
        start: outer_resume.end,
        end: outer_resume.end,
    };
    for (index, frame) in frames.into_iter().enumerate() {
        let resume = if index == 0 { outer_resume } else { empty };
        try_push_frame(
            &mut generator.machine.borrow_mut(),
            crate::machine::Frame::Try {
                phase: frame.phase,
                body: frame.body,
                handler: frame.handler,
                finalizer: frame.finalizer,
                body_resume: frame.body_resume,
                resume,
                yield_dst,
                catch_slot: frame.catch_slot,
            },
        )?;
    }
    Ok(has_frames)
}

struct InitialTryPath {
    frames: Vec<InitialTryFrame>,
    yield_dst: u16,
    continuation: crate::machine::CodeRange,
}

fn initial_try_path(
    code: crate::machine::CodeView<'_>,
    range: crate::machine::CodeRange,
) -> Option<InitialTryPath> {
    for (index, op) in code.cold_ops() {
        if let Op::Yield { src } | Op::Await { dst: src, .. } = op {
            return Some(InitialTryPath {
                frames: Vec::new(),
                yield_dst: *src,
                continuation: crate::machine::CodeRange {
                    code: range.code,
                    start: range.start.saturating_add(index as u32 + 1),
                    end: range.end,
                },
            });
        }
        let nested = match op {
            Op::Loop { body, .. }
            | Op::ForOf { body, .. }
            | Op::ForIn { body, .. } => Some(body),
            _ => None,
        };
        if let Some(nested) = nested {
            let Some(nested_code) = nested.code() else { continue };
            let Some(mut path) = initial_try_path(nested_code, nested.range) else {
                continue;
            };
            path.continuation = crate::machine::CodeRange {
                code: range.code,
                start: range.start.saturating_add(index as u32 + 1),
                end: range.end,
            };
            return Some(path);
        }
        let Op::Try {
            body,
            handler,
            finalizer,
            catch_slot,
            ..
        } = op
        else {
            continue;
        };
        for (branch, phase) in [
            (Some(body), crate::machine::TryPhase::Body),
            (handler.as_ref(), crate::machine::TryPhase::Catch),
            (finalizer.as_ref(), crate::machine::TryPhase::Finally),
        ] {
            let Some(branch) = branch else { continue };
            let Some(branch_code) = branch.code() else {
                continue;
            };
            let Some(mut path) = initial_try_path(branch_code, branch.range) else {
                continue;
            };
            path.frames.insert(
                0,
                InitialTryFrame {
                    phase,
                    body: body.range,
                    handler: handler.as_ref().map(|body| body.range),
                    finalizer: finalizer.as_ref().map(|body| body.range),
                    body_resume: path.continuation,
                    yield_dst: path.yield_dst,
                    catch_slot: *catch_slot,
                },
            );
            path.continuation = crate::machine::CodeRange {
                code: range.code,
                start: range.start.saturating_add(index as u32 + 1),
                end: range.end,
            };
            return Some(path);
        }
    }
    None
}

fn push_private_frame(generator: &GeneratorData, state: &GeneratorState) -> Result<(), VmError> {
    if generator.machine.borrow().frame_count() != 0 {
        return Ok(());
    }
    let Some((_, body_ops, index)) = suspended_private_scope(generator, state) else {
        return Ok(());
    };
    let Some(Op::Yield { src }) = body_ops.cold_at(index) else {
        return Ok(());
    };
    let Some(Op::PrivateScope { body, .. }) = generator
        .function
        .code
        .code()
        .and_then(|code| code.cold_at(machine_pc(generator).wrapping_sub(1)))
    else {
        return Ok(());
    };
    let Some(environment) = state.private_environment.clone() else {
        return Ok(());
    };
    let body_resume = crate::machine::CodeRange {
        code: body.range.code,
        start: body.range.start.saturating_add(index as u32 + 1),
        end: body.range.end,
    };
    let resume = parent_resume_range(generator, state);
    try_push_frame(
        &mut generator.machine.borrow_mut(),
        crate::machine::Frame::Private {
            phase: crate::machine::PrivatePhase::Body,
            environment,
            body_resume,
            resume,
            yield_dst: *src,
        },
    )
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
    let store = generator
        .machine
        .borrow()
        .store
        .clone()
        .ok_or(VmError::MissingReturn)?;
    let code = store.code(range).ok_or(VmError::MissingReturn)?;
    let Some(Op::Yield { src }) = next.checked_sub(1).and_then(|index| code.cold_at(index)) else {
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
        .advance_frame_resume(resume, *src)
        .then_some(())
        .ok_or(VmError::MissingReturn)
}

fn parent_resume_range(
    generator: &GeneratorData,
    _state: &GeneratorState,
) -> crate::machine::CodeRange {
    crate::machine::CodeRange {
        code: generator.function.code_id(),
        start: generator
            .function
            .code
            .range
            .start
            .saturating_add(machine_pc(generator) as u32),
        end: generator.function.code.range.end,
    }
}

fn try_push_frame(
    machine: &mut crate::machine::Machine,
    frame: crate::machine::Frame,
) -> Result<(), VmError> {
    machine
        .try_push_frame(frame)
        .map_err(|_| VmError::EvalError("continuation frame stack overflow".to_string()))
}

fn resume_machine_frame(
    generator: &GeneratorData,
    state: &mut GeneratorState,
    completion: crate::completion::Completion,
) -> Result<Value, VmError> {
    // A resumed structured frame may suspend again at a different operation.
    // Replace the old point before handing the promise back to the host; the
    // next settlement must install the continuation that actually executed,
    // not the point that caused the previous await.
    state.suspension = completion.suspension_point().cloned();
    generator
        .machine
        .borrow_mut()
        .record_completion(completion.clone());
    complete_step(generator, state, completion)
}
