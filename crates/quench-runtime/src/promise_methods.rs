fn then_species_constructor(promise: &Value) -> Result<Value, VmError> {
    let constructor = crate::execute::get_property_result(promise, "constructor")?;
    if matches!(constructor, Value::Undefined | Value::Null) {
        return Ok(Value::Builtin(Builtin::Promise));
    }
    let species = crate::execute::get_property_result(&constructor, "Symbol.species")?;
    Ok(if matches!(species, Value::Undefined | Value::Null) {
        Value::Builtin(Builtin::Promise)
    } else { species })
}

fn construct_then_result(constructor: &Value) -> Result<Value, VmError> {
    if matches!(constructor, Value::Builtin(Builtin::Promise)) {
        return Ok(new_promise());
    }
    let target = Rc::new(PromiseData::default());
    let executor = bound_settler(Builtin::PromiseResolve, &target);
    if let Value::BoundFunction(executor) = &executor {
        executor.properties.borrow_mut().push(("length".to_string(), Value::Number(2.0)));
    }
    crate::construct::construct_value(constructor, &[executor])
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
