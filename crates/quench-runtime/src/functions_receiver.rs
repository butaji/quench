fn execute_generator_target(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    target: &crate::value::Value,
    receiver: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Result<Option<crate::value::Value>, crate::execute::VmError> {
    if !(matches!(function.kind, crate::ops::FunctionKind::Generator) || function.is_async) {
        return Ok(None);
    }
    if matches!(function.kind, crate::ops::FunctionKind::Generator) {
        if let crate::value::Value::Generator(generator) = receiver {
            let next_arg = arguments.first().cloned().unwrap_or(crate::value::Value::Undefined);
            let result = crate::generator::resume(
                generator,
                crate::generator::Resume::Next(next_arg),
            )?;
            return Ok(Some(result));
        }
    }
    Ok(Some(execute_target(target, receiver, arguments)?))
}

pub(crate) fn execute_target_with_receiver(
    target: &crate::value::Value,
    receiver: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Result<
    (crate::value::Value, crate::value::Value),
    crate::execute::VmError,
> {
    let crate::value::Value::Function(function) = target else {
        eprintln!("TRACE receiver_not_function target_variant={} receiver_variant={}", value_variant(target), value_variant(receiver));
        let result = execute_target(target, receiver, arguments)?;
        return Ok((result, receiver.clone()));
    };
    let frame = CallFrame::new(std::rc::Rc::clone(function), receiver.clone(), arguments.to_vec());
    if let Some(result) = execute_generator_target(function, target, receiver, arguments)? {
        return Ok((result, receiver.clone()));
    }
    let _private_environment = crate::private_environment::Guard::install_environment(
        frame.function.private_environment.clone(),
    );
    let _home = crate::super_scope::Guard::install(&frame.function, &frame.receiver);
    let _with_scope = crate::with_scope::FunctionGuard::install(&frame.function.with_captures);
    let (mut registers, environment) =
        build_registers(&frame.function, &frame.receiver, &frame.arguments);
    let context = crate::vm::current_context_or_default();
    let completion = crate::vm::execute_frame_completion(
        frame.function.ops(),
        &mut registers,
        &context,
        std::rc::Rc::clone(&environment),
    )?;
    let result = crate::vm::completion_result(completion)?;
    let slot = frame.function.captures.len() as u16 + frame.function.params + 1;
    Ok((result, environment.get(slot)))
}
fn value_variant(value: &crate::value::Value) -> u32 {
    match value {
        crate::value::Value::Undefined => 0,
        crate::value::Value::Null => 1,
        crate::value::Value::Boolean(_) => 2,
        crate::value::Value::Number(_) => 3,
        crate::value::Value::String(_) => 4,
        crate::value::Value::HostCapability(_) => 5,
        crate::value::Value::Builtin(_) => 6,
        crate::value::Value::Function(_) => 7,
        crate::value::Value::BoundFunction(_) => 8,
        crate::value::Value::Proxy(_) => 9,
        _ => 10,
    }
}
