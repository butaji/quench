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
    let Some(Value::Array(values)) = receiver else {
        return Err(crate::value::error::throw_type_error("Array method called on incompatible receiver"));
    };
    let Some(callback) = arguments.first() else {
        return Err(crate::value::error::throw_type_error("predicate must be callable"));
    };
    if !crate::conversion::is_callable(callback) {
        return Err(crate::value::error::throw_type_error("predicate must be callable"));
    }
    for index in (0..values.len()).rev() {
        let value = values.get_index(index).unwrap_or(Value::Undefined);
        let args = [
            value.clone(),
            Value::Number(index as f64),
            receiver.cloned().unwrap_or(Value::Undefined),
        ];
        if crate::execute::is_truthy(&crate::functions::execute_target(
            callback,
            &Value::Undefined,
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
