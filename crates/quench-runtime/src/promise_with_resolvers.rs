fn with_resolvers(receiver: Option<&Value>) -> Result<Value, VmError> {
    let constructor = receiver.filter(|value| match value {
        Value::Builtin(Builtin::Promise) => true,
        Value::Function(function) => crate::functions::is_constructible(function),
        _ => false,
    });
    if constructor.is_none() {
        return Err(crate::value::error::throw_type_error(
            "Promise.withResolvers receiver is not a constructor",
        ));
    }
    let promise = Rc::new(PromiseData::default());
    if let Some(constructor) = constructor.filter(|value| !matches!(value, Value::Builtin(Builtin::Promise))) {
        let prototype = crate::execute::get_property_result(constructor, "prototype")?;
        if crate::value::is_object(&prototype) {
            promise.set_prototype(prototype);
        }
    }
    let resolve = bound_settler(Builtin::PromiseResolve, &promise);
    let reject = bound_settler(Builtin::PromiseReject, &promise);
    Ok(Value::Object(Rc::new(crate::value::ObjectData::new(vec![
        ("promise".to_string(), Value::Promise(promise)),
        ("resolve".to_string(), resolve),
        ("reject".to_string(), reject),
    ]))))
}
