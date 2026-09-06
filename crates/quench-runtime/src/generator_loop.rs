struct LoopFrameResume {
    label: Option<String>,
    body: crate::machine::CodeRange,
    test: crate::machine::CodeRange,
    update: crate::machine::CodeRange,
    phase: crate::continuation::LoopPhase,
    phase_resume: crate::machine::CodeRange,
    resume: crate::machine::CodeRange,
    dst: u16,
    yield_dst: u16,
    post_test: bool,
    per_iteration: std::rc::Rc<[u16]>,
}

fn loop_frame_resume(generator: &GeneratorData) -> Option<LoopFrameResume> {
    let frames = &generator.machine.borrow().frames.frames;
    let frame = match frames.last()? {
        crate::machine::Frame::Await { .. } => frames.get(frames.len().checked_sub(2)?)?.clone(),
        frame => frame.clone(),
    };
    let crate::machine::Frame::Loop {
        label,
        body,
        test,
        update,
        phase,
        phase_resume,
        resume,
        dst,
        yield_dst,
        post_test,
        per_iteration,
    } = frame
    else {
        return None;
    };
    Some(LoopFrameResume {
        label,
        body,
        test,
        update,
        phase,
        phase_resume,
        resume,
        dst,
        yield_dst,
        post_test,
        per_iteration,
    })
}

fn resume_loop_frame(
    generator: &GeneratorData,
    state: &mut GeneratorState,
    input: crate::completion::Completion,
) -> Result<Option<crate::completion::Completion>, VmError> {
    let Some(frame) = loop_frame_resume(generator) else {
        return Ok(None);
    };
    if matches!(
        generator.machine.borrow().frames.frames.last(),
        Some(crate::machine::Frame::Await { .. })
    ) {
        generator.machine.borrow_mut().pop_await_frame();
    }
    if !matches!(input, crate::completion::Completion::Normal) {
        if matches!(
            input,
            crate::completion::Completion::Throw(_) | crate::completion::Completion::Return(_)
        ) && generator
            .machine
            .borrow()
            .frames
            .frames
            .iter()
            .rev()
            .nth(1)
            .is_some_and(|frame| matches!(frame, crate::machine::Frame::Try { .. }))
        {
            generator.machine.borrow_mut().pop_frame();
            return crate::generator::resume_try_frame(generator, state, input);
        }
        // A throw/return injected at a yield inside a structured try must
        // enter that try's handler before the loop is unwound. Materialize
        // the nested continuation frame on demand; ordinary next() resumes
        // continue through the compact loop path below.
        if matches!(
            input,
            crate::completion::Completion::Throw(_) | crate::completion::Completion::Return(_)
        )
            && crate::generator::suspended_try(generator, state).is_some()
        {
            crate::generator::push_try_frame(generator, state)?;
            return crate::generator::resume_try_frame(generator, state, input);
        }
        generator.machine.borrow_mut().pop_frame();
        return Ok(Some(input));
    }
    let _private = crate::private_environment::Guard::install_environment(
        generator.function.private_environment.clone(),
    );
    let _home = crate::super_scope::Guard::install(&generator.function, &generator.receiver);
    let _with = crate::with_scope::FunctionGuard::install(&generator.function.with_captures);
    let _locals = crate::locals::EnvironmentGuard::install(machine_environment(generator)?);
    let completion = run_loop_after_yield(generator, &frame)?;
    if completion.is_suspension() {
        if matches!(
            completion,
            crate::completion::Completion::Suspend(_)
                | crate::completion::Completion::SuspendAt(_, _)
        ) {
            let destination = completion
                .suspension_point()
                .and_then(resume_destination)
                .unwrap_or(frame.yield_dst);
            try_push_frame(
                &mut generator.machine.borrow_mut(),
                crate::machine::Frame::Await {
                    phase: 0,
                    resume: generator.function.code.range,
                    destination,
                },
            )?;
        }
        return Ok(Some(completion));
    }
    generator.machine.borrow_mut().pop_frame();
    state.suspension = None;
    // A nested loop owns the next continuation.  Resume the enclosing loop
    // from its already-recorded post-inner-loop body range before returning
    // to the parent function; otherwise the inner loop's completion skips the
    // remaining outer iterations.
    if matches!(
        generator.machine.borrow().frames.frames.last(),
        Some(crate::machine::Frame::Loop { .. })
    ) {
        return resume_loop_frame(generator, state, completion);
    }
    if matches!(
        generator.machine.borrow().frames.frames.last(),
        Some(crate::machine::Frame::Try { .. })
    ) {
        if let Some(completion) = crate::generator::resume_try_frame(generator, state, completion.clone())? {
            return Ok(Some(completion));
        }
    }
    resume_generator_range(generator, state, frame.resume, completion).map(Some)
}

