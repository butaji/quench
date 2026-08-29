fn execute_forward_construct_call(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    receiver: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Option<Result<crate::value::Value, crate::execute::VmError>> {
    let fact = function.code.facts().forward_construct_call.as_deref()?;
    if arguments.len() != usize::from(function.params) {
        return None;
    }
    let constructor = function.captures.get(fact.constructor_slot);
    let created_arguments = fact
        .constructor_arguments
        .iter()
        .map(|source| forward_value(source, function, receiver, arguments))
        .collect::<Option<Vec<_>>>()?;
    let created = match crate::construct::construct_value(&constructor, &created_arguments) {
        Ok(created) => created,
        Err(error) => return Some(Err(error)),
    };
    let method = match crate::execute::get_property_result(receiver, &fact.method) {
        Ok(method) if crate::conversion::is_callable(&method) => method,
        Ok(_) => return None,
        Err(error) => return Some(Err(error)),
    };
    let mut forwarded = Vec::with_capacity(fact.forwarded_arguments.len() + 1);
    for index in fact.forwarded_arguments.iter() {
        forwarded.push(arguments[usize::from(*index)].clone());
    }
    forwarded.push(created);
    let result = crate::functions::execute_target(&method, receiver, &forwarded);
    crate::execution_trace::kernel("forward_construct_call", result.is_err());
    Some(result.map(|_| crate::value::Value::Undefined))
}

fn forward_value(
    source: &crate::facts::ForwardValueSource,
    function: &crate::value::FunctionValue,
    receiver: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Option<crate::value::Value> {
    use crate::facts::ForwardValueSource::*;
    match source {
        Receiver => Some(receiver.clone()),
        Argument(index) => arguments.get(usize::from(*index)).cloned(),
        Integer(value) => Some(crate::value::Value::Number(f64::from(*value))),
        Capture(slot) => Some(function.captures.get(*slot)),
    }
}
