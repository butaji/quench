fn execute_forward_then_call(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    receiver: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Option<Result<crate::value::Value, crate::execute::VmError>> {
    let fact = function.code.facts().forward_then_call.as_deref()?;
    let first = match crate::execute::get_property_result(receiver, &fact.first_method) {
        Ok(first) if crate::conversion::is_callable(&first) => first,
        Ok(_) => return None,
        Err(error) => return Some(Err(error)),
    };
    if let Err(error) = crate::functions::execute_target(&first, receiver, arguments) {
        return Some(Err(error));
    }
    let nested = match crate::execute::get_property_result(receiver, &fact.nested_property) {
        Ok(nested) => nested,
        Err(error) => return Some(Err(error)),
    };
    let second = match crate::execute::get_property_result(&nested, &fact.nested_method) {
        Ok(second) if crate::conversion::is_callable(&second) => second,
        Ok(_) => return None,
        Err(error) => return Some(Err(error)),
    };
    let result = crate::functions::execute_target(&second, &nested, &[]);
    crate::execution_trace::kernel("forward_then_call", result.is_err());
    Some(result.map(|_| crate::value::Value::Undefined))
}
