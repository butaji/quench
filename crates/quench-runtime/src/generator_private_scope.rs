fn suspended_conditional<'a>(
    generator: &'a GeneratorData,
    _state: &GeneratorState,
) -> Option<(&'a Op, crate::machine::CodeView<'a>)> {
    let Op::Conditional {
        condition,
        consequent,
        alternate,
        ..
    } = generator.function.code.code()?.cold_at(machine_pc(generator).checked_sub(1)?)?
    else {
        return None;
    };
    let test = crate::execute::read_register(&registers(generator), *condition).ok()?;
    let branch = if crate::execute::is_truthy(&test) {
        consequent
    } else {
        alternate
    };
    let branch = branch.code()?;
    let (index, op) = branch.find_cold(|op| matches!(op, Op::Yield { .. }))?;
    Some((op, branch.slice(index + 1, branch.len())?))
}

fn resume_suspended_conditional(
    generator: &GeneratorData,
    state: &mut GeneratorState,
    resume: crate::completion::Completion,
) -> Result<Option<crate::completion::Completion>, VmError> {
    let Some((_, suffix)) = suspended_conditional(generator, state) else {
        return Ok(None);
    };
    if !matches!(resume, crate::completion::Completion::Normal) {
        return Ok(Some(resume));
    }
    let completion = execute_with_generator_registers(generator, |registers| {
        crate::vm::execute_code_completion_in_current_frame(suffix, registers)
    })?;
    Ok(Some(completion))
}

fn capture_suspended_private_environment(
    generator: &GeneratorData,
    state: &mut GeneratorState,
    completion: &crate::completion::Completion,
) {
    if !matches!(completion, crate::completion::Completion::Yield(_)) {
        return;
    }
    let Some(Op::PrivateScope { .. }) = generator.function.code.code().and_then(|code| code.cold_at(machine_pc(generator).wrapping_sub(1)))
    else {
        return;
    };
    if let Some(environment) = crate::private_environment::take_suspended() {
        state.private_environment = Some(environment);
    }
}

fn install_nested_resume_input(
    generator: &GeneratorData,
    state: &mut GeneratorState,
    input: Value,
) {
    if let Some((Op::Yield { src }, _)) = suspended_conditional(generator, state) {
        crate::execute::write_value(&mut registers_mut(generator), *src, input);
        return;
    }
    if install_iterator_binding_input(generator, state, &input) {
        return;
    }
    if install_iterator_conditional_input(generator, state, &input) {
        return;
    }
    let Some((_, body, index)) = suspended_private_scope(generator, state) else {
        return;
    };
    if let Some(Op::Yield { src }) = body.cold_at(index) {
        crate::execute::write_value(&mut registers_mut(generator), *src, input);
    }
}

/// Locates a class body (`PrivateScope`) suspended on a `yield`, returning the
/// private names, the body, and the index of the `Yield` op to resume from.
fn suspended_private_scope<'a>(
    generator: &'a GeneratorData,
    state: &GeneratorState,
) -> Option<(&'a [crate::facts::PrivateNameId], crate::machine::CodeView<'a>, usize)> {
    let Op::PrivateScope { names, body, .. } = generator.function.code.code()?.cold_at(machine_pc(generator).checked_sub(1)?)?
    else {
        return None;
    };
    let body = body.code()?;
    let index = match state.nested.checked_sub(1) {
        Some(index) if index < body.len() => index,
        _ => body.position_cold(|op| matches!(op, Op::Yield { .. }))?,
    };
    Some((names, body, index))
}

fn resume_suspended_private_scope(
    generator: &GeneratorData,
    state: &mut GeneratorState,
    resume: crate::completion::Completion,
) -> Result<Option<crate::completion::Completion>, VmError> {
    let Some((names, body, index)) = suspended_private_scope(generator, state) else {
        return Ok(None);
    };
    if !matches!(resume, crate::completion::Completion::Normal) {
        state.nested = 0;
        state.private_environment = None;
        return Ok(Some(resume));
    }
    let _scope = match &state.private_environment {
        Some(environment) => {
            crate::private_environment::Guard::install_environment(environment.clone())
        }
        None => crate::private_environment::Guard::install(names, &[]),
    };
    let suffix = body.slice(index + 1, body.len()).ok_or(VmError::MissingReturn)?;
    let step = execute_with_generator_registers(generator, |registers| {
        crate::vm::execute_code_completion_step_in_place(suffix, registers)
    })?;
    let completion = step.completion;
    if matches!(completion, crate::completion::Completion::Yield(_)) {
        state.nested = index + step.next + 1;
        return Ok(Some(completion));
    }
    state.nested = 0;
    state.private_environment = None;
    finish_private_scope_resume(generator, state, completion).map(Some)
}

fn finish_private_scope_resume(
    generator: &GeneratorData,
    state: &mut GeneratorState,
    completion: crate::completion::Completion,
) -> Result<crate::completion::Completion, VmError> {
    if !matches!(completion, crate::completion::Completion::Normal) {
        return Ok(completion);
    }
    let _home = crate::super_scope::Guard::install(&generator.function, &generator.receiver);
    let _with_scope = crate::with_scope::FunctionGuard::install(&generator.function.with_captures);
    let step = execute_with_generator_registers(generator, |registers| {
        crate::vm::execute_generator_code_step(
            generator.function.code.code().ok_or(VmError::MissingReturn)?, registers, machine_environment(generator)?, machine_pc(generator),
            crate::completion::Completion::Normal,
        )
    })?;
    set_machine_pc(generator, step.pc);
    state.suspension = step.suspension;
    Ok(step.completion)
}
