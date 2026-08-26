fn then_species_constructor(promise: &Value) -> Result<Value, VmError> {
    let constructor = crate::execute::get_property_result(promise, "constructor")?;
    if matches!(constructor, Value::Undefined) {
        return Ok(Value::Builtin(Builtin::Promise));
    }
    if !crate::value::is_object(&constructor) {
        return Err(crate::value::error::throw_type_error(
            "Promise constructor is not an object",
        ));
    }
    let species = crate::execute::get_property_result(&constructor, "Symbol.species")?;
    Ok(if matches!(species, Value::Undefined | Value::Null) {
        Value::Builtin(Builtin::Promise)
    } else { species })
}

fn construct_then_result(constructor: &Value) -> Result<(Value, Value, Value), VmError> {
    if matches!(constructor, Value::Builtin(Builtin::Promise)) {
        let target = new_promise();
        let Value::Promise(promise) = &target else {
            return Err(crate::vm::not_callable());
        };
        let resolve = bound_settler(Builtin::PromiseResolve, promise, 1.0);
        let reject = bound_settler(Builtin::PromiseReject, promise, 1.0);
        return Ok((target, resolve, reject));
    }
    crate::promise::new_promise_capability(constructor)
}

pub fn promise_catch(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let Some(receiver) = receiver else { return Err(VmError::NotCallable); };
    let then = crate::execute::get_property_result(receiver, "then")?;
    if !crate::conversion::is_callable(&then) { return Err(crate::vm::not_callable()); }
    crate::functions::execute_target(&then, receiver, &[
        Value::Undefined,
        arguments.first().cloned().unwrap_or(Value::Undefined),
    ])
}
