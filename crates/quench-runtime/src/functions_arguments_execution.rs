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
        _ => Err(crate::execute::VmError::NotCallable),
    }
}

pub(crate) fn execute(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    this_value: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    // Track the active JS function chain so `Error.captureStackTrace` can
    // produce real (function-name, module-file) frames. The guard pops on
    // every exit path, including errors.
    let _frame = crate::frame_stack::FrameGuard::enter(function);
    let frame = CallFrame::new(
        std::rc::Rc::clone(function),
        this_value.clone(),
        arguments.to_vec(),
    );
    if is_class_constructor(function) {
        return Err(crate::value::error::throw_type_error(
            "Class constructor cannot be invoked without 'new'",
        ));
    }
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
    let target_for_receiver = target.clone();
    match target {
        crate::value::Value::Function(function) => {
            resolve_function_target(function, receiver, arguments)
        }
        crate::value::Value::Builtin(crate::ops::Builtin::HostCapability(kind)) => {
            let capability_receiver = match &receiver {
                crate::value::Value::HostCapability(capability) => {
                    Some(crate::value::Value::HostCapability(capability.clone()))
                }
                crate::value::Value::BindingCell(cell) => match &*cell.borrow() {
                    crate::value::Value::HostCapability(capability) => {
                        Some(crate::value::Value::HostCapability(capability.clone()))
                    }
                    _ => None,
                },
                _ => crate::vm::realm_token(crate::vm::current_context_or_default().realm()),
            }
            .ok_or(crate::execute::VmError::NotCallable)?;
            crate::vm::execute_host_capability_with_receiver(
                kind,
                Some(&capability_receiver),
                Some(&receiver),
                &arguments,
            )
            .map(TailTarget::Value)
        }
        crate::value::Value::HostCapability(capability) => {
            crate::vm::execute_host_capability_with_receiver(
                capability.descriptor.kind,
                Some(&target_for_receiver),
                Some(&receiver),
                &arguments,
            )
            .map(TailTarget::Value)
        }
        crate::value::Value::Builtin(builtin) => {
            execute_builtin_target(builtin, Some(&receiver), &arguments).map(TailTarget::Value)
        }
        crate::value::Value::Proxy(_) => {
            crate::proxy::proxy_apply(&target, &receiver, &arguments).map(TailTarget::Value)
        }
        other => {
            let discriminant = match &other {
                crate::value::Value::Number(_) => 0,
                crate::value::Value::Boolean(_) => 1,
                crate::value::Value::String(_) => 2,
                crate::value::Value::StringUnits(_) => 3,
                crate::value::Value::BigInt(_) => 4,
                crate::value::Value::Array(_) => 5,
                crate::value::Value::Object(_) => 6,
                crate::value::Value::ObjectAlias(_) => 7,
                crate::value::Value::BindingCell(_) => 8,
                crate::value::Value::ArrayBuffer(_) => 9,
                crate::value::Value::Float64Array(_) => 10,
                crate::value::Value::Float32Array(_) => 11,
                crate::value::Value::Int8Array(_) => 12,
                crate::value::Value::Int16Array(_) => 13,
                crate::value::Value::Int32Array(_) => 14,
                crate::value::Value::BigInt64Array(_) => 15,
                crate::value::Value::BigUint64Array(_) => 16,
                crate::value::Value::Uint32Array(_) => 17,
                crate::value::Value::Uint8Array(_) => 18,
                crate::value::Value::Uint8ClampedArray(_) => 19,
                crate::value::Value::Uint16Array(_) => 20,
                crate::value::Value::DataView(_) => 21,
                crate::value::Value::Builtin(_) => 22,
                crate::value::Value::Function(_) => 23,
                crate::value::Value::BoundFunction(_) => 24,
                crate::value::Value::Proxy(_) => 25,
                crate::value::Value::Promise(_) => 26,
                crate::value::Value::HostCapability(_) => 27,
                crate::value::Value::Map(_) => 28,
                crate::value::Value::Set(_) => 29,
                crate::value::Value::Iterator(_) => 30,
                crate::value::Value::Generator(_) => 31,
                crate::value::Value::Null => 32,
                crate::value::Value::Undefined => 33,
            };
            Err(crate::execute::VmError::NotCallable)
        }
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
