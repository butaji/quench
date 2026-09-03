pub(crate) fn execute_binding(
    registers: &mut crate::register_file::RegisterFile,
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
    let completion = crate::vm::execute_function_code_completion_in_current_frame(body, registers)?;
    if completion.is_suspension() {
        return Ok(completion);
    }
    if matches!(completion, crate::completion::Completion::Normal) && !close_normal {
        return Ok(completion);
    }
    close(iterator, completion)
}
