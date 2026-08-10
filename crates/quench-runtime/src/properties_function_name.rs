pub(crate) fn execute_set_function_name(
    registers: &mut [crate::value::Value],
    op: &crate::ops::Op,
) -> Result<(), crate::execute::VmError> {
    let crate::ops::Op::SetFunctionName { function, name } = op else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    let value = crate::execute::read_register(registers, *function)?;
    crate::builtins::set_function_name(&value, name)
}
