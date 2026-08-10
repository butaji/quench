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

pub(crate) fn execute_set_function_name_dynamic(
    registers: &mut [crate::value::Value],
    op: &crate::ops::Op,
) -> Result<(), crate::execute::VmError> {
    let crate::ops::Op::SetFunctionNameDynamic {
        function,
        key,
        prefix,
    } = op
    else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    let value = crate::execute::read_register(registers, *function)?;
    let key = crate::execute::read_register(registers, *key)?;
    crate::builtins::set_dynamic_function_name(&value, &key, prefix.as_deref())
}
