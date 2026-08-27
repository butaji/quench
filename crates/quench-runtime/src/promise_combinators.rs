fn promise_combinator(
    kind: PromiseAggregateKind,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    promise_combinator_with_keys(kind, receiver, arguments, None)
}

fn promise_combinator_with_keys(
    kind: PromiseAggregateKind,
    receiver: Option<&Value>,
    arguments: &[Value],
    keys: Option<Vec<String>>,
) -> Result<Value, VmError> {
    let constructor = receiver.ok_or(VmError::NotCallable)?;
    let capability = crate::promise::new_promise_capability(constructor)?;
    promise_combinator_from_capability(kind, constructor, arguments, keys, capability)
}

fn promise_combinator_from_capability(
    kind: PromiseAggregateKind,
    constructor: &Value,
    arguments: &[Value],
    keys: Option<Vec<String>>,
    (result, capability_resolve, capability_reject): (Value, Value, Value),
) -> Result<Value, VmError> {
    let promise_resolve = match get_promise_resolve(Some(constructor)) {
        Ok(resolve) => resolve,
        Err(error) => {
            reject_with_completion_value(&capability_reject, error);
            return Ok(result);
        }
    };
    let source = arguments.first().cloned().unwrap_or(Value::Undefined);
    let aggregate = Rc::new(PromiseAggregate {
        kind,
        resolve: capability_resolve,
        reject: capability_reject,
        // The sentinel keeps a synchronously-settling thenable from
        // resolving the aggregate before the iterator loop has finished.
        remaining: RefCell::new(1),
        values: RefCell::new(Vec::new()),
        called: RefCell::new(Vec::new()),
        keys,
        settled: RefCell::new(false),
    });
    let iterator = match crate::collections::iterator::open(source) {
        Ok(iterator) => iterator,
        Err(error) => {
            reject_with_completion_value(&aggregate.reject, error);
            return Ok(result);
        }
    };
    loop {
        let value = match crate::collections::iterator::step_value(&iterator) {
            Ok(Some(value)) => value,
            Ok(None) => break,
            Err(error) => {
                reject_with_completion_value(&aggregate.reject, error);
                break;
            }
        };
        let index = {
            let mut values = aggregate.values.borrow_mut();
            let index = values.len();
            values.push(Value::Undefined);
            aggregate.called.borrow_mut().push(false);
            *aggregate.remaining.borrow_mut() += 1;
            index
        };
        let value = match crate::functions::execute_target(&promise_resolve, constructor, &[value]) {
            Ok(value) => value,
            Err(error) => {
                if let VmError::Thrown(reason) = &error {
                    let _ = crate::collections::iterator::close(
                        iterator.clone(),
                        crate::completion::Completion::Throw(reason.clone()),
                    );
                }
                reject_with_completion_value(&aggregate.reject, error);
                break;
            }
        };
        if let Err(error) = register_aggregate_value(&aggregate, index, value) {
            if let VmError::Thrown(reason) = &error {
                let _ = crate::collections::iterator::close(
                    iterator.clone(),
                    crate::completion::Completion::Throw(reason.clone()),
                );
            }
            reject_with_completion_value(&aggregate.reject, error);
            break;
        }
    }
    let remaining = *aggregate.remaining.borrow();
    *aggregate.remaining.borrow_mut() = remaining.saturating_sub(1);
    if *aggregate.remaining.borrow() == 0 {
        if aggregate.values.borrow().is_empty() {
            finish_empty_aggregate(&aggregate);
        } else {
            match kind {
                PromiseAggregateKind::All | PromiseAggregateKind::AllSettled => {
                    resolve_aggregate(&aggregate, Value::array(aggregate.values.borrow().clone()))
                }
                PromiseAggregateKind::Any => {
                    let errors = Value::array(aggregate.values.borrow().clone());
                    let error = crate::construct::construct_value(
                        &Value::Builtin(Builtin::AggregateError),
                        &[errors],
                    )
                    .unwrap_or(Value::Undefined);
                    reject_aggregate(&aggregate, error);
                }
                PromiseAggregateKind::Race => {}
            }
        }
    }
    Ok(result)
}

