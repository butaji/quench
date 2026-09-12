pub(crate) fn execute_target_with_receiver(
    target: &crate::value::Value,
    receiver: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Result<(crate::value::Value, crate::value::Value), crate::execute::VmError> {
    let crate::value::Value::Function(function) = target else {
        let result = execute_target(target, receiver, arguments)?;
        return Ok((result, receiver.clone()));
    };
    let realm = crate::construct::function_realm_id(function);
    if realm != crate::ops::RealmId::ROOT {
        return crate::vm::with_realm(realm, || {
            execute_target_with_receiver_in_realm(target, receiver, arguments)
        })
        .unwrap_or_else(|| execute_target_with_receiver_in_realm(target, receiver, arguments));
    }
    execute_target_with_receiver_in_realm(target, receiver, arguments)
}

fn execute_target_with_receiver_in_realm(
    target: &crate::value::Value,
    receiver: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Result<(crate::value::Value, crate::value::Value), crate::execute::VmError> {
    let crate::value::Value::Function(function) = target else {
        let result = execute_target(target, receiver, arguments)?;
        return Ok((result, receiver.clone()));
    };
    let source_name = match crate::execute::get_property(target, "\0quench:source_name") {
        crate::value::Value::String(name) => Some(name),
        _ => None,
    };
    let _source_guard = source_name
        .as_deref()
        .map(|name| crate::vm::active_source_name(Some(name)));
    if matches!(function.kind, FunctionKind::Generator) || function.is_async {
        // When the protocol machinery invokes a generator function with an
        // existing generator as receiver, the call is the generator's resume
        // operation rather than creation of a second frame.  Ordinary bare
        // generator calls still use `execute_target` below.
        if matches!(function.kind, FunctionKind::Generator) {
            if let crate::value::Value::Generator(generator) = receiver {
                let next_arg = arguments
                    .first()
                    .cloned()
                    .unwrap_or(crate::value::Value::Undefined);
                let result =
                    crate::generator::resume(generator, crate::generator::Resume::Next(next_arg))?;
                return Ok((result, receiver.clone()));
            }
        }
        let result = execute_target(target, receiver, arguments)?;
        return Ok((result, receiver.clone()));
    }
    let receiver = crate::vm::bare_call_receiver(function, receiver);
    let _private_environment = crate::private_environment::Guard::install_environment(
        function.private_environment.clone(),
    );
    let _home = crate::super_scope::Guard::install(function, &receiver);
    let _with_scope = crate::with_scope::FunctionGuard::install(&function.with_captures);
    let (mut registers, environment) = build_registers(function, &receiver, arguments);
    let context = crate::vm::current_context_or_default();
    let completion = match crate::vm::execute_code_frame_completion(
        function
            .code
            .code()
            .ok_or(crate::execute::VmError::MissingReturn)?,
        &mut registers,
        &context,
        std::rc::Rc::clone(&environment),
    ) {
        Ok(completion) => completion,
        Err(crate::execute::VmError::Thrown(error)) => {
            let function_value = crate::value::Value::Function(std::rc::Rc::clone(function));
            if matches!(
                crate::execute::get_property(&function_value, "\0quench:hidden_stack_frames"),
                crate::value::Value::Boolean(true)
            ) {
                return Err(crate::execute::VmError::Thrown(error));
            }
            let name = match crate::execute::get_property(&function_value, "name") {
                crate::value::Value::String(name) if !name.is_empty() => name,
                _ => "<anonymous>".to_string(),
            };
            crate::vm::append_stack_frame(&error, &name);
            return Err(crate::execute::VmError::Thrown(error));
        }
        Err(error) => return Err(error),
    };
    let result = match completion {
        crate::completion::Completion::TailCall(request) => crate::functions::execute_target(
            &request.callee,
            &request.receiver,
            &request.arguments,
        )?,
        completion => crate::vm::completion_result(completion)?,
    };
    let slot = function.captures.len() as u16 + function.params + 1;
    Ok((result, environment.get(slot)))
}
