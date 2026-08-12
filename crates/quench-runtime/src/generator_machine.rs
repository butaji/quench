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
    let input = completion;
    let mut generated = None;
    let completion = machine.step(input.clone(), |_| {
        let step = crate::vm::execute_generator_step(
            generator.function.ops(),
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

fn ensure_try_frame(generator: &GeneratorData, state: &GeneratorState) {
    if suspended_try(generator, state).is_none()
        || generator.machine.borrow().frame_count() != 0
    {
        return;
    }
    generator
        .machine
        .borrow_mut()
        .push_frame(crate::machine::Frame::Try {
            phase: 0,
            body: crate::machine::CodeRange {
                code: crate::machine::CodeId(0),
                start: state.pc.saturating_sub(1) as u32,
                end: state.pc as u32,
            },
            handler: None,
            finalizer: None,
        });
}

fn ensure_control_frame(generator: &GeneratorData, state: &GeneratorState) {
    if (suspended_conditional(generator, state).is_none()
        && suspended_private_scope(generator, state).is_none())
        || generator.machine.borrow().frame_count() != 0
    {
        return;
    }
    generator
        .machine
        .borrow_mut()
        .push_frame(crate::machine::Frame::Control {
            phase: 0,
            body: crate::machine::CodeRange {
                code: crate::machine::CodeId(0),
                start: state.pc.saturating_sub(1) as u32,
                end: state.pc as u32,
            },
        });
}

fn update_await_frame(generator: &GeneratorData, state: &GeneratorState, completion: &crate::completion::Completion) {
    if !matches!(completion, crate::completion::Completion::Suspend(_)) {
        return;
    }
    generator
        .machine
        .borrow_mut()
        .push_frame(crate::machine::Frame::Await {
            phase: 0,
            resume: crate::machine::CodeRange {
                code: crate::machine::CodeId(0),
                start: state.pc as u32,
                end: state.pc as u32 + 1,
            },
        });
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
