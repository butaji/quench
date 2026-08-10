pub(crate) fn build_registers(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    this_value: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> (
    Vec<crate::value::Value>,
    std::rc::Rc<crate::environment::Environment>,
) {
    let original_arguments = arguments.to_vec();
    let mut parameters = arguments.to_vec();
    parameters.resize(usize::from(function.params), crate::value::Value::Undefined);
    parameters.truncate(usize::from(function.params));
    parameters.push(crate::value::Value::Undefined);
    parameters.push(this_value.clone());
    if !matches!(function.kind, FunctionKind::Arrow) {
        parameters.push(crate::value::Value::Undefined);
    }
    let environment = crate::environment::Environment::child(&function.captures, parameters);
    let arguments = arguments_object(function, original_arguments, &environment);
    let arguments_slot = function.captures.len() as u16 + function.params;
    environment.set(arguments_slot, arguments);
    (vec![crate::value::Value::Undefined; 32], environment)
}

/// Execute a constructor and return its result plus the final `this` value.
pub(crate) fn execute_construct(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    this_value: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Result<(crate::value::Value, crate::value::Value), crate::execute::VmError> {
    let captures = function.captures.len() as u16;
    let (mut registers, environment) = build_registers(function, this_value, arguments);
    environment.set(
        captures.saturating_add(function.params).saturating_add(2),
        crate::value::Value::Function(std::rc::Rc::clone(function)),
    );
    let result = crate::vm::execute_in_environment(
        &function.body,
        &mut registers,
        &crate::vm::VmContext::default(),
        std::rc::Rc::clone(&environment),
    )?;
    let final_this = environment.get(captures.saturating_add(function.params).saturating_add(1));
    Ok((result, final_this))
}

fn arguments_object(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    values: Vec<crate::value::Value>,
    environment: &std::rc::Rc<crate::environment::Environment>,
) -> crate::value::Value {
    let length = values.len() as f64;
    let strict = matches!(function.strictness, FunctionStrictness::Strict);
    let mut arguments = crate::value::ArrayData::new_arguments(values, strict);
    arguments.set_property("length", crate::value::Value::Number(length));
    arguments.set_property(
        "Symbol.iterator",
        crate::value::Value::Builtin(crate::ops::Builtin::ArrayIterator),
    );
    if matches!(function.strictness, FunctionStrictness::Sloppy) {
        if function.mapped_arguments {
            map_arguments(&mut arguments, function, environment);
        }
        arguments.set_property(
            "callee",
            crate::value::Value::Function(std::rc::Rc::clone(function)),
        );
    }
    crate::value::Value::Array(std::rc::Rc::new(arguments))
}

fn map_arguments(
    arguments: &mut crate::value::ArrayData,
    function: &crate::value::FunctionValue,
    environment: &crate::environment::Environment,
) {
    let captures = function.captures.len() as u16;
    let mapped = function.params.min(arguments.logical_len() as u16);
    for index in 0..mapped {
        if let Some(binding) = environment.slot(captures.saturating_add(index)) {
            arguments.map_index(usize::from(index), binding);
        }
    }
}

pub(crate) fn is_constructible(function: &crate::value::FunctionValue) -> bool {
    match (function.kind, function.strictness, function.is_async) {
        (FunctionKind::Ordinary, FunctionStrictness::Sloppy, false)
        | (FunctionKind::Ordinary, FunctionStrictness::Strict, false) => true,
        (FunctionKind::Arrow, _, _)
        | (FunctionKind::Generator, _, _)
        | (FunctionKind::Ordinary, _, true) => false,
    }
}

pub(crate) fn execute(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    this_value: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    let frame = CallFrame::new(std::rc::Rc::clone(function), this_value.clone(), arguments.to_vec());
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
    let _home = crate::super_scope::Guard::install(&frame.function, &frame.receiver);
    let _with_scope = crate::with_scope::FunctionGuard::isolate();
    let (mut registers, environment) =
        build_registers(&frame.function, &frame.receiver, &frame.arguments);
    crate::vm::execute_frame_completion(
        &frame.function.body,
        &mut registers,
        &crate::vm::VmContext::default(),
        environment,
    )
}

fn resolve_tail_target(
    request: crate::completion::TailCallRequest,
) -> Result<TailTarget, crate::execute::VmError> {
    let (target, receiver, arguments) = flatten_bound_target(
        request.callee,
        request.receiver,
        request.arguments,
    );
    match target {
        crate::value::Value::Function(function) => resolve_function_target(function, receiver, arguments),
        crate::value::Value::Builtin(builtin) => {
            execute_builtin_target(builtin, Some(&receiver), &arguments).map(TailTarget::Value)
        }
        crate::value::Value::Proxy(_) => crate::proxy::proxy_apply(&target, &receiver, &arguments)
            .map(TailTarget::Value),
        _ => Err(crate::execute::VmError::NotCallable),
    }
}

fn flatten_bound_target(
    mut target: crate::value::Value,
    mut receiver: crate::value::Value,
    mut arguments: Vec<crate::value::Value>,
) -> (crate::value::Value, crate::value::Value, Vec<crate::value::Value>) {
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
