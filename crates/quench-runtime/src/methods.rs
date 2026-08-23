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
        spreads,
    } = op
    else {
        return Err(VmError::NotCallable);
    };
    let receiver = read_register(registers, *object)?;
    let callee = resolved_callee(registers, *callee, &receiver, key)?;
    eprintln!("TRACE METHOD key={} callee={:?} receiver={:?} argc={}", key, callee, receiver, args.len());
    let arguments = crate::vm::vm_ops::collect_call_arguments(registers, args, spreads)?;
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
    registers: &mut Vec<Value>,
) -> Result<Value, VmError> {
    if let Value::BindingCell(cell) = &callee {
        return execute_callee(cell.borrow().clone(), receiver, arguments, args, registers);
    }
    match &callee {
        Value::Builtin(crate::ops::Builtin::HostCapability(kind)) => {
            let capability_receiver =
                crate::vm::realm_token(crate::vm::current_context_or_default().realm())
                    .ok_or(VmError::NotCallable)?;
            let js_receiver = if matches!(kind, crate::ops::HostCapabilityKind::Custom(1))
                && matches!(receiver, Value::Undefined)
            {
                crate::vm::current_global_object()
            } else {
                receiver.clone()
            };
            crate::vm::execute_host_capability_with_receiver(
                *kind,
                Some(&capability_receiver),
                Some(&js_receiver),
                arguments,
            )
        }
        Value::HostCapability(capability) => {
            crate::vm::execute_host_capability_with_receiver(
                capability.descriptor.kind,
                Some(&callee),
                Some(receiver),
                arguments,
            )
        }
        Value::Builtin(crate::ops::Builtin::String) => {
            execute_builtin_with_receiver(crate::ops::Builtin::String, arguments, None)
        }
        Value::Builtin(
            builtin @ (crate::ops::Builtin::ObjectDefineProperty
            | crate::ops::Builtin::ObjectDefineProperties),
        ) => define_object_properties(*builtin, arguments, args, registers),
        Value::Builtin(crate::ops::Builtin::ObjectSetPrototypeOf) => {
            execute_mutating_object_builtin(arguments, args, registers)
        }
        Value::Builtin(builtin) => {
            execute_builtin_with_receiver(*builtin, arguments, Some(receiver))
        }
        Value::BoundFunction(bound)
            if matches!(
                bound.target,
                Value::Builtin(crate::ops::Builtin::HostCapability(_))
            ) =>
        {
            execute_bound_host_capability(bound, receiver, arguments)
        }
        Value::BoundFunction(bound) if matches!(bound.target, Value::HostCapability(_)) => {
            let Value::HostCapability(capability) = &bound.target else {
                unreachable!()
            };
            let capability_receiver = match &bound.receiver {
                Value::BindingCell(cell) => cell.borrow().clone(),
                value => value.clone(),
            };
            let mut combined = bound.arguments.clone();
            combined.extend_from_slice(arguments);
            crate::vm::execute_host_capability_with_receiver(
                capability.descriptor.kind,
                Some(&bound.target),
                Some(&capability_receiver),
                &combined,
            )
        }
        Value::Function(_) | Value::BoundFunction(_) => {
            crate::functions::execute_target_with_receiver(&callee, receiver, arguments)
                .map(|(value, _)| value)
        }
        Value::Proxy(_) => crate::functions::execute_target(&callee, receiver, arguments),
        other => {
            let discriminant = match &other {
                Value::Number(_) => 0, Value::Boolean(_) => 1, Value::String(_) => 2,
                Value::StringUnits(_) => 3, Value::BigInt(_) => 4, Value::Array(_) => 5,
                Value::Object(_) => 6, Value::ObjectAlias(_) => 7, Value::BindingCell(_) => 8,
                Value::ArrayBuffer(_) => 9, Value::Float64Array(_) => 10, Value::Float32Array(_) => 11,
                Value::Int8Array(_) => 12, Value::Int16Array(_) => 13, Value::Int32Array(_) => 14,
                Value::BigInt64Array(_) => 15, Value::BigUint64Array(_) => 16, Value::Uint32Array(_) => 17,
                Value::Uint8Array(_) => 18, Value::Uint8ClampedArray(_) => 19, Value::Uint16Array(_) => 20,
                Value::DataView(_) => 21, Value::Builtin(_) => 22, Value::Function(_) => 23,
                Value::BoundFunction(_) => 24, Value::Proxy(_) => 25, Value::Promise(_) => 26,
                Value::HostCapability(_) => 27,
                Value::Map(_) => 28, Value::Set(_) => 29, Value::Iterator(_) => 30,
                Value::Generator(_) => 31, Value::Null => 32, Value::Undefined => 33,
            };
            return Err(crate::value::error::throw_type_error(&format!(
                "value is not callable [method callee discriminant={discriminant}]"
            )));
        }
    }
}

fn execute_bound_host_capability(
    bound: &crate::value::BoundFunctionValue,
    receiver: &Value,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let Value::Builtin(crate::ops::Builtin::HostCapability(kind)) = bound.target else {
        unreachable!()
    };
    let capability_receiver = match &bound.receiver {
        Value::HostCapability(_) => bound.receiver.clone(),
        _ => crate::vm::realm_token(bound.realm).ok_or(crate::vm::VmError::NotCallable)?,
    };
    crate::vm::execute_host_capability_with_receiver(
        kind,
        Some(&capability_receiver),
        Some(receiver),
        arguments,
    )
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
