fn set_promise_state(promise: &Rc<PromiseData>, state: PromiseState) {
    let result = match &state {
        PromiseState::Pending => None,
        PromiseState::Fulfilled(value) | PromiseState::Rejected(value) => Some(value.clone()),
    };
    *promise.state.borrow_mut() = state;
    *promise.result.borrow_mut() = result;
}

fn claim_promise(promise: &Rc<PromiseData>) -> bool {
    !promise.already_resolved.replace(true)
}

fn settle_fulfilled(promise: &Rc<PromiseData>, value: Value) {
    if matches!(*promise.state.borrow(), PromiseState::Pending) {
        set_promise_state(promise, PromiseState::Fulfilled(value));
        queue_promise(promise);
    }
}

fn settle_rejected(promise: &Rc<PromiseData>, reason: Value) {
    if matches!(*promise.state.borrow(), PromiseState::Pending) {
        set_promise_state(promise, PromiseState::Rejected(reason));
        queue_promise(promise);
    }
}

fn adopt_resolve(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::Promise(promise)) = receiver else {
        return Err(VmError::NotCallable);
    };
    settle_fulfilled(promise, arguments.first().cloned().unwrap_or(Value::Undefined));
    Ok(Value::Undefined)
}

fn adopt_reject(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::Promise(promise)) = receiver else {
        return Err(VmError::NotCallable);
    };
    settle_rejected(promise, arguments.first().cloned().unwrap_or(Value::Undefined));
    Ok(Value::Undefined)
}
