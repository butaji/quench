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
    let capability = Rc::new(PromiseData::default());
    *capability.capability_executor.borrow_mut() = Some(
        crate::value::PromiseCapabilityExecutor {
            resolve: RefCell::new(None),
            reject: RefCell::new(None),
            called: std::cell::Cell::new(false),
        },
    );
    let mut returned = None;
    let result = if matches!(constructor, Value::Builtin(Builtin::Promise)) {
        Rc::clone(&capability)
    } else {
        let executor = capability_executor_function(&capability);
        match crate::construct::construct_value(constructor, &[executor]) {
            Ok(value) => {
                if !capability_callbacks_callable(&capability) {
                    return Err(crate::value::error::throw_type_error(
                        "Promise capability callbacks must be callable",
                    ));
                }
                let value = attach_promise_data(value, Rc::clone(&capability));
                returned = Some(value);
                if let Ok(prototype) = crate::execute::get_property_result(constructor, "prototype") {
                    if crate::value::is_object(&prototype) {
                        capability.set_prototype(prototype);
                    }
                }
                Rc::clone(&capability)
            }
            Err(error) => return Err(error),
        }
    };
    let resolve = match get_promise_resolve(Some(constructor)) {
        Ok(resolve) => resolve,
        Err(error) => {
            reject_both(&result, &capability, error);
            return Ok(returned.unwrap_or(Value::Promise(result)));
        }
    };
    let aggregate = Rc::new(PromiseAggregate {
        kind,
        result: Rc::clone(&result),
        capability: Rc::clone(&capability),
        remaining: RefCell::new(1),
        values: RefCell::new(Vec::new()),
        keys,
        settled: RefCell::new(false),
    });
    let source = arguments.first().cloned().unwrap_or(Value::Undefined);
    let iterator = match crate::collections::iterator::open(source) {
        Ok(iterator) => iterator,
        Err(error) => {
            reject_both(&result, &capability, error);
            return Ok(returned.unwrap_or(Value::Promise(result)));
        }
    };
    loop {
        let item = match crate::collections::iterator::step_value(&iterator) {
            Ok(Some(item)) => item,
            Ok(None) => break,
            Err(error) => {
                reject_both(&result, &capability, error);
                break;
            }
        };
        let index = aggregate.values.borrow().len();
        aggregate.values.borrow_mut().push(Value::Undefined);
        *aggregate.remaining.borrow_mut() += 1;
        let value = match crate::functions::execute_target(&resolve, constructor, &[item]) {
            Ok(value) => value,
            Err(error) => {
                close_on_error(&iterator, &result, &capability, error);
                break;
            }
        };
        if let Err(error) = register_aggregate_value(&aggregate, index, value) {
            close_on_error(&iterator, &result, &capability, error);
            break;
        }
    }
    decrement_aggregate(&aggregate);
    if *aggregate.remaining.borrow() == 0 {
        finish_empty_aggregate(&aggregate);
    }
    Ok(returned.unwrap_or(Value::Promise(result)))
}

fn capability_callbacks_callable(capability: &Rc<PromiseData>) -> bool {
    let executor = capability.capability_executor.borrow();
    let Some(executor) = executor.as_ref() else {
        return false;
    };
    executor
        .resolve
        .borrow()
        .as_ref()
        .is_some_and(crate::conversion::is_callable)
        && executor
            .reject
            .borrow()
            .as_ref()
            .is_some_and(crate::conversion::is_callable)
}

fn promise_keyed_combinator(
    kind: PromiseAggregateKind,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let source = arguments.first().cloned().unwrap_or(Value::Undefined);
    let keys = crate::own_keys::enumerable_key_strings(Some(&source));
    let values = keys
        .iter()
        .map(|key| crate::execute::get_property_result(&source, key))
        .collect::<Result<Vec<_>, _>>()?;
    promise_combinator_with_keys(kind, receiver, &[Value::array(values)], Some(keys))
}

fn get_promise_resolve(receiver: Option<&Value>) -> Result<Value, VmError> {
    let receiver = receiver.ok_or(VmError::NotCallable)?;
    crate::execute::get_property_result(receiver, "resolve")
}

