pub(crate) fn execute_target_with_receiver(
    target: &crate::value::Value,
    receiver: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Result<
    (crate::value::Value, crate::value::Value),
    crate::execute::VmError,
> {
    let crate::value::Value::Function(function) = target else {
        let result = execute_target(target, receiver, arguments)?;
        return Ok((result, receiver.clone()));
    };
    let frame = CallFrame::new(std::rc::Rc::clone(function), receiver.clone(), arguments.to_vec());
    if matches!(function.kind, FunctionKind::Generator) || function.is_async {
        // If the receiver is already an existing generator, resume it instead
        // of calling generator::create (which would start a fresh generator).
        // This prevents infinite-generator creation when call_next invokes a
        // generator-backed protocol iterator under the
        // ReceiverUpdateGuard (e.g., Array.from(generator-iterator)).
        if matches!(function.kind, FunctionKind::Generator) {
            if let crate::value::Value::Generator(generator) = receiver {
                let next_arg = arguments.first().cloned().unwrap_or(crate::value::Value::Undefined);
                let result = crate::generator::resume(generator, crate::generator::Resume::Next(next_arg))?;
                return Ok((result, receiver.clone()));
            }
        }
        let result = execute_target(target, receiver, arguments)?;
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
    let mut registers = crate::register_file::RegisterFile::from_values(registers);
    let completion = crate::vm::execute_code_frame_completion(
        frame.function.code.code().ok_or(crate::execute::VmError::MissingReturn)?,
        &mut registers,
        &context,
        std::rc::Rc::clone(&environment),
    )?;
    let result = crate::vm::completion_result(completion)?;
    let slot = frame.function.captures.len() as u16 + frame.function.params + 1;
    Ok((result, environment.get(slot)))
}
