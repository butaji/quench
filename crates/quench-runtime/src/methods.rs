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
    if spreads.iter().all(|spread| !spread) {
        if let Value::Function(function) = &callee {
            if let Some(value) =
                crate::loops::execute_crypto_integer_registers(function, &receiver, registers, args)
            {
                write_value(registers, *dst, value);
                return Ok(());
            }
        }
    }
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
    crate::execution_trace::named_call(key);
    if instruction.flags == 0 && execute_named_word(registers, instruction, cache).is_some() {
        return Ok(());
    }
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

fn execute_named_word(
    registers: &mut crate::register_file::RegisterFile,
    instruction: crate::ir::Instruction,
    cache: &std::cell::Cell<u64>,
) -> Option<()> {
    let receiver = registers.read_object(usize::from(instruction.b))?;
    let crate::vm::NamedCachedPayload::Word(slot) =
        crate::vm::get_named_cached_payload(receiver, cache)?
    else {
        return None;
    };
    // SAFETY: receiver owns the method slot and the register owns receiver
    // throughout this non-mutating call.
    let function = unsafe { &*(&*slot).function_ptr()? };
    let value = crate::functions::execute_shape_kernel_word(function, receiver)?;
    crate::execution_trace::call_method(0, false, true, "Function");
    crate::execution_trace::function_call_shape(
        function.params,
        function.code.capture_slots().len(),
        function.code.code(),
    );
    registers.write_number(usize::from(instruction.a), value);
    Some(())
}

pub(crate) fn execute_registered(
    registers: &mut crate::register_file::RegisterFile,
    instruction: crate::ir::Instruction,
    code: crate::machine::CodeView<'_>,
    pc: usize,
) -> Result<(), VmError> {
    if instruction.flags == 0 {
        return Err(VmError::MissingReturn);
    }
    let metadata_window = code.operand_window_at(pc);
    let first = instruction.a.saturating_sub(u16::from(instruction.flags));
    if instruction.flags == 6 {
        let consecutive = [first, first + 1, first + 2, first + 3, first + 4, first + 5];
        let argument_registers = metadata_window.unwrap_or(&consecutive);
        if let (Some(function), Some(receiver)) = (
            registers.function_ptr(usize::from(instruction.c)),
            registers.read_object(usize::from(instruction.b)),
        ) {
            if let Some(value) = crate::loops::execute_crypto_integer_words(
                function,
                receiver,
                registers,
                argument_registers,
            ) {
                crate::execution_trace::call_method(6, false, true, "Function");
                registers.write_number(usize::from(instruction.a), value);
                return Ok(());
            }
        }
    }
    let receiver = crate::locals::resolved_replacement(read_register(registers, instruction.b)?);
    let callee = read_register(registers, instruction.c)?;
    crate::execution_trace::call_method(
        usize::from(instruction.flags),
        false,
        true,
        call_target_name(&callee),
    );
    if instruction.flags == 6 {
        if let Value::Function(function) = &callee {
            let consecutive = [first, first + 1, first + 2, first + 3, first + 4, first + 5];
            let argument_registers = metadata_window.unwrap_or(&consecutive);
            if let Some(value) = crate::loops::execute_crypto_integer_registers(
                function,
                &receiver,
                registers,
                argument_registers,
            ) {
                write_value(registers, instruction.a, value);
                return Ok(());
            }
        }
    }
    let consecutive;
    let argument_registers = if let Some(window) = metadata_window {
        window
    } else {
        consecutive = (first..instruction.a).collect::<Vec<_>>();
        consecutive.as_slice()
    };
    let arguments: Vec<_> = argument_registers
        .iter()
        .map(|register| read_register(registers, *register))
        .collect::<Result<_, _>>()?;
    let value = execute_named_callee(
        callee,
        &receiver,
        &arguments,
        argument_registers,
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
            crate::vm::execute_host_capability_with_receiver(
                kind,
                Some(&bound.receiver),
                Some(receiver),
                arguments,
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