fn promise_keyed_combinator(
    kind: PromiseAggregateKind,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let constructor = receiver.ok_or(VmError::NotCallable)?;
    let capability = crate::promise::new_promise_capability(constructor)?;
    let source = arguments.first().cloned().unwrap_or(Value::Undefined);
    if !crate::value::is_object(&source) {
        reject_with_completion_value(
            &capability.2,
            crate::value::error::throw_type_error("Promise keyed input must be an object"),
        );
        return Ok(capability.0);
    }
    let keys = crate::own_keys::enumerable_key_strings(Some(&source));
    let values = match keys
        .iter()
        .map(|key| crate::execute::get_property_result(&source, key))
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(values) => values,
        Err(error) => {
            reject_with_completion_value(&capability.2, error);
            return Ok(capability.0);
        }
    };
    promise_combinator_from_capability(
        kind,
        constructor,
        &[Value::array(values)],
        Some(keys),
        capability,
    )
}

fn get_promise_resolve(receiver: Option<&Value>) -> Result<Value, VmError> {
    let receiver = receiver.ok_or(VmError::NotCallable)?;
    crate::execute::get_property_result(receiver, "resolve")
}

fn reject_with_completion_value(reject: &Value, error: VmError) {
    let reason = match error {
        VmError::Thrown(reason) => reason,
        _ => Value::Undefined,
    };
    let _ = crate::functions::execute_target(reject, &Value::Undefined, &[reason]);
}

fn register_aggregate_value(
    aggregate: &Rc<PromiseAggregate>,
    index: usize,
    value: Value,
) -> Result<(), VmError> {
    if aggregate.kind == PromiseAggregateKind::Race {
        if crate::value::is_object(&value) {
            match crate::execute::get_property_result(&value, "then") {
                Ok(then) if crate::conversion::is_callable(&then) => {
                    crate::functions::execute_target(
                        &then,
                        &value,
                        &[aggregate.resolve.clone(), aggregate.reject.clone()],
                    )?;
                }
                Ok(_) => {
                    crate::functions::execute_target(
                        &aggregate.resolve,
                        &Value::Undefined,
                        &[value],
                    )?;
                }
                Err(error) => {
                    crate::functions::execute_target(
                        &aggregate.reject,
                        &Value::Undefined,
                        &[Value::Undefined],
                    )?;
                    return Err(error);
                }
            }
        } else {
            crate::functions::execute_target(
                &aggregate.resolve,
                &Value::Undefined,
                &[value],
            )?;
        }
        return Ok(());
    }
    // PerformPromiseAll invokes the resolved promise's `then` method during
    // the combinator call.  Assimilation of a custom thenable may therefore
    // settle the aggregate synchronously, while native Promise reactions
    // remain queued as microtasks.
    let promise = PromiseData::allocate(PromiseState::Pending);
    let resolve = bound_settler(Builtin::PromiseResolve, &promise, 1.0);
    let reject = bound_settler(Builtin::PromiseReject, &promise, 1.0);
    if crate::value::is_object(&value) {
        match crate::execute::get_property_result(&value, "then") {
            Ok(then) if crate::conversion::is_callable(&then) => {
                let rejection = if aggregate.kind == PromiseAggregateKind::All {
                    aggregate.reject.clone()
                } else {
                    reject.clone()
                };
                let call_result = crate::functions::execute_target(
                    &then,
                    &value,
                    &[resolve.clone(), rejection],
                );
                if call_result.is_err() {
                    let _ = crate::functions::execute_target(
                        &reject,
                        &Value::Undefined,
                        &[Value::Undefined],
                    );
                    return call_result.map(|_| ());
                }
            }
            Ok(_) => {
                let _ = crate::functions::execute_target(&resolve, &Value::Undefined, &[value]);
            }
            Err(error) => {
                let _ = crate::functions::execute_target(
                    &reject,
                    &Value::Undefined,
                    &[Value::Undefined],
                );
                return Err(error);
            }
        }
    } else {
        let _ = crate::functions::execute_target(&resolve, &Value::Undefined, &[value]);
    }
    let state = promise.state.borrow().clone();
    if !matches!(state, PromiseState::Pending) {
        aggregate_settle(aggregate, index, &state);
        return Ok(());
    }
    promise.add_aggregate_hook(Rc::clone(aggregate), index);
    Ok(())
}

