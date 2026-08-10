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
    let environment = crate::environment::Environment::child(&function.captures, parameters);
    let arguments = arguments_object(function, original_arguments, &environment);
    let arguments_slot = function.captures.len() as u16 + function.params;
    environment.set(arguments_slot, arguments);
    (vec![crate::value::Value::Undefined; 32], environment)
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
    let this_value = crate::vm::bare_call_receiver(function, this_value);
    if matches!(function.kind, FunctionKind::Generator) {
        return Ok(crate::generator::create(function, &this_value, arguments));
    }
    let completion = execute_body(function, &this_value, arguments);
    if function.is_async {
        return Ok(crate::promise::from_async_completion(completion));
    }
    completion
}

pub(crate) fn execute_body(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    this_value: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    let _home = crate::super_scope::Guard::install(function, this_value);
    let (mut registers, environment) = build_registers(function, this_value, arguments);
    crate::vm::execute_in_environment(
        &function.body,
        &mut registers,
        &crate::vm::VmContext::default(),
        environment,
    )
}
