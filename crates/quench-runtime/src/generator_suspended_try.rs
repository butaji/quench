fn suspended_try<'a>(
    generator: &'a GeneratorData,
    _state: &GeneratorState,
) -> Option<(&'a Op, &'a Op, crate::machine::CodeView<'a>)> {
    let index = machine_pc(generator).checked_sub(1)?;
    let op = generator.function.code.code()?.cold_at(index)?;
    match op {
        op @ Op::Try { body, .. } => try_yield_parts(generator, op, body),
        Op::ForOf { body, .. } | Op::ForIn { body, .. } | Op::Loop { body, .. } => {
            body.code()?.cold_ops().find_map(|(_, inner)| match inner {
                inner @ Op::Try { body, .. } => try_yield_parts(generator, inner, body),
                _ => None,
            })
        }
        _ => {
            generator
                .function
                .code
                .code()?
                .cold_ops()
                .find_map(|(candidate_index, candidate)| match candidate {
                    candidate @ Op::Try { body, .. }
                        if candidate_index == index
                            && body.code().is_some_and(|body| {
                            body.cold_ops()
                                .any(|(_, op)| matches!(op, Op::Await { .. }))
                        }) =>
                    {
                        try_yield_parts(generator, candidate, body)
                    }
                    _ => None,
                })
        }
    }
}

fn try_yield_parts<'a>(
    generator: &GeneratorData,
    op: &'a Op,
    body: &'a crate::machine::FunctionCode,
) -> Option<(&'a Op, &'a Op, crate::machine::CodeView<'a>)> {
    let body = body.code()?;
    let destination = crate::generator::peek_await_destination();
    let (yield_index, yield_op) = body
        .cold_ops()
        .find(|(_, candidate)| {
            suspended_try_op(candidate, generator)
                && match (destination, candidate) {
                    (Some(destination), Op::Await { dst, .. }) => *dst == destination,
                    (Some(_), Op::Yield { .. }) => false,
                    _ => true,
                }
        })?;
    Some((op, yield_op, body.slice(yield_index + 1, body.len())?))
}

fn resume_suspended_try(
    generator: &GeneratorData,
    state: &mut GeneratorState,
    resume: crate::completion::Completion,
) -> Result<Option<crate::completion::Completion>, VmError> {
    let _private = crate::private_environment::Guard::install_environment(
        generator.function.private_environment.clone(),
    );
    let _home = crate::super_scope::Guard::install(&generator.function, &generator.receiver);
    let _with = crate::with_scope::FunctionGuard::install(&generator.function.with_captures);
    let _locals = crate::locals::EnvironmentGuard::install(machine_environment(generator)?);
    let Some((try_op, yield_op, suffix)) = suspended_try(generator, state) else {
        return Ok(None);
    };
    let completion = execute_with_generator_registers(generator, |registers| {
        resume_suspended_try_op(registers, yield_op, suffix, resume)
    })?;
    if completion.is_suspension() {
        return Ok(Some(completion));
    }
    let completion = execute_with_generator_registers(generator, |registers| {
        complete_suspended_try(try_op, registers, completion)
    })?;
    if matches!(completion, crate::completion::Completion::Normal) {
        let resume = parent_resume_range(generator, state);
        return resume_generator_range(generator, state, resume, completion).map(Some);
    }
    Ok(Some(completion))
}

fn complete_suspended_try(
    op: &Op,
    registers: &mut crate::register_file::RegisterFile,
    completion: crate::completion::Completion,
) -> Result<crate::completion::Completion, VmError> {
    let Op::Try {
        handler,
        finalizer,
        catch_slot,
        ..
    } = op
    else {
        return Err(VmError::MissingReturn);
    };
    let completion = handle_suspended_throw(registers, handler, *catch_slot, completion)?;
    let Some(finalizer) = finalizer else {
        return Ok(completion);
    };
    let Some(finalizer) = finalizer.code() else {
        return Err(VmError::MissingReturn);
    };
    match crate::vm::execute_code_completion_in_current_frame(finalizer, registers)? {
        crate::completion::Completion::Normal => Ok(completion),
        abrupt => Ok(abrupt),
    }
}

fn handle_suspended_throw(
    registers: &mut crate::register_file::RegisterFile,
    handler: &Option<crate::machine::FunctionCode>,
    catch_slot: Option<u16>,
    completion: crate::completion::Completion,
) -> Result<crate::completion::Completion, VmError> {
    let crate::completion::Completion::Throw(value) = completion else {
        return Ok(completion);
    };
    let Some(handler) = handler else {
        return Ok(crate::completion::Completion::Throw(value));
    };
    if let Some(slot) = catch_slot {
        crate::locals::write(slot, value);
    }
    let Some(handler) = handler.code() else {
        return Err(VmError::MissingReturn);
    };
    crate::vm::execute_code_completion_in_current_frame(handler, registers)
}

fn throw_and_finish(generator: &GeneratorData, value: Value) -> Result<Value, VmError> {
    *generator.done.borrow_mut() = true;
    Err(VmError::Thrown(value))
}
