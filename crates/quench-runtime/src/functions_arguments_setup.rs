fn arguments_object(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    values: Vec<crate::value::Value>,
    environment: &std::rc::Rc<crate::environment::Environment>,
) -> crate::value::Value {
    let length = values.len() as f64;
    let strict =
        matches!(function.strictness, FunctionStrictness::Strict) || !function.mapped_arguments;
    let mut arguments = crate::value::ArrayData::new_arguments(values, strict);
    arguments.set_property("length", crate::value::Value::Number(length));
    arguments.set_property(
        "Symbol.iterator",
        crate::value::Value::Builtin(crate::ops::Builtin::ArrayIterator),
    );
    if strict {
        let realm = crate::vm::realm_id_for_global_value(&function.captures.get(0))
            .unwrap_or(crate::vm::current_context_or_default().realm());
        arguments.set_property(
            "\0realm",
            crate::vm::realm_token(realm).unwrap_or(crate::value::Value::Undefined),
        );
    }
    if matches!(function.strictness, FunctionStrictness::Sloppy) && function.mapped_arguments {
        map_arguments(&mut arguments, function, environment);
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
