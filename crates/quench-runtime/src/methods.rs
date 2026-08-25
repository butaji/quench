use crate::{
    execute::{
        execute_builtin_with_receiver, get_property_result, read_register, write_value, VmError,
    },
    ops::Op,
    value::Value,
};

pub(crate) fn execute(
    registers: &mut crate::register_file::RegisterFile,
    op: &Op,
) -> Result<(), VmError> {
    let Op::CallMethod {
        dst,
        object,
        key,
        callee,
        args,
        spreads,
    } = op
    else {
        return Err(VmError::NotCallable);
    };
    let registered_callee = callee.is_some();
    let receiver = crate::locals::resolved_replacement(read_register(registers, *object)?);
    let callee = resolved_callee(registers, *callee, &receiver, key)?;
    crate::execution_trace::call_method(
        args.len(),
        spreads.iter().any(|spread| *spread),
        registered_callee,
        call_target_name(&callee),
    );
    let propagates = matches!(
        callee,
        Value::Builtin(crate::ops::Builtin::MapSet | crate::ops::Builtin::SetAdd)
    );
    let arguments = crate::vm::vm_ops::collect_call_arguments(registers, args, spreads)?;
    let value = execute_callee(callee, &receiver, &arguments, args, registers)?;
    if propagates {
        crate::properties::propagate_updated_object(registers, Some(*object), &receiver, &value);
    }
    write_value(registers, *dst, value);
    Ok(())
}

pub(crate) fn execute_named(
    registers: &mut crate::register_file::RegisterFile,
    instruction: crate::ir::Instruction,
    key: &str,
    cache: &std::cell::Cell<u64>,
) -> Result<(), VmError> {
    let receiver = crate::locals::resolved_replacement(read_register(registers, instruction.b)?);
    let callee = crate::vm::get_named_property_result(&receiver, key, cache)?;
    let argument = (instruction.flags == 1)
        .then(|| read_register(registers, instruction.c))
        .transpose()?;
    crate::execution_trace::call_method(
        usize::from(instruction.flags),
        false,
        true,
        call_target_name(&callee),
    );
    let argument_values = argument
        .as_ref()
        .map(std::slice::from_ref)
        .unwrap_or_default();
    let argument_registers = (instruction.flags == 1).then_some(instruction.c);
    let value = execute_named_callee(
        callee,
        &receiver,
        argument_values,
        argument_registers.as_slice(),
        instruction.b,
        registers,
    )?;
    write_value(registers, instruction.a, value);
    Ok(())
}

pub(crate) fn execute_registered(
    registers: &mut crate::register_file::RegisterFile,
    instruction: crate::ir::Instruction,
) -> Result<(), VmError> {
    if !(1..=2).contains(&instruction.flags) {
        return Err(VmError::MissingReturn);
    }
    let receiver = crate::locals::resolved_replacement(read_register(registers, instruction.b)?);
    let callee = read_register(registers, instruction.c)?;
    let first = instruction
        .a
        .checked_sub(u16::from(instruction.flags))
        .ok_or(VmError::MissingReturn)?;
    let arguments = [
        read_register(registers, first)?,
        (instruction.flags == 2)
            .then(|| read_register(registers, first + 1))
            .transpose()?
            .unwrap_or(Value::Undefined),
    ];
    let argument_registers = [first, first + 1];
    crate::execution_trace::call_method(
        usize::from(instruction.flags),
        false,
        true,
        call_target_name(&callee),
    );
    let value = execute_named_callee(
        callee,
        &receiver,
        &arguments[..usize::from(instruction.flags)],
        &argument_registers[..usize::from(instruction.flags)],
        instruction.b,
        registers,
    )?;
    write_value(registers, instruction.a, value);
    Ok(())
}

fn execute_named_callee(
    callee: Value,
    receiver: &Value,
    arguments: &[Value],
    argument_registers: &[u16],
    receiver_register: u16,
    registers: &mut crate::register_file::RegisterFile,
) -> Result<Value, VmError> {
    let propagates = matches!(
        callee,
        Value::Builtin(crate::ops::Builtin::MapSet | crate::ops::Builtin::SetAdd)
    );
    let value = execute_callee(callee, receiver, arguments, argument_registers, registers)?;
    if propagates {
        crate::properties::propagate_updated_object(
            registers,
            Some(receiver_register),
            receiver,
            &value,
        );
    }
    Ok(value)
}

fn call_target_name(value: &Value) -> &'static str {
    match value {
        Value::Function(_) => "Function",
        Value::Builtin(_) => "Builtin",
        Value::BoundFunction(_) => "BoundFunction",
        Value::Undefined => "Undefined",
        _ => "Other",
    }
}

fn resolved_callee(
    registers: &crate::register_file::RegisterFile,
    callee: Option<u16>,
    receiver: &Value,
    key: &str,
) -> Result<Value, VmError> {
    callee.map_or_else(
        || match get_property_result(receiver, key)? {
            Value::Undefined if key == "slice" && is_arguments_object(receiver) => {
                Ok(Value::Builtin(crate::ops::Builtin::ArraySlice))
            }
            value => Ok(value),
        },
        |callee| read_register(registers, callee),
    )
}

fn is_arguments_object(value: &Value) -> bool {
    matches!(value, Value::Array(values) if values.is_arguments())
}

fn execute_callee(
    callee: Value,
    receiver: &Value,
    arguments: &[Value],
    args: &[u16],
    registers: &mut crate::register_file::RegisterFile,
) -> Result<Value, VmError> {
    if !crate::conversion::is_callable(&callee) {
        return Err(crate::vm::not_callable());
    }
    let value = match &callee {
        Value::Builtin(crate::ops::Builtin::String) => {
            execute_builtin_with_receiver(crate::ops::Builtin::String, arguments, None)?
        }
        Value::Builtin(
            builtin @ (crate::ops::Builtin::ObjectDefineProperty
            | crate::ops::Builtin::ObjectDefineProperties),
        ) => define_object_properties(*builtin, arguments, args, registers)?,
        Value::Builtin(crate::ops::Builtin::ObjectSetPrototypeOf) => {
            execute_mutating_object_builtin(arguments, args, registers)?
        }
        Value::Builtin(builtin) => {
            execute_builtin_with_receiver(*builtin, arguments, Some(receiver))?
        }
        Value::Function(_) => crate::functions::execute_target(&callee, receiver, arguments)?,
        Value::BoundFunction(bound)
            if matches!(
                bound.target,
                Value::Builtin(crate::ops::Builtin::HostCapability(_))
            ) =>
        {
            let Value::Builtin(crate::ops::Builtin::HostCapability(kind)) = bound.target else {
                unreachable!()
            };
            let mut combined = bound.arguments.clone();
            combined.extend_from_slice(arguments);
            crate::vm::execute_host_capability_with_receiver(
                kind,
                Some(&bound.receiver),
                Some(receiver),
                &combined,
            )?
        }
        Value::BoundFunction(bound) => crate::functions::execute_bound(bound, arguments)?,
        Value::Undefined => return Err(crate::vm::not_callable()),
        _ => return Err(crate::vm::not_callable()),
    };
    Ok(value)
}

fn define_object_properties(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
    args: &[u16],
    registers: &mut crate::register_file::RegisterFile,
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
    registers: &mut crate::register_file::RegisterFile,
) -> Result<Value, VmError> {
    let target = arguments.first().cloned().unwrap_or(Value::Undefined);
    let value = crate::builtins::object::set_prototype_of(arguments)?;
    crate::properties::propagate_updated_object(registers, args.first().copied(), &target, &value);
    Ok(value)
}
