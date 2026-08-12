fn promise_try(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let constructor = receiver
        .filter(|value| match value {
            Value::Builtin(Builtin::Promise) => true,
            Value::Function(function) => crate::functions::is_constructible(function),
            _ => false,
        })
        .ok_or_else(|| {
            crate::value::error::throw_type_error("Promise.try receiver is not a constructor")
        })?;
    let callback = arguments.first().cloned().unwrap_or(Value::Undefined);
    if !crate::conversion::is_callable(&callback) {
        return Err(crate::value::error::throw_type_error(
            "Promise.try callback is not callable",
        ));
    }
    let promise = if matches!(constructor, Value::Builtin(Builtin::Promise)) {
        Value::Promise(Rc::new(PromiseData::default()))
    } else {
        let executor_target = Rc::new(PromiseData::default());
        let executor = bound_settler(Builtin::PromiseResolve, &executor_target);
        crate::construct::construct_value(constructor, &[executor])?
    };
    let Value::Promise(promise_data) = &promise else {
        return Err(crate::value::error::throw_type_error(
            "Promise.try constructor did not create a Promise",
        ));
    };
    match crate::functions::execute_target(&callback, &Value::Undefined, &arguments[1..]) {
        Ok(value) => resolve_promise(promise_data, value),
        Err(VmError::Thrown(reason)) => reject_promise(promise_data, reason),
        Err(_) => reject_promise(promise_data, Value::Undefined),
    }
    Ok(promise)
}
