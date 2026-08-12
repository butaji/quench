pub(crate) fn execute_binding(
    registers: &mut Vec<Value>,
    op: &crate::ops::Op,
) -> Result<crate::completion::Completion, crate::execute::VmError> {
    let crate::ops::Op::IteratorBinding {
        iterator,
        body,
        close_normal,
    } = op
    else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    let iterator = read(registers, *iterator)?;
    let Some(body) = body.ops() else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    let completion = crate::execute::execute_completion_in_place(body, registers)?;
    if completion.is_suspension() {
        return Ok(completion);
    }
    if matches!(completion, crate::completion::Completion::Normal) && !close_normal {
        return Ok(completion);
    }
    close(iterator, completion)
}