fn run_loop_after_yield(
    generator: &GeneratorData,
    frame: &LoopFrameResume,
) -> Result<crate::completion::Completion, VmError> {
    let mut phase = frame.phase;
    let mut resume = frame.phase_resume;
    loop {
        let (code, pc) = phase_execution(frame, phase, resume);
        let step = execute_loop_phase(generator, code, pc)?;
        if step.completion.is_suspension() {
            return suspend_loop_phase(generator, frame, phase, code, step);
        }
        if let Some(completion) = finish_loop_phase(generator, frame, phase, step.completion)? {
            return Ok(completion);
        }
        phase = next_loop_phase(phase, frame.post_test);
        resume = phase_range(frame, phase);
    }
}

fn execute_loop_phase(
    generator: &GeneratorData,
    range: crate::machine::CodeRange,
    pc: usize,
) -> Result<crate::vm::GeneratorStep, VmError> {
    if range.start == range.end {
        return Ok(crate::vm::GeneratorStep {
            completion: crate::completion::Completion::Normal,
            pc: 0,
            suspension: None,
        });
    }
    execute_loop_body_range(generator, range, pc)
}

fn phase_execution(
    frame: &LoopFrameResume,
    phase: crate::continuation::LoopPhase,
    resume: crate::machine::CodeRange,
) -> (crate::machine::CodeRange, usize) {
    let base = phase_range(frame, phase);
    if phase == crate::continuation::LoopPhase::Init || resume.code != base.code {
        return (resume, 0);
    }
    let pc = resume.start.saturating_sub(base.start) as usize;
    (base, pc)
}

fn suspend_loop_phase(
    generator: &GeneratorData,
    frame: &LoopFrameResume,
    phase: crate::continuation::LoopPhase,
    resume: crate::machine::CodeRange,
    step: crate::vm::GeneratorStep,
) -> Result<crate::completion::Completion, VmError> {
    let destination = step
        .suspension
        .as_ref()
        .and_then(resume_destination)
        .unwrap_or(frame.yield_dst);
    update_loop_phase_resume(generator, frame.body, phase, resume, step.pc, destination)?;
    if matches!(step.completion, crate::completion::Completion::Yield(_)) {
        return Ok(step.completion);
    }
    let point = loop_point(phase, resume, step.pc, destination, frame);
    Ok(wrap_loop_suspension(step.completion, point))
}

fn finish_loop_phase(
    generator: &GeneratorData,
    frame: &LoopFrameResume,
    phase: crate::continuation::LoopPhase,
    completion: crate::completion::Completion,
) -> Result<Option<crate::completion::Completion>, VmError> {
    match phase {
        crate::continuation::LoopPhase::Init => {
            finish_fragment(completion)?;
            refresh_loop_bindings(&frame.per_iteration);
            Ok(None)
        }
        crate::continuation::LoopPhase::Test => {
            Ok((!test_completion(completion)?).then_some(crate::completion::Completion::Normal))
        }
        crate::continuation::LoopPhase::Update => {
            finish_fragment(completion)?;
            Ok(None)
        }
        crate::continuation::LoopPhase::Body => finish_loop_body(generator, frame, completion),
    }
}

fn finish_loop_body(
    generator: &GeneratorData,
    frame: &LoopFrameResume,
    completion: crate::completion::Completion,
) -> Result<Option<crate::completion::Completion>, VmError> {
    match completion {
        crate::completion::Completion::Return(value) => Ok(Some(crate::completion::Completion::Return(value))),
        completion => match completion.into_loop_transition(&frame.label) {
            crate::completion::LoopTransition::Continue(value) => {
                store_loop_value(generator, frame.dst, value)?;
                refresh_loop_bindings(&frame.per_iteration);
                Ok(None)
            }
            crate::completion::LoopTransition::Break(value) => {
                store_loop_value(generator, frame.dst, value)?;
                Ok(Some(crate::completion::Completion::Normal))
            }
            crate::completion::LoopTransition::Propagate(completion) => Ok(Some(completion)),
        },
    }
}

fn next_loop_phase(
    phase: crate::continuation::LoopPhase,
    post_test: bool,
) -> crate::continuation::LoopPhase {
    match phase {
        crate::continuation::LoopPhase::Init if post_test => crate::continuation::LoopPhase::Body,
        crate::continuation::LoopPhase::Init | crate::continuation::LoopPhase::Update => {
            crate::continuation::LoopPhase::Test
        }
        crate::continuation::LoopPhase::Test => crate::continuation::LoopPhase::Body,
        crate::continuation::LoopPhase::Body => crate::continuation::LoopPhase::Update,
    }
}

fn phase_range(
    frame: &LoopFrameResume,
    phase: crate::continuation::LoopPhase,
) -> crate::machine::CodeRange {
    match phase {
        crate::continuation::LoopPhase::Body => frame.body,
        crate::continuation::LoopPhase::Test => frame.test,
        crate::continuation::LoopPhase::Update => frame.update,
        crate::continuation::LoopPhase::Init => frame.phase_resume,
    }
}

