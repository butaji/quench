fn then_species_constructor(promise: &Value) -> Result<Value, VmError> {
    let constructor = crate::execute::get_property_result(promise, "constructor")?;
    if matches!(constructor, Value::Undefined) {
        return Ok(Value::Builtin(Builtin::Promise));
    }
    if matches!(constructor, Value::Null) || !crate::value::is_object(&constructor) {
        return Err(crate::value::error::throw_type_error(
            "Promise constructor must be an object",
        ));
    }
    let species = crate::execute::get_property_result(&constructor, "Symbol.species")?;
    let parent = crate::builtins::object::get_prototype_of(Some(&constructor))?;
    if !matches!(constructor, Value::Builtin(Builtin::Promise))
        && matches!(species, Value::Builtin(Builtin::Promise))
        && !crate::execute::has_own_property(&constructor, "Symbol.species")
    {
        return Ok(constructor);
    }
    if !matches!(constructor, Value::Builtin(Builtin::Promise))
        && matches!(parent, Value::Builtin(Builtin::Promise))
        && matches!(species, Value::Builtin(Builtin::Promise))
    {
        return Ok(constructor);
    }
    if !matches!(species, Value::Undefined | Value::Null) {
        return Ok(species);
    }
    Ok(if matches!(parent, Value::Builtin(Builtin::Promise)) {
        constructor
    } else {
        Value::Builtin(Builtin::Promise)
    })
}

fn construct_then_result(constructor: &Value) -> Result<(Value, Rc<PromiseData>), VmError> {
    if matches!(constructor, Value::Builtin(Builtin::Promise)) {
        let result = Rc::new(PromiseData::default());
        return Ok((Value::Promise(Rc::clone(&result)), result));
    }
    let capability = Rc::new(PromiseData::default());
    *capability.capability_executor.borrow_mut() = Some(crate::value::PromiseCapabilityExecutor {
        resolve: std::cell::RefCell::new(None),
        reject: std::cell::RefCell::new(None),
        called: std::cell::Cell::new(false),
    });
    let executor = capability_executor_function(&capability);
    let result = crate::construct::construct_value(constructor, &[executor])?;
    if !capability_callbacks_callable(&capability) {
        return Err(crate::value::error::throw_type_error(
            "Promise capability callbacks must be callable",
        ));
    }
    let result = attach_promise_data(result, Rc::clone(&capability));
    Ok((result, capability))
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
