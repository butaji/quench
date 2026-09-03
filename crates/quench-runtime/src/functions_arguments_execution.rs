pub(crate) fn function_builtin(
    builtin: crate::ops::Builtin,
    receiver: Option<&crate::value::Value>,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    match builtin {
        crate::ops::Builtin::Function
        | crate::ops::Builtin::AsyncFunction
        | crate::ops::Builtin::GeneratorFunction
        | crate::ops::Builtin::AsyncGeneratorFunction => {
            crate::functions_dynamic::construct_builtin(builtin, arguments)
                .unwrap_or_else(|| Err(crate::execute::VmError::NotCallable))
        }
        crate::ops::Builtin::FunctionCall => execute_function_call(receiver, arguments),
        crate::ops::Builtin::FunctionApply => {
            crate::vm::execute_function_apply(receiver, arguments)
        }
        crate::ops::Builtin::FunctionBind => bind_function_target(receiver, arguments),
        crate::ops::Builtin::ArrayJoin => crate::builtins::array_join(receiver, arguments),
        crate::ops::Builtin::TypedArrayJoin => crate::arrays::typed_array_join(receiver, arguments),
        crate::ops::Builtin::ArrayToString => crate::builtins::array_to_string(receiver),
        crate::ops::Builtin::ArrayPush => crate::builtins::array_push(receiver, arguments),
        crate::ops::Builtin::ArrayShift => crate::builtins::array_shift(receiver),
        crate::ops::Builtin::ArrayReverse => crate::builtins::array_reverse(receiver),
        crate::ops::Builtin::ArrayPop => crate::builtins::array_pop(receiver),
        crate::ops::Builtin::ArrayUnshift => crate::builtins::array_unshift(receiver, arguments),
        crate::ops::Builtin::ArrayFill => crate::builtins::array_fill(receiver, arguments),
        crate::ops::Builtin::ArrayCopyWithin => {
            crate::builtins::array_copy_within(receiver, arguments)
        }
        crate::ops::Builtin::ArrayFindLast => crate::builtins::array_find_last(receiver, arguments),
        crate::ops::Builtin::ArrayFindLastIndex => {
            crate::builtins::array_find_last_index(receiver, arguments)
        }
        crate::ops::Builtin::ArrayFindIndex => crate::arrays::find_index(receiver, arguments),
        crate::ops::Builtin::ArrayToSorted => crate::builtins::array_to_sorted(receiver, arguments),
        crate::ops::Builtin::ArrayToSpliced => {
            crate::builtins::array_to_spliced(receiver, arguments)
        }
        crate::ops::Builtin::ArrayWith => crate::builtins::array_with(receiver, arguments),
        crate::ops::Builtin::ObjectPropertyIsEnumerable => {
            crate::builtins::object::object_property_is_enumerable(receiver, arguments)
        }
        _ => Err(crate::execute::VmError::NotCallable),
    }
}

