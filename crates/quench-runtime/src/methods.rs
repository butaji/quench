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
    let value = match callee {
        Value::Builtin(builtin) => {
            execute_builtin_with_receiver(builtin, &arguments, Some(&receiver))?
        }
        Value::Function(function) => crate::functions::execute(&function, &arguments)?,
        Value::Undefined if matches!(receiver, Value::Builtin(_) | Value::Function(_)) => {
            // Fallback: when callee is undefined, call the receiver directly.
            // This handles `toString.call(x)` where toString is a Builtin/Function
            // without an explicit `call` property.
            match &receiver {
                Value::Builtin(builtin) => {
                    execute_builtin_with_receiver(*builtin, &arguments, Some(&receiver))?
                }
                Value::Function(function) => crate::functions::execute(function, &arguments)?,
                _ => return Err(VmError::NotCallable),
            }
        }
        _ => return Err(VmError::NotCallable),
    };
    write_value(registers, *dst, value);
    Ok(())
}
