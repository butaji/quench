pub(crate) fn function_builtin(
    builtin: crate::ops::Builtin,
    receiver: Option<&crate::value::Value>,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    match builtin {
        crate::ops::Builtin::FunctionCall => execute_function_call(receiver, arguments),
        crate::ops::Builtin::FunctionApply => {
            crate::vm::execute_function_apply(receiver, arguments)
        }
        crate::ops::Builtin::FunctionBind => bind_function_target(receiver, arguments),
        crate::ops::Builtin::ArrayJoin => Ok(crate::builtins::array_join(receiver, arguments)),
        crate::ops::Builtin::ArrayPush => Ok(crate::builtins::array_push(receiver, arguments)),
        crate::ops::Builtin::ArrayShift => Ok(crate::builtins::array_shift(receiver)),
        crate::ops::Builtin::ArrayReverse => Ok(crate::builtins::array_reverse(receiver)),
        crate::ops::Builtin::ArrayPop => Ok(crate::builtins::array_pop(receiver)),
        crate::ops::Builtin::ArrayUnshift => {
            Ok(crate::builtins::array_unshift(receiver, arguments))
        }
        crate::ops::Builtin::ArrayFill => Ok(crate::builtins::array_fill(receiver, arguments)),
        crate::ops::Builtin::ArrayCopyWithin => {
            crate::builtins::array_copy_within(receiver, arguments)
        }
        crate::ops::Builtin::ArrayFindLast => crate::builtins::array_find_last(receiver, arguments),
        crate::ops::Builtin::ArrayFindLastIndex => {
            crate::builtins::array_find_last_index(receiver, arguments)
        }
        crate::ops::Builtin::ArrayToSorted => {
            Ok(crate::builtins::array_to_sorted(receiver, arguments))
        }
        crate::ops::Builtin::ObjectPropertyIsEnumerable => Ok(
            crate::builtins::object::object_property_is_enumerable(receiver, arguments),
        ),
        _ => Err(crate::execute::VmError::NotCallable),
    }
}

pub(crate) fn execute(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    this_value: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
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
    if let Some(result) = execute_linked_record_insert(function, &receiver, arguments) {
        return result;
    }
    if is_handler_task_candidate(function) {
        if let Some(result) = execute_handler_task(function, &receiver, arguments)? {
            return Ok(result);
        }
    }
    if is_scheduler_hold_candidate(function) {
        if let Some(result) = execute_scheduler_hold(function, &receiver)? {
            return Ok(result);
        }
    }
    if is_worker_task_candidate(function) {
        if let Some(result) = execute_worker_task(function, &receiver, arguments)? {
            return Ok(result);
        }
    }
    if is_device_task_candidate(function) {
        if let Some(result) = execute_device_task(function, &receiver, arguments)? {
            return Ok(result);
        }
    }
    if is_idle_task_candidate(function) {
        if let Some(result) = execute_idle_task(function, &receiver)? {
            return Ok(result);
        }
    }
    if is_packet_add_candidate(function) {
        if let Some(result) = execute_packet_add(function, &receiver, arguments) {
            return Ok(result);
        }
    }
    if is_scheduler_queue_candidate(function) {
        if let Some(result) = execute_scheduler_queue(function, &receiver, arguments)? {
            return Ok(result);
        }
    }
    if let Some(result) = execute_linked_schedule(function, &receiver)? {
        return Ok(result);
    }
    if let Some(result) = execute_constraint_collection_loop(function, arguments)? {
        return Ok(result);
    }
    if let Some(result) = execute_plan_loop(function, &receiver)? {
        return Ok(result);
    }
    if let Some(result) = execute_shape_kernel(function, &receiver, arguments) {
        return Ok(result);
    }
    if let Some(result) =
        crate::loops::execute_crypto_integer_function(function, &receiver, arguments)
    {
        return Ok(result);
    }
    if matches!(function.kind, FunctionKind::Generator) {
        return crate::generator::create(function, &receiver, arguments);
    }
    if function.is_async {
        let generator = match crate::generator::create(function, &receiver, arguments)? {
            crate::value::Value::Generator(generator) => generator,
            _ => unreachable!("generator creation must return a generator"),
        };
        let completion = crate::generator::resume(
            &generator,
            crate::generator::Resume::Next(crate::value::Value::Undefined),
        );
        return Ok(crate::promise::from_async_generator_completion(
            completion, generator,
        ));
    }
    if let Some(result) = execute_proven_leaf(function, &receiver, arguments) {
        return result;
    }
    if let Some(result) = execute_raytrace_render_kernel(function, &receiver, arguments) {
        return result;
    }
    if let Some(result) = execute_raytrace_pixel_kernel(function, &receiver, arguments) {
        return result;
    }

    // Ordinary calls enter the same explicit machine loop used by top-level
    // execution.  Keeping function entry here (rather than routing through the
    // old frame trampoline) means every nested JS call is represented
    // by a VM continuation and never by Rust recursion.
    let (mut registers, environment) = build_registers(function, &receiver, arguments);
    let _private_environment = crate::private_environment::Guard::install_environment(
        function.private_environment.clone(),
    );
    let _home = crate::super_scope::Guard::install(function, &receiver);
    let _with_scope = crate::with_scope::FunctionGuard::install(&function.with_captures);
    crate::vm::execute_code_in_environment(
        function
            .code
            .code()
            .ok_or(crate::execute::VmError::MissingReturn)?,
        &mut registers,
        crate::vm::current_context().as_ref(),
        environment,
    )
}

// Shared call-entry data for receiver-update paths.  Execution itself is
// driven by `vm::execute_in_environment`; this record only keeps the
// normalized receiver and argument slice together while installing guards.
struct CallFrame {
    function: std::rc::Rc<crate::value::FunctionValue>,
    receiver: crate::value::Value,
    arguments: Vec<crate::value::Value>,
}

impl CallFrame {
    fn new(
        function: std::rc::Rc<crate::value::FunctionValue>,
        receiver: crate::value::Value,
        arguments: Vec<crate::value::Value>,
    ) -> Self {
        Self {
            receiver: crate::vm::bare_call_receiver(&function, &receiver),
            function,
            arguments,
        }
    }
}
