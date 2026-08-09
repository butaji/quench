use crate::{
    execute::{execute_builtin_with_receiver, get_property, read_register, write_value, VmError},
    ops::Op,
    value::Value,
};

pub(crate) fn execute(registers: &mut Vec<Value>, op: &Op) -> Result<(), VmError> {
    let Op::CallMethod {
        dst,
        object,
        key,
        args,
    } = op
    else {
        return Err(VmError::NotCallable);
    };
    let receiver = read_register(registers, *object)?;
    let callee = get_property(&receiver, key);
    let arguments = args
        .iter()
        .map(|index| read_register(registers, *index))
        .collect::<Result<Vec<_>, _>>()?;
    let Value::Builtin(builtin) = callee else {
        return Err(VmError::NotCallable);
    };
    let value = execute_builtin_with_receiver(builtin, &arguments, Some(&receiver))?;
    write_value(registers, *dst, value);
    Ok(())
}
