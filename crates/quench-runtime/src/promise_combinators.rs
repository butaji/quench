fn promise_combinator(
    kind: PromiseAggregateKind,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let result = Rc::new(PromiseData::default());
    let constructor = receiver.ok_or(VmError::NotCallable)?;
    let resolve = match get_promise_resolve(Some(constructor)) {
        Ok(resolve) => resolve,
        Err(error) => {
            reject_with_completion(&result, error);
            return Ok(Value::Promise(result));
        }
    };
    let source = arguments.first().cloned().unwrap_or(Value::Undefined);
    let values = match crate::collections::iterator::collect_iterable(source) {
        Ok(values) => values,
        Err(error) => {
            reject_with_completion(&result, error);
            return Ok(Value::Promise(result));
        }
    };
    let aggregate = Rc::new(PromiseAggregate {
        kind,
        result: Rc::clone(&result),
        remaining: RefCell::new(values.len()),
        values: RefCell::new(vec![Value::Undefined; values.len()]),
        settled: RefCell::new(false),
    });
    for (index, value) in values.into_iter().enumerate() {
        let value = match crate::functions::execute_target(&resolve, constructor, &[value]) {
            Ok(value) => value,
            Err(error) => {
                reject_with_completion(&result, error);
                break;
            }
        };
        register_aggregate_value(&aggregate, index, value);
    }
    if *aggregate.remaining.borrow() == 0 {
        finish_empty_aggregate(&aggregate);
    }
    Ok(Value::Promise(result))
}

fn get_promise_resolve(receiver: Option<&Value>) -> Result<Value, VmError> {
    let receiver = receiver.ok_or(VmError::NotCallable)?;
    crate::execute::get_property_result(receiver, "resolve")
}

fn reject_with_completion(promise: &Rc<PromiseData>, error: VmError) {
    let reason = match error {
        VmError::Thrown(reason) => reason,
        _ => Value::Undefined,
    };
    reject_promise(promise, reason);
}

fn register_aggregate_value(aggregate: &Rc<PromiseAggregate>, index: usize, value: Value) {
    let value = if crate::value::is_object(&value) {
        resolve_value(value)
    } else {
        value
    };
    let Value::Promise(promise) = value else {
        aggregate_settle(aggregate, index, &PromiseState::Fulfilled(value));
        return;
    };
    promise
        .continuations
        .borrow_mut()
        .push(PromiseContinuation::Aggregate {
            aggregate: Rc::clone(aggregate),
            index,
        });
    if !matches!(*promise.state.borrow(), PromiseState::Pending) {
        queue_promise(&promise);
    }
    drain_microtasks();
}

fn aggregate_settle(aggregate: &Rc<PromiseAggregate>, index: usize, state: &PromiseState) {
    if *aggregate.settled.borrow() {
        return;
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
                reject_aggregate(aggregate, Value::array(aggregate.values.borrow().clone()));
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
    resolve_promise(&aggregate.result, value);
}

fn reject_aggregate(aggregate: &PromiseAggregate, value: Value) {
    *aggregate.settled.borrow_mut() = true;
    reject_promise(&aggregate.result, value);
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
