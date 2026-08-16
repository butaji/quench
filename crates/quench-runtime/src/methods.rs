use crate::{
    execute::{
        execute_builtin_with_receiver, get_property_result, read_register, write_value, VmError,
    },
    ops::Op,
    value::Value,
};

pub(crate) fn execute(registers: &mut Vec<Value>, op: &Op) -> Result<(), VmError> {
    let Op::CallMethod {
        dst,
        object,
        key,
        callee,
        args,
    } = op
    else {
        return Err(VmError::NotCallable);
    };
    let receiver = read_register(registers, *object)?;
    let callee = resolved_callee(registers, *callee, &receiver, key)?;
    let arguments = args
        .iter()
        .map(|index| read_register(registers, *index))
        .collect::<Result<Vec<_>, _>>()?;
    let propagates = matches!(
        callee,
        Value::Builtin(crate::ops::Builtin::MapSet | crate::ops::Builtin::SetAdd)
    );
    let value = execute_callee(callee, &receiver, &arguments, args, registers)?;
    if propagates {
        crate::properties::propagate_updated_object(registers, Some(*object), &receiver, &value);
    }
    write_value(registers, *dst, value);
    Ok(())
}

fn resolved_callee(
    registers: &[Value],
    callee: Option<u16>,
    receiver: &Value,
    key: &str,
) -> Result<Value, VmError> {
    callee.map_or_else(
        || get_property_result(receiver, key),
        |callee| read_register(registers, callee),
    )
}

fn execute_callee(
    callee: Value,
    receiver: &Value,
    arguments: &[Value],
    args: &[u16],
    registers: &mut Vec<Value>,
) -> Result<Value, VmError> {
    let value = match callee {
        Value::Builtin(crate::ops::Builtin::String) => {
            execute_builtin_with_receiver(crate::ops::Builtin::String, arguments, None)?
        }
        Value::Builtin(
            builtin @ (crate::ops::Builtin::ObjectDefineProperty
            | crate::ops::Builtin::ObjectDefineProperties),
        ) => define_object_properties(builtin, arguments, args, registers)?,
        Value::Builtin(crate::ops::Builtin::ObjectSetPrototypeOf) => {
            execute_mutating_object_builtin(arguments, args, registers)?
        }
        Value::Builtin(builtin) => {
            execute_builtin_with_receiver(builtin, arguments, Some(receiver))?
        }
        Value::Function(function) => crate::functions::execute(&function, receiver, arguments)?,
        Value::BoundFunction(bound)
            if matches!(bound.target, Value::Builtin(crate::ops::Builtin::HostCapability(_))) =>
        {
            let Value::Builtin(crate::ops::Builtin::HostCapability(kind)) = bound.target else {
                unreachable!()
            };
            crate::vm::execute_host_capability_with_receiver(
                kind,
                Some(&bound.receiver),
                Some(receiver),
                arguments,
            )?
        }
        Value::BoundFunction(bound) => crate::functions::execute_bound(&bound, arguments)?,
        Value::Undefined => return Err(crate::vm::not_callable()),
        _ => return Err(crate::vm::not_callable()),
    };
    Ok(value)
}

fn define_object_properties(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
    args: &[u16],
    registers: &mut Vec<Value>,
) -> Result<Value, VmError> {
    let target = arguments.first().cloned().unwrap_or(Value::Undefined);
    let value = match builtin {
        crate::ops::Builtin::ObjectDefineProperty => crate::builtins::define_property(arguments)?,
        crate::ops::Builtin::ObjectDefineProperties => {
            crate::builtins::define_properties(arguments)?
        }
        _ => return Err(VmError::NotCallable),
    };
    crate::properties::propagate_updated_object(registers, args.first().copied(), &target, &value);
    Ok(value)
}

fn execute_mutating_object_builtin(
    arguments: &[Value],
    args: &[u16],
    registers: &mut Vec<Value>,
) -> Result<Value, VmError> {
    let target = arguments.first().cloned().unwrap_or(Value::Undefined);
    let value = crate::builtins::object::set_prototype_of(arguments)?;
    crate::properties::propagate_updated_object(registers, args.first().copied(), &target, &value);
    Ok(value)
}
