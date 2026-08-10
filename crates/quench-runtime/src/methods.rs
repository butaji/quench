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
    let value = execute_callee(callee, *object, &receiver, &arguments, args, registers)?;
    write_value(registers, *dst, value);
    Ok(())
}

fn execute_callee(
    callee: Value,
    owner: u16,
    receiver: &Value,
    arguments: &[Value],
    args: &[u16],
    registers: &mut Vec<Value>,
) -> Result<Value, VmError> {
    let value = match callee {
        Value::Builtin(crate::ops::Builtin::String) => {
            execute_builtin_with_receiver(crate::ops::Builtin::String, arguments, None)?
        }
        Value::Builtin(crate::ops::Builtin::ObjectDefineProperty) => {
            let value = crate::builtins::define_property(arguments);
            crate::properties::propagate_updated_object(
                registers,
                owner,
                args.first().copied(),
                receiver,
                &value,
            );
            value
        }
        Value::Builtin(builtin) => {
            execute_builtin_with_receiver(builtin, arguments, Some(receiver))?
        }
        Value::Function(function) => crate::functions::execute(&function, receiver, arguments)?,
        Value::BoundFunction(bound) => crate::functions::execute_bound(&bound, arguments)?,
        Value::Undefined => execute_fallback(receiver, arguments)?,
        _ => return Err(VmError::NotCallable),
    };
    Ok(value)
}

fn execute_fallback(receiver: &Value, arguments: &[Value]) -> Result<Value, VmError> {
    let this_arg = arguments.first().unwrap_or(&Value::Undefined);
    let call_arguments = arguments.get(1..).unwrap_or_default();
    match receiver {
        Value::Builtin(builtin) => {
            execute_builtin_with_receiver(*builtin, call_arguments, Some(this_arg))
        }
        Value::Function(function) => crate::functions::execute(function, this_arg, call_arguments),
        Value::BoundFunction(bound) => crate::functions::execute_bound(bound, call_arguments),
        _ => Err(VmError::NotCallable),
    }
}