fn aggregate_callback(
    receiver: Option<&Value>,
    arguments: &[Value],
    fulfilled: bool,
) -> Result<Value, VmError> {
    let Some(Value::Promise(callback)) = receiver else {
        return Ok(Value::Undefined);
    };
    let callback = callback.aggregate_callback.borrow().clone();
    let Some(callback) = callback else {
        return Ok(Value::Undefined);
    };
    let (aggregate, index, called) = match callback {
        crate::value::PromiseAggregateCallback::Resolve {
            aggregate,
            index,
            called,
        }
        | crate::value::PromiseAggregateCallback::Reject {
            aggregate,
            index,
            called,
        } => (aggregate, index, called),
    };
    if called.replace(true) {
        return Ok(Value::Undefined);
    }
    let value = arguments.first().cloned().unwrap_or(Value::Undefined);
    let state = if fulfilled {
        PromiseState::Fulfilled(value)
    } else {
        PromiseState::Rejected(value)
    };
    aggregate_settle(&aggregate, index, &state);
    Ok(Value::Undefined)
}

fn reject_with_completion(promise: &Rc<PromiseData>, error: VmError) {
    let reason = match error {
        VmError::Thrown(reason) => reason,
        _ => Value::Undefined,
    };
    reject_promise(promise, reason);
}

fn register_aggregate_value(
    aggregate: &Rc<PromiseAggregate>,
    index: usize,
    value: Value,
) -> Result<(), VmError> {
    let Value::Promise(promise) = &value else {
        if !crate::value::is_object(&value) {
            aggregate_settle(aggregate, index, &PromiseState::Fulfilled(value));
            return Ok(());
        }
        let then = match crate::execute::get_property_result(&value, "then") {
            Ok(then) => then,
            Err(error) => return Err(error),
        };
        if !crate::conversion::is_callable(&then) {
            aggregate_settle(aggregate, index, &PromiseState::Fulfilled(value));
            return Ok(());
        }
        let resolve_element = aggregate_resolve_callback(aggregate, index);
        let reject_element = aggregate_reject_callback(aggregate, index);
        let (resolve, reject) = if matches!(
            aggregate.kind,
            PromiseAggregateKind::All | PromiseAggregateKind::Any | PromiseAggregateKind::Race
        ) {
            (resolve_element, reject_element)
        } else {
            aggregate_callback_pair(aggregate, index)
        };
        let result = crate::functions::execute_target(&then, &value, &[resolve, reject]);
        result?;
        return Ok(());
    };
    let promise = Rc::clone(promise);
    let then = match crate::execute::get_property_result(&Value::Promise(Rc::clone(&promise)), "then") {
        Ok(then) => then,
        Err(error) => return Err(error),
    };
    if !is_native_promise_then(&then) {
        let resolve = aggregate_resolve_callback(aggregate, index);
        let reject = aggregate_reject_callback(aggregate, index);
        let (resolve, reject) = if matches!(aggregate.kind, PromiseAggregateKind::All | PromiseAggregateKind::Any | PromiseAggregateKind::Race) {
            (resolve, reject)
        } else {
            aggregate_callback_pair(aggregate, index)
        };
        let result = crate::functions::execute_target(
            &then,
            &Value::Promise(Rc::clone(&promise)),
            &[resolve, reject],
        );
        result?;
        return Ok(());
    }
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
    Ok(())
}

fn aggregate_reject_callback(aggregate: &Rc<PromiseAggregate>, index: usize) -> Value {
    if !matches!(
        aggregate.kind,
        PromiseAggregateKind::All | PromiseAggregateKind::Race
    ) {
        return aggregate_callback_function(aggregate, index, false);
    }
    aggregate
        .capability
        .capability_executor
        .borrow()
        .as_ref()
        .and_then(|executor| executor.reject.borrow().clone())
        .filter(crate::conversion::is_callable)
        .unwrap_or_else(|| aggregate_callback_function(aggregate, index, false))
}

fn aggregate_resolve_callback(aggregate: &Rc<PromiseAggregate>, index: usize) -> Value {
    if matches!(aggregate.kind, PromiseAggregateKind::Race | PromiseAggregateKind::Any) {
        return aggregate
            .capability
            .capability_executor
            .borrow()
            .as_ref()
            .and_then(|executor| executor.resolve.borrow().clone())
            .filter(crate::conversion::is_callable)
            .unwrap_or_else(|| aggregate_callback_function(aggregate, index, true));
    }
    aggregate_callback_function(aggregate, index, true)
}

fn aggregate_callback_pair(aggregate: &Rc<PromiseAggregate>, index: usize) -> (Value, Value) {
    let called = Rc::new(std::cell::Cell::new(false));
    (
        aggregate_callback_function_with_called(aggregate, index, true, Rc::clone(&called)),
        aggregate_callback_function_with_called(aggregate, index, false, called),
    )
}

