/// Execute Promise.prototype.finally.
pub fn promise_finally(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let Some(receiver) = receiver else {
        return Err(VmError::NotCallable);
    };
    let then = crate::execute::get_property_result(receiver, "then")?;
    if !crate::conversion::is_callable(&then) {
        return Err(crate::vm::not_callable());
    }
    let callback = arguments.first().cloned().unwrap_or(Value::Undefined);
    if !crate::conversion::is_callable(&callback) {
        return crate::functions::execute_target(
            &then,
            receiver,
            &[callback.clone(), callback],
        );
    }
    let fulfilled = finally_handler(Builtin::PromiseFinallyOnFulfilled, callback.clone());
    let rejected = finally_handler(Builtin::PromiseFinallyOnRejected, callback);
    crate::functions::execute_target(&then, receiver, &[fulfilled, rejected])
}

fn finally_handler(builtin: Builtin, callback: Value) -> Value {
    Value::BoundFunction(Rc::new(crate::value::BoundFunctionValue {
        target: Value::Builtin(builtin),
        receiver: callback,
        arguments: Vec::new(),
    }))
}

fn execute_finally_handler(
    fulfilled: bool,
    callback: Option<&Value>,
    original: Value,
) -> Result<Value, VmError> {
    let Some(callback) = callback else {
        return settle_original(fulfilled, original);
    };
    if !crate::conversion::is_callable(callback) {
        return settle_original(fulfilled, original);
    }
    let result = crate::functions::execute_target(callback, &Value::Undefined, &[])?;
    let promise = promise_resolve(&[result]);
    let on_settled = finally_settle_handler(fulfilled, original);
    promise_then(Some(&promise), &[on_settled])
}

fn settle_original(fulfilled: bool, original: Value) -> Result<Value, VmError> {
    if fulfilled {
        Ok(original)
    } else {
        Err(VmError::Thrown(original))
    }
}

fn finally_settle_handler(fulfilled: bool, original: Value) -> Value {
    Value::BoundFunction(Rc::new(crate::value::BoundFunctionValue {
        target: Value::Builtin(if fulfilled {
            Builtin::PromiseFinallyFulfilled
        } else {
            Builtin::PromiseFinallyRejected
        }),
        receiver: original,
        arguments: Vec::new(),
    }))
}

fn settle_finally_value(fulfilled: bool, original: Option<&Value>) -> Result<Value, VmError> {
    let value = original.cloned().unwrap_or(Value::Undefined);
    settle_original(fulfilled, value)
}