fn aggregate_settle(aggregate: &Rc<PromiseAggregate>, index: usize, state: &PromiseState) {
    if *aggregate.settled.borrow() {
        return;
    }
    let guard = matches!(aggregate.kind, PromiseAggregateKind::All | PromiseAggregateKind::AllSettled)
        || (aggregate.kind == PromiseAggregateKind::Any
            && matches!(state, PromiseState::Rejected(_)));
    if guard {
        let mut called = aggregate.called.borrow_mut();
        if called.get(index).copied().unwrap_or(false) {
            return;
        }
        if let Some(entry) = called.get_mut(index) {
            *entry = true;
        }
    }
    let Some(value) = settled_value(state) else { return };
    match (aggregate.kind, state) {
        (PromiseAggregateKind::Race, PromiseState::Fulfilled(_))
        | (PromiseAggregateKind::Any, PromiseState::Fulfilled(_)) => {
            resolve_aggregate(aggregate, value)
        }
        (PromiseAggregateKind::Race, PromiseState::Rejected(_))
        | (PromiseAggregateKind::All, PromiseState::Rejected(_)) => {
            reject_aggregate(aggregate, value)
        }
        (PromiseAggregateKind::Any, PromiseState::Rejected(_)) => {
            store_aggregate(aggregate, index, value);
            decrement_aggregate(aggregate);
            if *aggregate.remaining.borrow() == 0 {
                let errors = Value::array(aggregate.values.borrow().clone());
                let error = crate::construct::construct_value(
                    &Value::Builtin(Builtin::AggregateError),
                    &[errors],
                )
                .unwrap_or(Value::Undefined);
                reject_aggregate(aggregate, error);
            }
        }
        (PromiseAggregateKind::AllSettled, PromiseState::Fulfilled(_)) => {
            store_aggregate(aggregate, index, settled_record("fulfilled", value));
            decrement_aggregate(aggregate);
            finish_when_empty(aggregate);
        }
        (PromiseAggregateKind::AllSettled, PromiseState::Rejected(_)) => {
            store_aggregate(aggregate, index, settled_record("rejected", value));
            decrement_aggregate(aggregate);
            finish_when_empty(aggregate);
        }
        (PromiseAggregateKind::All, PromiseState::Fulfilled(_)) => {
            store_aggregate(aggregate, index, value);
            decrement_aggregate(aggregate);
            finish_when_empty(aggregate);
        }
        _ => {}
    }
}

fn settled_value(state: &PromiseState) -> Option<Value> {
    match state {
        PromiseState::Fulfilled(value) | PromiseState::Rejected(value) => Some(value.clone()),
        PromiseState::Pending => None,
    }
}

fn store_aggregate(aggregate: &PromiseAggregate, index: usize, value: Value) {
    if let Some(slot) = aggregate.values.borrow_mut().get_mut(index) {
        *slot = value;
    }
}

fn decrement_aggregate(aggregate: &PromiseAggregate) {
    let mut remaining = aggregate.remaining.borrow_mut();
    *remaining = remaining.saturating_sub(1);
}

fn finish_when_empty(aggregate: &PromiseAggregate) {
    if *aggregate.remaining.borrow() == 0 {
        resolve_aggregate(aggregate, Value::array(aggregate.values.borrow().clone()));
    }
}

fn finish_empty_aggregate(aggregate: &PromiseAggregate) {
    match aggregate.kind {
        PromiseAggregateKind::All | PromiseAggregateKind::AllSettled => {
            resolve_aggregate(aggregate, Value::array(Vec::new()))
        }
        PromiseAggregateKind::Any => reject_aggregate(aggregate, Value::array(Vec::new())),
        PromiseAggregateKind::Race => {}
    }
}

fn resolve_aggregate(aggregate: &PromiseAggregate, value: Value) {
    *aggregate.settled.borrow_mut() = true;
    let value = if let Some(keys) = &aggregate.keys {
        let values = match value {
            Value::Array(values) => values.snapshot(),
            _ => Vec::new(),
        };
        let mut properties = vec![("\0prototype".to_string(), Value::Null)];
        properties.extend(keys.iter().enumerate().map(|(index, key)| {
            (
                key.clone(),
                values.get(index).cloned().unwrap_or(Value::Undefined),
            )
        }));
        Value::Object(Rc::new(crate::value::ObjectData::new(properties)))
    } else {
        value
    };
    if crate::functions::execute_target(&aggregate.resolve, &Value::Undefined, &[value]).is_err()
    {
        // A user-supplied capability resolver is allowed to throw; the
        // reject callback remains the only observable completion path.
    }
}

fn reject_aggregate(aggregate: &PromiseAggregate, value: Value) {
    *aggregate.settled.borrow_mut() = true;
    let _ = crate::functions::execute_target(&aggregate.reject, &Value::Undefined, &[value]);
}

fn settled_record(status: &str, value: Value) -> Value {
    Value::Object(Rc::new(crate::value::ObjectData::new(vec![
        ("status".to_string(), Value::String(status.to_string())),
        (
            if status == "fulfilled" {
                "value".to_string()
            } else {
                "reason".to_string()
            },
            value,
        ),
    ])))
}
