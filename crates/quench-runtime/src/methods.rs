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
    execute_call_method(registers, op)
}

fn execute_call_method(
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

#[inline(never)]
pub(crate) fn execute_named(
    registers: &mut crate::register_file::RegisterFile,
    instruction: crate::ir::Instruction,
    key: &str,
    cache: &std::cell::Cell<u64>,
) -> Result<Option<crate::completion::Completion>, VmError> {
    crate::execution_trace::named_call(key);
    if instruction.flags == 0 && execute_named_word(registers, instruction, cache).is_some() {
        return Ok(None);
    }
    let receiver = crate::locals::resolved_replacement(read_register(registers, instruction.b)?);
    let callee = named_known_callee(&receiver, cache)
        .or_else(|| array_known_callee(&receiver, key))
        .map_or_else(
            || crate::vm::get_named_property_result(&receiver, key, cache),
            Ok,
        )?;
    let argument = (instruction.flags == 1)
        .then(|| read_register(registers, instruction.c))
        .transpose()?;
    crate::execution_trace::call_method(
        usize::from(instruction.flags),
        false,
        true,
        call_target_name(&callee),
    );
    let argument_values = argument.as_slice();
    let argument_registers = (instruction.flags == 1).then_some(instruction.c);
    finish_named_call(
        registers,
        instruction.a,
        instruction.b,
        callee,
        receiver,
        argument_values,
        argument_registers.as_slice(),
    )
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
) -> Result<Option<crate::completion::Completion>, VmError> {
    if instruction.flags == 0 {
        return Err(VmError::MissingReturn);
    }
    let metadata_window = code.operand_window_at(pc);
    let first = instruction.a.saturating_sub(u16::from(instruction.flags));
    let receiver = crate::locals::resolved_replacement(read_register(registers, instruction.b)?);
    let callee = read_register(registers, instruction.c)?;
    if instruction.flags <= 4
        && matches!(callee, Value::Builtin(crate::ops::Builtin::ArrayPush))
    {
        // The intrinsic has already been resolved, so collect only the small
        // fixed argument window on the stack. Larger/dynamic calls retain the
        // ordinary Vec-backed path below, preserving all generic semantics.
        let argument_count = usize::from(instruction.flags);
        let mut arguments: [Value; 4] = std::array::from_fn(|_| Value::Undefined);
        for (offset, argument) in arguments.iter_mut().take(argument_count).enumerate() {
            *argument = read_register(
                registers,
                instruction
                    .a
                    .saturating_sub(u16::from(instruction.flags))
                    .saturating_add(offset as u16),
            )?;
        }
        let value = crate::builtins::array_push(
            Some(&receiver),
            &arguments[..argument_count],
        )?;
        write_value(registers, instruction.a, value);
        return Ok(None);
    }
    if instruction.flags == 1 {
        if let Value::Builtin(
            builtin @ (crate::ops::Builtin::StringCharAt | crate::ops::Builtin::StringCharCodeAt),
        ) = callee
        {
            let argument = read_register(registers, first)?;
            if let Value::Number(index) = argument {
                if let Some(value) = crate::strings::char_code_at_number(&receiver, index) {
                    registers.write_number(usize::from(instruction.a), value);
                    return Ok(None);
                }
            }
            let value = execute_builtin_with_receiver(builtin, &[argument], Some(&receiver))?;
            write_value(registers, instruction.a, value);
            return Ok(None);
        }
    }
    crate::execution_trace::call_method(
        usize::from(instruction.flags),
        false,
        true,
        call_target_name(&callee),
    );
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
    finish_named_call(
        registers,
        instruction.a,
        instruction.b,
        callee,
        receiver,
        &arguments,
        argument_registers,
    )
}

fn named_known_callee(receiver: &Value, cache: &std::cell::Cell<u64>) -> Option<Value> {
    let Value::Object(object) = receiver else {
        return None;
    };
    let crate::vm::NamedCachedPayload::Word(slot) =
        crate::vm::get_named_cached_payload(object, cache)?
    else {
        return None;
    };
    // SAFETY: receiver owns the method slot for this call.
    let callee = unsafe { &*slot }.load();
    matches!(
        callee,
        Value::Function(_) | Value::Builtin(_) | Value::BoundFunction(_)
    )
    .then_some(callee)
}

#[inline]
fn array_known_callee(receiver: &Value, key: &str) -> Option<Value> {
    let Value::Array(array) = receiver else { return None };
    if !array.is_packed_ordinary() || !crate::builtins::array_prototype_is_clean() {
        return None;
    }
    crate::arrays::packed_method(key).map(Value::Builtin)
}

fn finish_named_call(
    registers: &mut crate::register_file::RegisterFile,
    destination: u16,
    receiver_register: u16,
    callee: Value,
    receiver: Value,
    arguments: &[Value],
    argument_registers: &[u16],
) -> Result<Option<crate::completion::Completion>, VmError> {
    if let Value::Function(function) = &callee {
        if let Some(value) =
            crate::functions::try_execute_specialized(function, &receiver, arguments)?
        {
            crate::execution_trace::kernel("CallKnown", false);
            write_value(registers, destination, value);
            return Ok(None);
        }
        return Ok(Some(crate::vm::vm_ops::take_call_continuation(
            registers,
            destination,
            callee,
            receiver,
            arguments.to_vec().into(),
        )));
    }
    let value = execute_named_callee(
        callee,
        &receiver,
        arguments,
        argument_registers,
        receiver_register,
        registers,
    )?;
    write_value(registers, destination, value);
    Ok(None)
}

fn execute_named_callee(
    callee: Value,
    receiver: &Value,
    arguments: &[Value],
    argument_registers: &[u16],
    receiver_register: u16,
    registers: &mut crate::register_file::RegisterFile,
) -> Result<Value, VmError> {
    if !crate::conversion::is_callable(&callee) {
        return Err(crate::vm::not_callable());
    }
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
        || {
            if let Some(value) = array_known_callee(receiver, key) {
                return Ok(value);
            }
            match get_property_result(receiver, key)? {
                Value::Undefined if key == "slice" && is_arguments_object(receiver) => {
                    Ok(Value::Builtin(crate::ops::Builtin::ArraySlice))
                }
                value => Ok(value),
            }
        },
        |callee| {
            let value = read_register(registers, callee)?;
            if matches!(value, Value::Undefined) {
                array_known_callee(receiver, key)
                    .map_or_else(|| get_property_result(receiver, key), Ok)
            } else {
                Ok(value)
            }
        },
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
            let mut combined = bound.arguments.clone();
            combined.extend_from_slice(arguments);
            let effective_receiver = if matches!(receiver, Value::Undefined)
                && bound.properties.borrow().iter().any(|(key, value)| {
                    key == "\0vm_compiled_function" && matches!(value, Value::Boolean(true))
                }) {
                Value::BoundFunction(bound.clone())
            } else {
                receiver.clone()
            };
            crate::vm::execute_host_capability_with_receiver(
                kind,
                Some(&bound.receiver),
                Some(&effective_receiver),
                &combined,
            )?
        }
        Value::BoundFunction(bound)
            if matches!(bound.target, Value::Builtin(_))
                && matches!(bound.receiver, Value::HostCapability(_)) =>
        {
            // Intrinsic prototype methods are represented as bound values to
            // carry their realm token, not a JavaScript `this`.  A normal
            // member call supplies the receiver at this boundary; ordinary
            // user-created bound builtins retain their captured receiver.
            let Value::Builtin(builtin) = bound.target else {
                unreachable!()
            };
            execute_builtin_with_receiver(builtin, arguments, Some(receiver))?
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