fn is_native_promise_then(value: &Value) -> bool {
    matches!(value, Value::Builtin(Builtin::PromiseThen))
        || matches!(value, Value::BoundFunction(bound) if bound.target == Value::Builtin(Builtin::PromiseThen))
}

fn aggregate_callback_function(
    aggregate: &Rc<PromiseAggregate>,
    index: usize,
    fulfilled: bool,
) -> Value {
    aggregate_callback_function_with_called(
        aggregate,
        index,
        fulfilled,
        Rc::new(std::cell::Cell::new(false)),
    )
}

fn aggregate_callback_function_with_called(
    aggregate: &Rc<PromiseAggregate>,
    index: usize,
    fulfilled: bool,
    called: Rc<std::cell::Cell<bool>>,
) -> Value {
    let callback = Rc::new(PromiseData::default());
    *callback.aggregate_callback.borrow_mut() = Some(if fulfilled {
        crate::value::PromiseAggregateCallback::Resolve {
            aggregate: Rc::clone(aggregate),
            index,
            called,
        }
    } else {
        crate::value::PromiseAggregateCallback::Reject {
            aggregate: Rc::clone(aggregate),
            index,
            called,
        }
    });
    let length = Value::Number(1.0);
    let descriptor = Value::Object(Rc::new(crate::value::ObjectData::new(vec![
        ("value".to_string(), length.clone()),
        ("writable".to_string(), Value::Boolean(false)),
        ("enumerable".to_string(), Value::Boolean(false)),
        ("configurable".to_string(), Value::Boolean(true)),
    ])));
    let name = Value::String(String::new());
    let name_descriptor = Value::Object(Rc::new(crate::value::ObjectData::new(vec![
        ("value".to_string(), name.clone()),
        ("writable".to_string(), Value::Boolean(false)),
        ("enumerable".to_string(), Value::Boolean(false)),
        ("configurable".to_string(), Value::Boolean(true)),
    ])));
    Value::BoundFunction(Rc::new(crate::value::BoundFunctionValue {
        realm: crate::vm::current_context_or_default().realm(),
        target: Value::Builtin(if fulfilled {
            Builtin::PromiseAggregateResolve
        } else {
            Builtin::PromiseAggregateReject
        }),
        receiver: Value::Promise(callback),
        arguments: Vec::new(),
        properties: RefCell::new(vec![
            ("length".to_string(), length),
            (crate::builtins::descriptor_key("length"), descriptor),
            ("name".to_string(), name),
            (crate::builtins::descriptor_key("name"), name_descriptor),
            ("\0receiver_bound_method".to_string(), Value::Boolean(true)),
        ]),
    }))
}

fn capability_executor(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::Promise(state)) = receiver else {
        return Ok(Value::Undefined);
    };
    let executors = state.capability_executor.borrow();
    let Some(executor) = executors.as_ref() else {
        return Ok(Value::Undefined);
    };
    let resolve = arguments.first().cloned().unwrap_or(Value::Undefined);
    let reject = arguments.get(1).cloned().unwrap_or(Value::Undefined);
    let already_captured = executor
        .resolve
        .borrow()
        .as_ref()
        .is_some_and(|value| !matches!(value, Value::Undefined))
        || executor
            .reject
            .borrow()
            .as_ref()
            .is_some_and(|value| !matches!(value, Value::Undefined));
    if already_captured {
        return Err(crate::value::error::throw_type_error(
            "Promise capability executor called twice",
        ));
    }
    executor.resolve.replace(Some(resolve));
    executor.reject.replace(Some(reject));
    Ok(Value::Undefined)
}

fn capability_executor_function(state: &Rc<PromiseData>) -> Value {
    let length = Value::Number(2.0);
    let descriptor = Value::Object(Rc::new(crate::value::ObjectData::new(vec![
        ("value".to_string(), length.clone()),
        ("writable".to_string(), Value::Boolean(false)),
        ("enumerable".to_string(), Value::Boolean(false)),
        ("configurable".to_string(), Value::Boolean(true)),
    ])));
    let name = Value::String(String::new());
    let name_descriptor = Value::Object(Rc::new(crate::value::ObjectData::new(vec![
        ("value".to_string(), name.clone()),
        ("writable".to_string(), Value::Boolean(false)),
        ("enumerable".to_string(), Value::Boolean(false)),
        ("configurable".to_string(), Value::Boolean(true)),
    ])));
    Value::BoundFunction(Rc::new(crate::value::BoundFunctionValue {
        realm: crate::vm::current_context_or_default().realm(),
        target: Value::Builtin(Builtin::PromiseCapabilityExecutor),
        receiver: Value::Promise(Rc::clone(state)),
        arguments: Vec::new(),
        properties: RefCell::new(vec![
            ("length".to_string(), length),
            (crate::builtins::descriptor_key("length"), descriptor),
            ("name".to_string(), name),
            (crate::builtins::descriptor_key("name"), name_descriptor),
            ("\0receiver_bound_method".to_string(), Value::Boolean(true)),
        ]),
    }))
}

