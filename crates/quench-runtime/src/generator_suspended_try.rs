fn suspended_try<'a>(generator: &'a GeneratorData, _state: &GeneratorState) -> Option<(&'a Op, &'a Op, &'a [Op])> {
    let index = machine_pc(generator).checked_sub(1)?;
    let op @ Op::Try { body, .. } = generator.function.ops().get(index)? else { return None; };
    let body = body.ops()?;
    let (yield_index, yield_op) = body.iter().enumerate().find(|(_, candidate)| suspended_try_op(candidate, generator))?;
    Some((op, yield_op, &body[yield_index + 1..]))
}

fn resume_suspended_try(generator: &GeneratorData, state: &mut GeneratorState, resume: crate::completion::Completion) -> Result<Option<crate::completion::Completion>, VmError> {
    let Some((try_op, yield_op, suffix)) = suspended_try(generator, state) else { return Ok(None); };
    let completion = execute_with_generator_registers(generator, |registers| resume_suspended_try_op(registers, yield_op, suffix, resume))?;
    if completion.is_suspension() { return Ok(Some(completion)); }
    execute_with_generator_registers(generator, |registers| complete_suspended_try(try_op, registers, completion)).map(Some)
}

fn complete_suspended_try(op: &Op, registers: &mut Vec<Value>, completion: crate::completion::Completion) -> Result<crate::completion::Completion, VmError> {
    let Op::Try { handler, finalizer, catch_slot, .. } = op else { return Err(VmError::MissingReturn); };
    let completion = handle_suspended_throw(registers, handler, *catch_slot, completion)?;
    let Some(finalizer) = finalizer else { return Ok(completion); };
    let Some(finalizer) = finalizer.ops() else { return Err(VmError::MissingReturn); };
    match crate::execute::execute_completion_in_place(finalizer, registers)? {
        crate::completion::Completion::Normal => Ok(completion), abrupt => Ok(abrupt),
    }
}

fn handle_suspended_throw(registers: &mut Vec<Value>, handler: &Option<crate::machine::FunctionCode>, catch_slot: Option<u16>, completion: crate::completion::Completion) -> Result<crate::completion::Completion, VmError> {
    let crate::completion::Completion::Throw(value) = completion else { return Ok(completion); };
    let Some(handler) = handler else { return Ok(crate::completion::Completion::Throw(value)); };
    if let Some(slot) = catch_slot { crate::locals::write(slot, value); }
    let Some(handler) = handler.ops() else { return Err(VmError::MissingReturn); };
    crate::execute::execute_completion_in_place(handler, registers)
}

fn throw_and_finish(generator: &GeneratorData, value: Value) -> Result<Value, VmError> {
    *generator.done.borrow_mut() = true;
    Err(VmError::Thrown(value))
}