fn resume_destination(
    point: &crate::continuation::SuspensionPoint,
) -> Option<u16> {
    match point {
        crate::continuation::SuspensionPoint::Yield { src, .. }
        | crate::continuation::SuspensionPoint::Loop { yield_dst: src, .. } => Some(*src),
        crate::continuation::SuspensionPoint::YieldStar { dst, .. } => Some(*dst),
        crate::continuation::SuspensionPoint::Branch { yield_dst, .. } => Some(*yield_dst),
        crate::continuation::SuspensionPoint::Nested { inner, .. } => resume_destination(inner),
    }
}

fn loop_point(
    phase: crate::continuation::LoopPhase,
    resume: crate::machine::CodeRange,
    next: usize,
    destination: u16,
    frame: &LoopFrameResume,
) -> crate::continuation::SuspensionPoint {
    crate::continuation::SuspensionPoint::Loop {
        pc: 0,
        label: frame.label.clone(),
        body: frame.body,
        test: frame.test,
        update: frame.update,
        phase,
        phase_resume: crate::machine::CodeRange {
            code: resume.code,
            start: resume.start.saturating_add(next as u32),
            end: resume.end,
        },
        dst: frame.dst,
        yield_dst: destination,
        post_test: frame.post_test,
        per_iteration: std::rc::Rc::clone(&frame.per_iteration),
    }
}

fn wrap_loop_suspension(
    completion: crate::completion::Completion,
    point: crate::continuation::SuspensionPoint,
) -> crate::completion::Completion {
    match completion {
        crate::completion::Completion::Suspend(promise) => {
            crate::completion::Completion::SuspendAt(promise, point)
        }
        crate::completion::Completion::SuspendAt(promise, inner) => {
            crate::completion::Completion::SuspendAt(
                promise,
                crate::continuation::SuspensionPoint::Nested {
                    inner: Box::new(inner),
                    outer: Box::new(point),
                },
            )
        }
        other => other,
    }
}

fn execute_loop_body_range(
    generator: &GeneratorData,
    range: crate::machine::CodeRange,
    pc: usize,
) -> Result<crate::vm::GeneratorStep, VmError> {
    let store = generator
        .machine
        .borrow()
        .store
        .clone()
        .ok_or(VmError::MissingReturn)?;
    let code = store.code(range).ok_or(VmError::MissingReturn)?;
    let environment = machine_environment(generator)?;
    execute_with_generator_registers(generator, |registers| {
        crate::vm::execute_generator_code_step(
            code,
            registers,
            environment,
            pc,
            crate::completion::Completion::Normal,
        )
    })
}

fn finish_fragment(completion: crate::completion::Completion) -> Result<(), VmError> {
    match completion {
        crate::completion::Completion::Normal | crate::completion::Completion::Return(_) => Ok(()),
        completion => completion.into_vm_error().map(|_| ()),
    }
}

fn test_completion(completion: crate::completion::Completion) -> Result<bool, VmError> {
    match completion {
        crate::completion::Completion::Return(value) => Ok(crate::execute::is_truthy(&value)),
        crate::completion::Completion::Normal => Ok(false),
        completion => completion
            .into_vm_error()
            .map(|value| crate::execute::is_truthy(&value)),
    }
}

fn refresh_loop_bindings(slots: &[u16]) {
    let environment = crate::locals::current();
    for &slot in slots {
        let value = environment.get(slot);
        let _ = environment.replace_slot(slot, value);
    }
}

fn store_loop_value(
    generator: &GeneratorData,
    dst: u16,
    value: Option<Value>,
) -> Result<(), VmError> {
    if let Some(value) = value {
        crate::execute::write_value(&mut registers_mut(generator), dst, value);
    }
    Ok(())
}

fn update_loop_phase_resume(
    generator: &GeneratorData,
    loop_body: crate::machine::CodeRange,
    next_phase: crate::continuation::LoopPhase,
    range: crate::machine::CodeRange,
    next: usize,
    yield_dst: u16,
) -> Result<(), VmError> {
    let resume = crate::machine::CodeRange {
        code: range.code,
        start: range.start.saturating_add(next as u32),
        end: range.end,
    };
    let mut machine = generator.machine.borrow_mut();
    let index = machine.frames.frames.iter().rposition(|frame| {
        matches!(frame, crate::machine::Frame::Loop { body, .. } if *body == loop_body)
    }).ok_or(VmError::MissingReturn)?;
    let crate::machine::Frame::Loop {
        phase: current_phase,
        phase_resume,
        yield_dst: destination,
        ..
    } =
        &mut machine.frames.frames[index] else { return Err(VmError::MissingReturn) };
    *current_phase = next_phase;
    *phase_resume = resume;
    *destination = yield_dst;
    Ok(())
}

fn install_loop_frame_input(generator: &GeneratorData, input: &Value) -> bool {
    let Some(frame) = loop_frame_resume(generator) else {
        return false;
    };
    crate::execute::write_value(&mut registers_mut(generator), frame.yield_dst, input.clone());
    true
}
