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
) -> Result<
    (crate::value::Value, crate::value::Value),
    crate::execute::VmError,
> {
    let crate::value::Value::Function(function) = target else {
        let result = execute_target(target, receiver, arguments)?;
        return Ok((result, receiver.clone()));
    };
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
    let receiver = crate::vm::bare_call_receiver(function, receiver);
    let _private_environment = crate::private_environment::Guard::install_environment(
        function.private_environment.clone(),
    );
    let _home = crate::super_scope::Guard::install(function, &receiver);
    let _with_scope = crate::with_scope::FunctionGuard::install(&function.with_captures);
    #[cfg(feature = "execution-trace")]
    crate::execution_trace::function_call_shape(
        function.params,
        function.code.capture_slots().len(),
        function.code.code(),
    );
    let (mut registers, environment) =
        build_registers(function, &receiver, arguments);
    let context = crate::vm::current_context_or_default();
    let completion = match crate::vm::execute_code_frame_completion_with_owner(
        function.code.code().ok_or(crate::execute::VmError::MissingReturn)?,
        &function.code,
        &mut registers,
        &context,
        std::rc::Rc::clone(&environment),
    ) {
        Ok(completion) => completion,
        Err(error) => {
            crate::environment::Environment::recycle_frame(environment);
            return Err(error);
        }
    };
    let recyclable = !completion.is_suspension();
    let result = match completion {
        crate::completion::Completion::TailCall(request) => crate::functions::execute_target(
            &request.callee,
            &request.receiver,
            &request.arguments,
        ),
        completion => crate::vm::completion_result(completion),
    };
    let slot = function.captures.len() as u16 + function.params + 1;
    let updated_receiver = environment.get(slot);
    if recyclable {
        crate::environment::Environment::recycle_frame(environment);
    }
    Ok((result?, updated_receiver))
}
