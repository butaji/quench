fn emit_function_expression(
    ops: &mut Vec<Op>,
    next: &mut u16,
    body: Vec<Op>,
    params: u16,
    captures: u16,
    metadata: FunctionMetadata,
    declared_name: Option<&str>,
) -> u16 {
    let function = emit_function_op(ops, next, body, params, captures, metadata);
    if let Some(name) = declared_name {
        ops.push(Op::SetFunctionName {
            function,
            name: name.to_string(),
        });
        let marker = *next;
        *next = next.saturating_add(1);
        ops.push(Op::Const {
            dst: marker,
            value: crate::ops::Constant::Boolean(true),
        });
        ops.push(Op::SetProperty {
            object: function,
            key: FUNCTION_SELF.to_string(),
            src: marker,
            strict: true,
        });
    }
    function
}

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
        if function
            .properties
            .borrow()
            .iter()
            .any(|(name, _)| name == FUNCTION_SELF)
        {
            parameters.push(crate::value::Value::Function(std::rc::Rc::clone(function)));
        }
    }
    let environment = crate::environment::Environment::child(&function.captures, parameters);
    let arguments = arguments_object(function, original_arguments, &environment);
    let arguments_slot = function.captures.len() as u16 + function.params;
    environment.set(arguments_slot, arguments);
    let register_count = function.ops().len().max(32);
    (vec![crate::value::Value::Undefined; register_count], environment)
}

/// Execute a constructor and return its result plus the final `this` value.
pub(crate) fn execute_construct(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    this_value: &crate::value::Value,
    new_target: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Result<(crate::value::Value, crate::value::Value), crate::execute::VmError> {
    let captures = function.captures.len() as u16;
    let (mut registers, environment) = build_registers(function, this_value, arguments);
    let this_slot = captures.saturating_add(function.params).saturating_add(1);
    let new_target_slot = this_slot.saturating_add(1);
    environment.set(new_target_slot, new_target.clone());
    if is_derived_constructor(function) {
        environment.mark_uninitialized(this_slot);
    }
    let result = crate::vm::execute_in_environment(
        function.ops(),
        &mut registers,
        &crate::vm::VmContext::default(),
        std::rc::Rc::clone(&environment),
    )?;
    let final_this = environment.get(this_slot);
    Ok((result, final_this))
}

fn is_derived_constructor(function: &crate::value::FunctionValue) -> bool {
    function
        .properties
        .borrow()
        .iter()
        .any(|(name, _)| name == "\0derived_constructor")
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
        environment.map_argument(
            arguments,
            usize::from(index),
            captures.saturating_add(index),
        );
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

pub(crate) fn execute_bound(
    bound: &crate::value::BoundFunctionValue,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    let mut combined = bound.arguments.clone();
    combined.extend_from_slice(arguments);
    match &bound.target {
        crate::value::Value::Builtin(builtin) => {
            execute_builtin_target(*builtin, Some(&bound.receiver), &combined)
        }
        crate::value::Value::Function(function) => execute(function, &bound.receiver, &combined),
        crate::value::Value::BoundFunction(next) => execute_bound(next, &combined),
        crate::value::Value::Proxy(_) => {
            crate::proxy::proxy_apply(&bound.target, &bound.receiver, &combined)
        }
        _ => Err(crate::execute::VmError::NotCallable),
    }
}

pub(crate) fn execute_target(
    target: &crate::value::Value,
    receiver: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    match target {
        crate::value::Value::Builtin(builtin) => {
            execute_builtin_target(*builtin, Some(receiver), arguments)
        }
        crate::value::Value::Function(function) => execute(function, receiver, arguments),
        crate::value::Value::BoundFunction(bound) => execute_bound(bound, arguments),
        crate::value::Value::Proxy(_) => crate::proxy::proxy_apply(target, receiver, arguments),
        _ => Err(crate::execute::VmError::NotCallable),
    }
}

fn execute_builtin_target(
    builtin: crate::ops::Builtin,
    receiver: Option<&crate::value::Value>,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    if let crate::ops::Builtin::HostCapability(kind) = builtin {
        return crate::vm::execute_host_capability(kind, receiver, arguments);
    }
    crate::execute::execute_builtin_with_receiver(builtin, arguments, receiver)
}

fn execute_function_call(
    receiver: Option<&crate::value::Value>,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    let receiver = receiver.ok_or(crate::execute::VmError::NotCallable)?;
    let this = arguments
        .first()
        .cloned()
        .unwrap_or(crate::value::Value::Undefined);
    execute_target(receiver, &this, arguments.get(1..).unwrap_or_default())
}

fn bind_function_target(
    receiver: Option<&crate::value::Value>,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    if !receiver.is_some_and(crate::conversion::is_callable) {
        return Err(crate::execute::VmError::NotCallable);
    }
    let target = arguments
        .first()
        .cloned()
        .unwrap_or(crate::value::Value::Undefined);
    let extra = arguments.get(1..).unwrap_or(&[]).to_vec();
    Ok(crate::value::Value::BoundFunction(std::rc::Rc::new(
        crate::value::BoundFunctionValue {
            target: receiver.cloned().unwrap_or(crate::value::Value::Undefined),
            receiver: target,
            arguments: extra,
        },
    )))
}

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
        crate::ops::Builtin::ArrayUnshift => Ok(crate::builtins::array_unshift(receiver, arguments)),
        crate::ops::Builtin::ArrayFill => Ok(crate::builtins::array_fill(receiver, arguments)),
        crate::ops::Builtin::ArrayCopyWithin => {
            Ok(crate::builtins::array_copy_within(receiver, arguments))
        }
        crate::ops::Builtin::ArrayFindLast => {
            crate::builtins::array_find_last(receiver, arguments)
        }
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
