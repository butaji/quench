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
    let frame = CallFrame::new(
        std::rc::Rc::clone(function),
        this_value.clone(),
        arguments.to_vec(),
    );
    if matches!(function.kind, FunctionKind::Generator) {
        return crate::generator::create(function, &frame.receiver, arguments);
    }
    if function.is_async {
        return Ok(crate::promise::from_async_completion(execute_frame_value(
            &frame,
        )));
    }
    execute_frames(frame)
}

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
        let receiver = crate::vm::bare_call_receiver(&function, &receiver);
        Self {
            function,
            receiver,
            arguments,
        }
    }
}

enum TailTarget {
    Frame(CallFrame),
    Value(crate::value::Value),
}

fn execute_frames(mut frame: CallFrame) -> Result<crate::value::Value, crate::execute::VmError> {
    loop {
        match execute_frame_completion(&frame)? {
            crate::completion::Completion::TailCall(request) => {
                match resolve_tail_target(request)? {
                    TailTarget::Frame(next) => frame = next,
                    TailTarget::Value(value) => return Ok(value),
                }
            }
            completion => return crate::vm::completion_result(completion),
        }
    }
}

fn execute_frame_value(frame: &CallFrame) -> Result<crate::value::Value, crate::execute::VmError> {
    crate::vm::completion_result(execute_frame_completion(frame)?)
}

fn execute_frame_completion(
    frame: &CallFrame,
) -> Result<crate::completion::Completion, crate::execute::VmError> {
    let _private_environment = crate::private_environment::Guard::install_environment(
        frame.function.private_environment.clone(),
    );
    let _home = crate::super_scope::Guard::install(&frame.function, &frame.receiver);
    let _with_scope = crate::with_scope::FunctionGuard::install(&frame.function.with_captures);
    let (mut registers, environment) =
        build_registers(&frame.function, &frame.receiver, &frame.arguments);
    let context = crate::vm::current_context_or_default();
    crate::vm::execute_frame_completion(frame.function.ops(), &mut registers, &context, environment)
}

fn resolve_tail_target(
    request: crate::completion::TailCallRequest,
) -> Result<TailTarget, crate::execute::VmError> {
    let (target, receiver, arguments) =
        flatten_bound_target(request.callee, request.receiver, request.arguments);
    match target {
        crate::value::Value::Function(function) => {
            resolve_function_target(function, receiver, arguments)
        }
        crate::value::Value::Builtin(builtin) => {
            execute_builtin_target(builtin, Some(&receiver), &arguments).map(TailTarget::Value)
        }
        crate::value::Value::Proxy(_) => {
            crate::proxy::proxy_apply(&target, &receiver, &arguments).map(TailTarget::Value)
        }
        _ => Err(crate::execute::VmError::NotCallable),
    }
}

fn flatten_bound_target(
    mut target: crate::value::Value,
    mut receiver: crate::value::Value,
    mut arguments: Vec<crate::value::Value>,
) -> (
    crate::value::Value,
    crate::value::Value,
    Vec<crate::value::Value>,
) {
    while let crate::value::Value::BoundFunction(bound) = target {
        let mut combined = bound.arguments.clone();
        combined.append(&mut arguments);
        arguments = combined;
        receiver = bound.receiver.clone();
        target = bound.target.clone();
    }
    (target, receiver, arguments)
}

fn resolve_function_target(
    function: std::rc::Rc<crate::value::FunctionValue>,
    receiver: crate::value::Value,
    arguments: Vec<crate::value::Value>,
) -> Result<TailTarget, crate::execute::VmError> {
    if function.is_async || matches!(function.kind, FunctionKind::Generator) {
        return execute(&function, &receiver, &arguments).map(TailTarget::Value);
    }
    Ok(TailTarget::Frame(CallFrame::new(
        function, receiver, arguments,
    )))
}
