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
        crate::ops::Builtin::TypedArrayCopyWithin => {
            crate::builtins::typed_array_copy_within(receiver, arguments)
        }
        crate::ops::Builtin::ArrayFindLast => crate::builtins::array_find_last(receiver, arguments),
        crate::ops::Builtin::ArrayFindLastIndex => {
            crate::builtins::array_find_last_index(receiver, arguments)
        }
        crate::ops::Builtin::ArrayFindIndex => crate::arrays::find_index(receiver, arguments),
        crate::ops::Builtin::ArrayToSorted => {
            crate::builtins::array_to_sorted(receiver, arguments)
        }
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
    // A proven side-effect-free numeric loop can complete before the general
    // specialization ladder. This keeps the per-call path bounded for hot
    // helpers while retaining the interpreter fallback for non-number input.
    if matches!(
        function.code.facts().counted_method_loop.as_deref(),
        Some(crate::facts::CountedMethodLoopFact::BitCount)
    ) {
        if let Some(crate::value::Value::Number(mut value)) = arguments.first().cloned() {
            let mut count = 0_u32;
            while value > 0.0 {
                let bits = crate::vm::vm_arithmetic::numeric_to_int32(value);
                value = f64::from(bits & bits.wrapping_sub(1));
                count += 1;
            }
            return Ok(Some(crate::value::Value::Number(f64::from(count))));
        }
    }
    if is_class_constructor(function) {
        return Err(crate::value::error::throw_type_error(
            "Class constructor cannot be invoked without 'new'",
        ));
    }
    let receiver = crate::vm::bare_call_receiver(function, this_value);
    if let Some(result) = execute_forward_construct_call(function, &receiver, arguments) {
        return result.map(Some);
    }
    if let Some(result) = execute_forward_then_call(function, &receiver, arguments) {
        return result.map(Some);
    }
    if let Some(result) = execute_slot_alu(function, &receiver, arguments) {
        return Ok(Some(result));
    }
    if let Some(result) = execute_select_update_call(function, &receiver)? {
        return Ok(Some(result));
    }
    if let Some(result) = execute_constraint_collection_loop(function, arguments)? {
        return Ok(Some(result));
    }
    if let Some(result) = execute_plan_loop(function, &receiver)? {
        return Ok(Some(result));
    }
    if let Some(result) = execute_counted_method_loop(function, &receiver, arguments)? {
        return Ok(Some(result));
    }
    if let Some(result) = execute_shape_kernel(function, &receiver, arguments) {
        return Ok(Some(result));
    }
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
        return Ok(Some(crate::promise::start_async_function(generator)));
    }
    // Compact numeric leaves stay on the ordinary proven-leaf path.
    if function.code.code().is_some_and(|code| {
        (0..code.len()).all(|pc| {
            code.instruction(pc)
                .is_some_and(|op| op.opcode != crate::ir::Opcode::Slow)
        })
    }) {
        if let Some(result) = execute_proven_leaf(function, &receiver, arguments) {
            return result.map(Some);
        }
    }
    Ok(None)
}

#[inline(never)]
pub(crate) fn execute(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    this_value: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    if let Some(result) = try_execute_specialized(function, this_value, arguments)? {
        return Ok(result);
    }
    stacker::maybe_grow(64 * 1024 * 1024, 256 * 1024 * 1024, || {
        execute_interpreter(function, this_value, arguments)
    })
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
        let (mut registers, environment) =
            build_registers(&function, &receiver, arguments.as_ref());
        let _private_environment = crate::private_environment::Guard::install_environment(
            function.private_environment.clone(),
        );
        let _home = crate::super_scope::Guard::install(&function, &receiver);
        let _with_scope = crate::with_scope::FunctionGuard::install(&function.with_captures);
        let completion = crate::vm::execute_code_frame_completion(
            function
                .code
                .code()
                .ok_or(crate::execute::VmError::MissingReturn)?,
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
        let (registers, environment) =
            crate::functions::build_registers(&function, &receiver, arguments.as_ref());
        let _private_environment = crate::private_environment::Guard::install_environment(
            function.private_environment.clone(),
        );
        let _home = crate::super_scope::Guard::install(&function, &receiver);
        let _with_scope = crate::with_scope::FunctionGuard::install(&function.with_captures);
        let mut registers = registers;
        let completion = crate::vm::execute_code_frame_completion(
            function
                .code
                .code()
                .ok_or(crate::execute::VmError::MissingReturn)?,
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
