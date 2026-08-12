fn suspended_iterator_binding<'a>(
    generator: &'a GeneratorData,
    state: &GeneratorState,
) -> Option<(&'a Op, &'a [Op], usize)> {
    let op @ Op::IteratorBinding { body, .. } = generator.function.ops().get(state.pc.checked_sub(1)?)? else {
        return None;
    };
    let body = body.ops()?;
    let index = state
        .nested
        .checked_sub(1)
        .filter(|index| *index < body.len())
        .or_else(|| body.iter().position(|op| matches!(op, Op::Yield { .. })))?;
    Some((op, body, index))
}

fn resume_suspended_iterator_binding(
    generator: &GeneratorData,
    state: &mut GeneratorState,
    resume: crate::completion::Completion,
) -> Result<Option<crate::completion::Completion>, VmError> {
    if let Some(completion) = resume_iterator_conditional(generator, state, resume.clone())? {
        return Ok(Some(completion));
    }
    let Some((op, body, index)) = suspended_iterator_binding(generator, state) else {
        return Ok(None);
    };
    if !matches!(resume, crate::completion::Completion::Normal) {
        state.nested = 0;
        return close_iterator_binding(op, &state.registers, resume).map(Some);
    }
    let completion = crate::execute::execute_completion_in_place(&body[index + 1..], &mut state.registers)?;
    if matches!(completion, crate::completion::Completion::Yield(_)) {
        if let Some(yielded) = next_yield(&body[index + 1..]) {
            state.nested = index + yielded + 2;
        }
        return Ok(Some(completion));
    }
    state.nested = 0;
    close_iterator_binding(op, &state.registers, completion).map(Some)
}

fn resume_iterator_conditional(
    generator: &GeneratorData,
    state: &mut GeneratorState,
    resume: crate::completion::Completion,
) -> Result<Option<crate::completion::Completion>, VmError> {
    let Some((binding, conditional, body, body_index, branch, yield_index)) =
        suspended_iterator_conditional(generator, state)
    else {
        return Ok(None);
    };
    if !matches!(resume, crate::completion::Completion::Normal) {
        return close_iterator_binding(binding, &state.registers, resume).map(Some);
    }
    let completion = crate::execute::execute_completion_in_place(
        &branch[yield_index + 1..],
        &mut state.registers,
    )?;
    let crate::completion::Completion::Return(value) = completion else {
        return Ok(Some(completion));
    };
    write_conditional_result(conditional, &mut state.registers, value)?;
    let completion = crate::execute::execute_completion_in_place(
        &body[body_index + 1..],
        &mut state.registers,
    )?;
    close_iterator_binding(binding, &state.registers, completion).map(Some)
}

fn write_conditional_result(
    conditional: &Op,
    registers: &mut Vec<Value>,
    value: Value,
) -> Result<(), VmError> {
    let Op::Conditional { dst, .. } = conditional else {
        return Err(VmError::MissingReturn);
    };
    crate::execute::write_value(registers, *dst, value);
    Ok(())
}

fn next_yield(ops: &[Op]) -> Option<usize> {
    ops.iter().position(|op| matches!(op, Op::Yield { .. }))
}

fn close_iterator_binding(
    op: &Op,
    registers: &[Value],
    completion: crate::completion::Completion,
) -> Result<crate::completion::Completion, VmError> {
    let Op::IteratorBinding { iterator, close_normal, .. } = op else {
        return Err(VmError::MissingReturn);
    };
    if matches!(completion, crate::completion::Completion::Normal) && !close_normal {
        return Ok(completion);
    }
    crate::collections::iterator::close(crate::execute::read_register(registers, *iterator)?, completion)
}

fn install_iterator_binding_input(generator: &GeneratorData, state: &mut GeneratorState, input: &Value) -> bool {
    let Some((_, body, index)) = suspended_iterator_binding(generator, state) else {
        return false;
    };
    let Some(Op::Yield { src }) = body.get(index) else {
        return false;
    };
    crate::execute::write_value(&mut state.registers, *src, input.clone());
    true
}

fn suspended_iterator_conditional<'a>(
    generator: &'a GeneratorData,
    state: &GeneratorState,
) -> Option<(&'a Op, &'a Op, &'a [Op], usize, &'a [Op], usize)> {
    let binding @ Op::IteratorBinding { body, .. } = generator.function.ops().get(state.pc.checked_sub(1)?)? else {
        return None;
    };
    let body = body.ops()?;
    let (body_index, conditional @ Op::Conditional { condition, consequent, alternate, .. }) = body
        .iter()
        .enumerate()
        .find(|(_, op)| matches!(op, Op::Conditional { .. }))?
    else {
        return None;
    };
    let test = crate::execute::read_register(&state.registers, *condition).ok()?;
    let branch = if crate::execute::is_truthy(&test) { consequent } else { alternate }.ops()?;
    let yield_index = branch.iter().position(|op| matches!(op, Op::Yield { .. }))?;
    Some((binding, conditional, body, body_index, branch, yield_index))
}

fn install_iterator_conditional_input(
    generator: &GeneratorData,
    state: &mut GeneratorState,
    input: &Value,
) -> bool {
    let Some((_, _, _, _, branch, index)) = suspended_iterator_conditional(generator, state) else {
        return false;
    };
    let Some(Op::Yield { src }) = branch.get(index) else {
        return false;
    };
    crate::execute::write_value(&mut state.registers, *src, input.clone());
    true
}