pub(crate) fn try_execute_specialized(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    this_value: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Result<Option<crate::value::Value>, crate::execute::VmError> {
    crate::execution_trace::function_call_shape(
        function.params,
        function.code.capture_slots().len(),
        function.code.code(),
    );
    if is_class_constructor(function) {
        return Err(crate::value::error::throw_type_error(
            "Class constructor cannot be invoked without 'new'",
        ));
    }
    let receiver = crate::vm::bare_call_receiver(function, this_value);
    if matches!(function.kind, FunctionKind::Generator) {
        return crate::generator::create(function, &receiver, arguments).map(Some);
    }
    if function.is_async {
        // Parameter evaluation belongs to the async call's completion. A
        // direct-eval SyntaxError in a default parameter rejects the promise
        // instead of escaping as a synchronous host error.
        let generator = match crate::generator::create(function, &receiver, arguments) {
            Ok(generator) => generator,
            Err(error) => return Ok(Some(crate::promise::from_async_completion(Err(error)))),
        };
        let generator = match generator {
            crate::value::Value::Generator(generator) => generator,
            _ => unreachable!("generator creation must return a generator"),
        };
        let completion = crate::generator::resume(
            &generator,
            crate::generator::Resume::Next(crate::value::Value::Undefined),
        );
        return Ok(Some(crate::promise::from_async_function_completion(
            completion, generator,
        )));
    }
    Ok(None)
}

/// Admission fact shared by ordinary and named calls.  The continuation
/// gateway remains authoritative for every function outside this compact
/// synchronous shape.
#[inline]
pub(crate) fn direct_call_eligible(function: &crate::value::FunctionValue) -> bool {
    !function.is_async
        && !matches!(function.kind, crate::ops::FunctionKind::Generator)
        && !crate::functions::is_class_constructor(function)
        && function.with_captures.is_empty()
        && !function.private_environment.has_names()
        && !crate::with_scope::is_active()
}

/// Enter a function whose ordinary synchronous shape has already been
/// established by a call-site guard. This preserves the same bounded stack
/// reserve and interpreter completion driver as the generic gateway while
/// avoiding a second specialized-function admission pass.
#[inline(never)]
pub(crate) fn execute_direct(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    this_value: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    stacker::maybe_grow(64 * 1024 * 1024, 256 * 1024 * 1024, || {
        execute_interpreter(function, this_value, arguments)
    })
}

#[inline(never)]
pub(crate) fn execute(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    this_value: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    let specialized = try_execute_specialized(function, this_value, arguments);
    match specialized {
        Ok(Some(result)) => Ok(result),
        Ok(None) => stacker::maybe_grow(64 * 1024 * 1024, 256 * 1024 * 1024, || {
            execute_interpreter(function, this_value, arguments)
        }),
        Err(error) => Err(error),
    }
}

fn execute_interpreter(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    this_value: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    let receiver = crate::vm::bare_call_receiver(function, this_value);

    // The packed continuation path intentionally omits dynamic object-scope
    // guards.  A function created inside `with` must retain that scope while a
    // promoted tail call replaces its activation, so drive this small class
    // through the guard-aware machine loop instead.
    if !function.with_captures.is_empty() || crate::with_scope::is_active() {
        return execute_with_dynamic_scope(function, receiver, arguments);
    }

    // Ordinary calls use the same bounded tail-call machine as dynamic-scope
    // calls. Tail-call promotion is a representation detail; it must never
    // escape as an observable VM error merely because the callee has no
    // `with` capture.
    let mut function = std::rc::Rc::clone(function);
    let mut receiver = receiver;
    let mut arguments = std::borrow::Cow::Borrowed(arguments);
    loop {
        let _ = function.code.enter_invocation();
        let (mut registers, environment) =
            build_registers(&function, &receiver, arguments.as_ref());
        let _private_environment = crate::private_environment::Guard::install_environment(
            function.private_environment.clone(),
        );
        let _home = crate::super_scope::Guard::install(&function, &receiver);
        let _with_scope = crate::with_scope::FunctionGuard::install(&function.with_captures);
        let completion = crate::vm::execute_code_frame_completion_with_owner(
            function
                .code
                .code()
                .ok_or(crate::execute::VmError::MissingReturn)?,
            &function.code,
            &mut registers,
            &crate::vm::current_context(),
            environment,
        )?;
        let crate::completion::Completion::TailCall(request) = completion else {
            return crate::vm::completion_result(completion);
        };
        let crate::value::Value::Function(next) = request.callee else {
            return crate::functions::execute_target(
                &request.callee,
                &request.receiver,
                &request.arguments,
            );
        };
        function = next;
        receiver = crate::vm::bare_call_receiver(&function, &request.receiver);
        arguments = std::borrow::Cow::Owned(request.arguments.into_vec());
    }
}

fn execute_with_dynamic_scope(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    receiver: crate::value::Value,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    let mut function = std::rc::Rc::clone(function);
    let mut receiver = receiver;
    let mut arguments = std::borrow::Cow::Borrowed(arguments);
    loop {
        let _ = function.code.enter_invocation();
        let (registers, environment) =
            crate::functions::build_registers(&function, &receiver, arguments.as_ref());
        let _private_environment = crate::private_environment::Guard::install_environment(
            function.private_environment.clone(),
        );
        let _home = crate::super_scope::Guard::install(&function, &receiver);
        let _with_scope = crate::with_scope::FunctionGuard::install(&function.with_captures);
        let mut registers = registers;
        let completion = crate::vm::execute_code_frame_completion_with_owner(
            function
                .code
                .code()
                .ok_or(crate::execute::VmError::MissingReturn)?,
            &function.code,
            &mut registers,
            &crate::vm::current_context(),
            environment,
        )?;
        let crate::completion::Completion::TailCall(request) = completion else {
            return crate::vm::completion_result(completion);
        };
        let crate::value::Value::Function(next) = request.callee else {
            return crate::functions::execute_target(
                &request.callee,
                &request.receiver,
                &request.arguments,
            );
        };
        function = next;
        receiver = crate::vm::bare_call_receiver(&function, &request.receiver);
        arguments = std::borrow::Cow::Owned(request.arguments.into_vec());
    }
}
