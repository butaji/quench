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
