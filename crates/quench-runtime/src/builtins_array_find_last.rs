pub(crate) fn array_find_last(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    find_last(receiver, arguments, false)
}

pub(crate) fn array_find_last_index(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    find_last(receiver, arguments, true)
}

fn find_last(
    receiver: Option<&Value>,
    arguments: &[Value],
    index_result: bool,
) -> Result<Value, crate::execute::VmError> {
    let Some(receiver) = receiver.filter(|value| !matches!(value, Value::Null | Value::Undefined)) else {
        return Err(crate::value::error::throw_type_error("Array method called on incompatible receiver"));
    };
    let length = crate::builtins::map_length(receiver)?;
    let Some(callback) = arguments.first() else {
        return Err(crate::value::error::throw_type_error("predicate must be callable"));
    };
    if !crate::conversion::is_callable(callback) {
        return Err(crate::value::error::throw_type_error("predicate must be callable"));
    }
    let this_arg = arguments.get(1).map_or(&Value::Undefined, |value| value);
    for index in (0..length).rev() {
        let value = crate::execute::get_property_result(receiver, &index.to_string())?;
        let args = [
            value.clone(),
            Value::Number(index as f64),
            receiver.clone(),
        ];
        if crate::execute::is_truthy(&crate::functions::execute_target(
            callback,
            this_arg,
            &args,
        )?) {
            return Ok(if index_result {
                Value::Number(index as f64)
            } else {
                value
            });
        }
    }
    Ok(if index_result {
        Value::Number(-1.0)
    } else {
        Value::Undefined
    })
}
