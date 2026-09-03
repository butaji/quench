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
    site: Option<&std::cell::RefCell<crate::quickening::QuickeningSite<4>>>,
) -> Result<Option<crate::completion::Completion>, VmError> {
    crate::execution_trace::named_call(key);
    if instruction.flags <= 1 {
        if let Some(outcome) = execute_named_word_fast(registers, instruction, key, cache)? {
            crate::execution_trace::call_method(
                usize::from(instruction.flags),
                false,
                true,
                "Function",
            );
            match outcome {
                NamedFunctionCall::Complete { value, specialized } => {
                    if specialized {
                        crate::execution_trace::kernel("CallKnown", false);
                    }
                    write_value(registers, instruction.a, value);
                    return Ok(None);
                }
                NamedFunctionCall::Continue {
                    function,
                    receiver,
                    argument,
                } => {
                    let mut arguments = crate::completion::CallArguments::new();
                    arguments.extend(argument);
                    return Ok(Some(crate::vm::vm_ops::take_call_continuation(
                        registers,
                        instruction.a,
                        Value::Function(function),
                        receiver,
                        arguments,
                    )));
                }
            }
        }
    }
    let receiver = crate::locals::resolved_replacement(read_register(registers, instruction.b)?);
    let callee = named_known_callee(&receiver, key, cache).map_or_else(
        || crate::vm::get_named_property_result(&receiver, key, cache),
        Ok,
    )?;
    if observe_callable(site, &callee) {
        crate::execution_trace::kernel("CallIC", false);
    }
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

enum NamedFunctionCall {
    Complete {
        value: Value,
        specialized: bool,
    },
    Continue {
        function: std::rc::Rc<crate::value::FunctionValue>,
        receiver: Value,
        argument: Option<Value>,
    },
}

fn execute_named_word_fast(
    registers: &mut crate::register_file::RegisterFile,
    instruction: crate::ir::Instruction,
    key: &str,
    cache: &std::cell::Cell<u64>,
) -> Result<Option<NamedFunctionCall>, VmError> {
    let receiver = crate::locals::resolved_replacement(read_register(registers, instruction.b)?);
    let Value::Object(object) = &receiver else {
        return Ok(None);
    };
    let Some(crate::vm::NamedCachedPayload::Word(slot)) =
        crate::vm::get_named_cached_payload(object, key, cache).or_else(|| {
            crate::vm::proven_own_word(object, key)
                .map(|slot| crate::vm::NamedCachedPayload::Word(std::ptr::from_ref(slot)))
        })
    else {
        return Ok(None);
    };
    let Some(pointer) = (unsafe { (&*slot).function_ptr() }) else {
        return Ok(None);
    };
    // The slot word roots the function for this call. Retain one temporary Rc
    // so the direct-frame path can avoid an intermediate `Value::Function`
    // decode while preserving ordinary call semantics.
    unsafe { std::rc::Rc::increment_strong_count(pointer) };
    let function = unsafe { std::rc::Rc::from_raw(pointer) };
    let argument = (instruction.flags == 1)
        .then(|| read_register(registers, instruction.c))
        .transpose()?;
    let arguments = argument
        .as_ref()
        .map(std::slice::from_ref)
        .unwrap_or_default();
    if crate::functions::direct_call_eligible(&function) {
        // The named-property cache already guards the receiver layout and
        // physical slot. Since this slot owns the callable identity, probing
        // a second weak callable cache here only repeats the same proof.
        let value = crate::functions::execute_direct(&function, &receiver, arguments)?;
        return Ok(Some(NamedFunctionCall::Complete {
            value,
            specialized: false,
        }));
    }
    if let Some(value) = crate::functions::try_execute_specialized(&function, &receiver, arguments)?
    {
        return Ok(Some(NamedFunctionCall::Complete {
            value,
            specialized: true,
        }));
    }
    Ok(Some(NamedFunctionCall::Continue {
        function,
        receiver,
        argument,
    }))
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
    if observe_callable(code.quickening_site(pc), &callee) {
        crate::execution_trace::kernel("CallIC", false);
    }
    if instruction.flags == 1 {
        if let Value::Builtin(
            builtin @ (crate::ops::Builtin::StringCharAt | crate::ops::Builtin::StringCharCodeAt),
        ) = callee
        {
            let argument = read_register(registers, first)?;
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
    // Registered calls are the common fixed-arity path for builtins and
    // methods. Keep their arguments in the bounded inline representation used
    // by continuations instead of allocating a fresh Vec on every call.
    let mut arguments = crate::completion::CallArguments::with_capacity(argument_registers.len());
    for register in argument_registers {
        arguments.push(read_register(registers, *register)?);
    }
    finish_named_call_owned(
        registers,
        instruction.a,
        instruction.b,
        callee,
        receiver,
        arguments,
        argument_registers,
    )
}

/// Record a callable identity only after the ordinary method/property gateway
/// has produced the callee. This keeps accessors, proxies, and receiver
/// effects on their complete semantic path while still feeding the site IC.
#[inline(always)]
fn observe_callable(
    site: Option<&std::cell::RefCell<crate::quickening::QuickeningSite<4>>>,
    callee: &Value,
) -> bool {
    let Value::Function(function) = callee else {
        return false;
    };
    if !crate::functions::direct_call_eligible(function) {
        return false;
    }
    site.is_some_and(|site| {
        matches!(
            site.borrow_mut().observe_callable(function),
            crate::quickening::QuickeningDecision::GuardedCallHit
        )
    })
}

fn named_known_callee(receiver: &Value, key: &str, cache: &std::cell::Cell<u64>) -> Option<Value> {
    let Value::Object(object) = receiver else {
        return None;
    };
    let crate::vm::NamedCachedPayload::Word(slot) =
        crate::vm::get_named_cached_payload(object, key, cache)?
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
        if crate::functions::direct_call_eligible(function) {
            let value = crate::functions::execute_direct(function, &receiver, arguments)?;
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

fn finish_named_call_owned(
    registers: &mut crate::register_file::RegisterFile,
    destination: u16,
    receiver_register: u16,
    callee: Value,
    receiver: Value,
    arguments: crate::completion::CallArguments,
    argument_registers: &[u16],
) -> Result<Option<crate::completion::Completion>, VmError> {
    if let Value::Function(function) = &callee {
        if let Some(value) =
            crate::functions::try_execute_specialized(function, &receiver, &arguments)?
        {
            crate::execution_trace::kernel("CallKnown", false);
            write_value(registers, destination, value);
            return Ok(None);
        }
        if crate::functions::direct_call_eligible(function) {
            let value = crate::functions::execute_direct(function, &receiver, &arguments)?;
            write_value(registers, destination, value);
            return Ok(None);
        }
        return Ok(Some(crate::vm::vm_ops::take_call_continuation(
            registers,
            destination,
            callee,
            receiver,
            arguments,
        )));
    }
    let value = execute_named_callee(
        callee,
        &receiver,
        &arguments,
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
        || match get_property_result(receiver, key)? {
            Value::Undefined if key == "slice" && is_arguments_object(receiver) => {
                Ok(Value::Builtin(crate::ops::Builtin::ArraySlice))
            }
            value => Ok(value),
        },
        |callee| {
            let value = read_register(registers, callee)?;
            if matches!(value, Value::Undefined) {
                get_property_result(receiver, key)
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
