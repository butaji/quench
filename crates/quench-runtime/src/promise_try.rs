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
    let promise = Rc::new(PromiseData::default());
    if !matches!(constructor, Value::Builtin(Builtin::Promise)) {
        let prototype = crate::execute::get_property_result(constructor, "prototype")?;
        if crate::value::is_object(&prototype) {
            promise.set_prototype(prototype);
        }
    }
    match crate::functions::execute_target(&callback, &Value::Undefined, &arguments[1..]) {
        Ok(value) => resolve_promise(&promise, value),
        Err(VmError::Thrown(reason)) => reject_promise(&promise, reason),
        Err(_) => reject_promise(&promise, Value::Undefined),
    }
    Ok(Value::Promise(promise))
}
