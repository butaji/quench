struct BranchFrameResume {
    branch_resume: crate::machine::CodeRange,
    resume: crate::machine::CodeRange,
    dst: Option<u16>,
    yield_dst: u16,
}

fn branch_frame_resume(generator: &GeneratorData) -> Option<BranchFrameResume> {
    let frame = generator.machine.borrow().frames.frames.last()?.clone();
    let crate::machine::Frame::Branch { branch_resume, resume, dst, yield_dst, .. } = frame else {
        return None;
    };
    Some(BranchFrameResume { branch_resume, resume, dst, yield_dst })
}

fn resume_branch_frame(
    generator: &GeneratorData,
    state: &mut GeneratorState,
    resume: crate::completion::Completion,
) -> Result<Option<crate::completion::Completion>, VmError> {
    let Some(frame) = branch_frame_resume(generator) else {
        return Ok(None);
    };
    if !matches!(resume, crate::completion::Completion::Normal) {
        generator.machine.borrow_mut().pop_frame();
        return Ok(Some(resume));
    }
    let _private = crate::private_environment::Guard::install_environment(
        generator.function.private_environment.clone(),
    );
    let _home = crate::super_scope::Guard::install(&generator.function, &generator.receiver);
    let _with = crate::with_scope::FunctionGuard::install(&generator.function.with_captures);
    let _locals = crate::locals::EnvironmentGuard::install(machine_environment(generator)?);
    let store = generator.machine.borrow().store.clone().ok_or(VmError::MissingReturn)?;
    let ops = store.code(frame.branch_resume).ok_or(VmError::MissingReturn)?;
    let step = execute_with_generator_registers(generator, |registers| {
        crate::vm::execute_code_completion_step_in_place(ops, registers)
    })?;
    let completion = step.completion;
    if completion.is_suspension() {
        advance_frame_after_yield(generator, frame.branch_resume, step.next)?;
        return Ok(Some(completion));
    }
    if let crate::completion::Completion::Return(value) = completion {
        if let Some(dst) = frame.dst {
            crate::execute::write_value(&mut registers_mut(generator), dst, value);
        } else {
            generator.machine.borrow_mut().pop_frame();
            return Ok(Some(crate::completion::Completion::Return(value)));
        }
    } else if !matches!(completion, crate::completion::Completion::Normal) {
        return Ok(Some(completion));
    }
    generator.machine.borrow_mut().pop_frame();
    // The machine PC already points at the enclosing function's continuation.
    // Resume through the ordinary generator step so lexical declarations
    // after the conditional are initialized normally (range execution can
    // bypass those declaration opcodes after an awaited branch).
    let step = execute_generator_step(generator, state, crate::completion::Completion::Normal)?;
    set_machine_pc(generator, step.pc);
    state.suspension = step.suspension;
    update_machine_frame(generator, state, &step.completion)?;
    update_await_frame(generator, state, &step.completion)?;
    Ok(Some(step.completion))
}

fn install_branch_frame_input(generator: &GeneratorData, input: &Value) -> bool {
    let Some(frame) = branch_frame_resume(generator) else {
        return false;
    };
    crate::execute::write_value(&mut registers_mut(generator), frame.yield_dst, input.clone());
    true
}