fn close_on_error(
    iterator: &Value,
    result: &Rc<PromiseData>,
    capability: &Rc<PromiseData>,
    error: VmError,
) {
    let reason = match error {
        VmError::Thrown(reason) => reason,
        _ => Value::Undefined,
    };
    let _ = crate::collections::iterator::close(
        iterator.clone(),
        crate::completion::Completion::Throw(reason.clone()),
    );
    reject_both(
        result,
        capability,
        VmError::Thrown(reason),
    );
}

fn reject_both(result: &Rc<PromiseData>, capability: &Rc<PromiseData>, error: VmError) {
    let reason = match &error {
        VmError::Thrown(reason) => reason.clone(),
        _ => Value::Undefined,
    };
    reject_with_capability(capability, VmError::Thrown(reason.clone()));
    reject_promise(result, reason);
}

fn reject_with_capability(capability: &Rc<PromiseData>, error: VmError) {
    let reason = match error {
        VmError::Thrown(reason) => reason,
        _ => Value::Undefined,
    };
    let reject = capability
        .capability_executor
        .borrow()
        .as_ref()
        .and_then(|executor| executor.reject.borrow().clone());
    if let Some(reject) = reject {
        let _ = crate::functions::execute_target(&reject, &Value::Undefined, &[reason]);
    }
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
                reject_aggregate(aggregate, aggregate_error(aggregate.values.borrow().clone()));
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

fn aggregate_error(values: Vec<Value>) -> Value {
    crate::builtins::set_property(
        crate::builtins::error(Builtin::AggregateError, &[]),
        "errors",
        Value::array(values),
    )
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

fn aggregate_result(aggregate: &PromiseAggregate) -> Value {
    let values = aggregate.values.borrow().clone();
    let Some(keys) = &aggregate.keys else {
        return Value::array(values);
    };
    let mut properties = vec![("\0prototype".to_string(), Value::Null)];
    properties.extend(keys.iter().zip(values).map(|(key, value)| (key.clone(), value)));
    Value::Object(Rc::new(crate::value::ObjectData::new(properties)))
}

fn finish_when_empty(aggregate: &PromiseAggregate) {
    if *aggregate.remaining.borrow() == 0 {
            resolve_aggregate(aggregate, aggregate_result(aggregate));
    }
}

fn finish_empty_aggregate(aggregate: &PromiseAggregate) {
    match aggregate.kind {
        PromiseAggregateKind::All | PromiseAggregateKind::AllSettled => {
            resolve_aggregate(aggregate, aggregate_result(aggregate))
        }
        PromiseAggregateKind::Any => {
            reject_aggregate(aggregate, aggregate_error(aggregate.values.borrow().clone()))
        }
        PromiseAggregateKind::Race => {}
    }
}

fn resolve_aggregate(aggregate: &PromiseAggregate, value: Value) {
    let mut settled = aggregate.settled.borrow_mut();
    if *settled {
        return;
    }
    *settled = true;
    if let Err(VmError::Thrown(reason)) = invoke_capability(&aggregate.capability, true, value.clone()) {
        let _ = invoke_capability(&aggregate.capability, false, reason.clone());
        reject_promise(&aggregate.result, reason);
        return;
    }
    resolve_promise(&aggregate.result, value);
}

fn reject_aggregate(aggregate: &PromiseAggregate, value: Value) {
    let mut settled = aggregate.settled.borrow_mut();
    if *settled {
        return;
    }
    *settled = true;
    let _ = invoke_capability(&aggregate.capability, false, value.clone());
    reject_promise(&aggregate.result, value);
}

fn invoke_capability(capability: &Rc<PromiseData>, fulfilled: bool, value: Value) -> Result<(), VmError> {
    let callback = capability
        .capability_executor
        .borrow()
        .as_ref()
        .and_then(|executor| {
            if fulfilled {
                executor.resolve.borrow().clone()
            } else {
                executor.reject.borrow().clone()
            }
        });
    let Some(callback) = callback.filter(crate::conversion::is_callable) else {
        return Ok(());
    };
    crate::functions::execute_target(&callback, &Value::Undefined, &[value]).map(|_| ())
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
