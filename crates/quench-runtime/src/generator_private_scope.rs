fn suspended_conditional<'a>(
    generator: &'a GeneratorData,
    state: &GeneratorState,
) -> Option<(&'a Op, &'a [Op])> {
    let Op::Conditional {
        condition,
        consequent,
        alternate,
        ..
    } = generator.function.ops().get(state.pc.checked_sub(1)?)?
    else {
        return None;
    };
    let test = crate::execute::read_register(&registers(generator), *condition).ok()?;
    let branch = if crate::execute::is_truthy(&test) {
        consequent
    } else {
        alternate
    };
    let branch = branch.ops()?;
    let index = branch
        .iter()
        .position(|op| matches!(op, Op::Yield { .. }))?;
    Some((&branch[index], &branch[index + 1..]))
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
    let completion = crate::execute::execute_completion_in_place(suffix, &mut registers_mut(generator))?;
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
    let Some(Op::PrivateScope { .. }) = generator.function.ops().get(state.pc.wrapping_sub(1))
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
    if let Some(Op::Yield { src }) = body.get(index) {
        crate::execute::write_value(&mut registers_mut(generator), *src, input);
    }
}

/// Locates a class body (`PrivateScope`) suspended on a `yield`, returning the
/// private names, the body, and the index of the `Yield` op to resume from.
fn suspended_private_scope<'a>(
    generator: &'a GeneratorData,
    state: &GeneratorState,
) -> Option<(&'a [crate::facts::PrivateNameId], &'a [Op], usize)> {
    let Op::PrivateScope { names, body } = generator.function.ops().get(state.pc.checked_sub(1)?)?
    else {
        return None;
    };
    let body = body.ops()?;
    let index = match state.nested.checked_sub(1) {
        Some(index) if index < body.len() => index,
        _ => body.iter().position(|op| matches!(op, Op::Yield { .. }))?,
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
        None => crate::private_environment::Guard::install(names),
    };
    let suffix = &body[index + 1..];
    let completion = crate::execute::execute_completion_in_place(suffix, &mut registers_mut(generator))?;
    if matches!(completion, crate::completion::Completion::Yield(_)) {
        let yielded = suffix
            .iter()
            .position(|op| matches!(op, Op::Yield { .. }))
            .ok_or(VmError::MissingReturn)?;
        state.nested = index + yielded + 2;
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
    let _with_scope = crate::with_scope::FunctionGuard::isolate();
    let step = crate::vm::execute_generator_step(
        generator.function.ops(),
        &mut registers_mut(generator),
        state.environment.clone(),
        state.pc,
        crate::completion::Completion::Normal,
    )?;
    state.pc = step.pc;
    state.suspension = step.suspension;
    Ok(step.completion)
}
