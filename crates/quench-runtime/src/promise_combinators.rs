fn promise_combinator(kind: PromiseAggregateKind, arguments: &[Value]) -> Result<Value, VmError> {
    let source = arguments.first().cloned().unwrap_or(Value::Undefined);
    let values = crate::collections::iterator::collect_iterable(source)?;
    let result = Rc::new(PromiseData::default());
    let aggregate = Rc::new(PromiseAggregate {
        kind,
        result: Rc::clone(&result),
        resolve,
        reject,
        remaining: RefCell::new(values.len()),
        values: RefCell::new(vec![Value::Undefined; values.len()]),
        settled: RefCell::new(false),
    });
    let empty = values.is_empty();
    for (index, value) in values.into_iter().enumerate() {
        register_aggregate_value(&aggregate, index, value);
    }
    if empty {
        finish_empty_aggregate(&aggregate);
    }
    crate::promise::drain_microtasks();
    Ok(constructed.unwrap_or(Value::Promise(result)))
}

fn register_aggregate_value(aggregate: &Rc<PromiseAggregate>, index: usize, value: Value) {
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
}

fn aggregate_settle(aggregate: &Rc<PromiseAggregate>, index: usize, state: &PromiseState) {
    if *aggregate.settled.borrow() {
        return;
    }
    let Some(value) = settled_value(state) else {
        return;
    };
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
    let _ = crate::functions::execute_target(&aggregate.resolve, &Value::Undefined, &[value]);
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
