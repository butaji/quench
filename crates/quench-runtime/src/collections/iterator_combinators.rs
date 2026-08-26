fn collect_zip_iterators(inputs: Value) -> Result<Vec<Value>, crate::execute::VmError> {
    let outer = open(inputs)?;
    let mut iterators = Vec::new();
    loop {
        match step_value(&outer) {
            Ok(Some(value)) => match zip_flattenable(value) {
                Ok(iterator) => iterators.push(iterator),
                Err(error) => return Err(close_zip_inputs(outer, iterators, error)),
            },
            Ok(None) => return Ok(iterators),
            Err(error) => return Err(close_zip_iterators(iterators, error)),
        }
    }
}

fn zip_flattenable(value: Value) -> Result<Value, crate::execute::VmError> {
    if matches!(value, Value::String(_) | Value::StringUnits(_)) {
        return Err(crate::value::error::throw_type_error(
            "Iterator.zip rejects string iterables",
        ));
    }
    from(std::slice::from_ref(&value))
}

fn close_zip_inputs(
    outer: Value,
    iterators: Vec<Value>,
    error: crate::execute::VmError,
) -> crate::execute::VmError {
    let completion = match crate::completion::Completion::from_vm_error(error.clone()) {
        Ok(completion) => completion,
        Err(_) => return error,
    };
    let completion = match close_iterators(iterators, completion) {
        Ok(completion) => completion,
        Err(error) => return error,
    };
    match close(outer, completion) {
        Ok(completion) => match completion.into_vm_error() {
            Err(error) => error,
            Ok(_) => error,
        },
        Err(error) => error,
    }
}

fn close_zip_iterators(
    iterators: Vec<Value>,
    error: crate::execute::VmError,
) -> crate::execute::VmError {
    let completion = match crate::completion::Completion::from_vm_error(error.clone()) {
        Ok(completion) => completion,
        Err(_) => return error,
    };
    let completion = match close_iterators(iterators, completion) {
        Ok(completion) => completion,
        Err(error) => return error,
    };
    match completion.into_vm_error() {
        Err(error) => error,
        Ok(_) => error,
    }
}

fn zip_mode(options: &Value) -> Result<u8, crate::execute::VmError> {
    if matches!(options, Value::Undefined) {
        return Ok(0);
    }
    if !crate::value::is_object(options) {
        return Err(crate::value::error::throw_type_error(
            "Iterator.zip options",
        ));
    }
    let mode = crate::execute::get_property_result(options, "mode")?;
    let mode = match mode {
        Value::Undefined => 0,
        Value::String(value) if value == "shortest" => 0,
        Value::String(value) if value == "longest" => 1,
        Value::String(value) if value == "strict" => 2,
        _ => return Err(crate::value::error::throw_type_error("Iterator.zip mode")),
    };
    if mode == 1 {
        let padding = crate::execute::get_property_result(options, "padding")?;
        if !matches!(padding, Value::Undefined) && !crate::value::is_object(&padding) {
            return Err(crate::value::error::throw_type_error(
                "Iterator.zip padding",
            ));
        }
    }
    Ok(mode)
}

pub(crate) fn from(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let value = arguments.first().cloned().unwrap_or(Value::Undefined);
    if matches!(value, Value::Iterator(_) | Value::Generator(_)) {
        return Ok(value);
    }
    if !crate::value::is_object(&value) {
        return open(value);
    }
    let method = crate::execute::get_property_result(&value, "Symbol.iterator")?;
    if matches!(method, Value::Null | Value::Undefined) {
        let next = crate::execute::get_property_result(&value, "next")?;
        let next = if crate::conversion::is_callable(&next) {
            next
        } else {
            Value::Undefined
        };
        return Ok(make_protocol_with_next(value, next));
    }
    open(value)
}

fn receiver_iterator(receiver: &Value) -> Result<Value, crate::execute::VmError> {
    if matches!(receiver, Value::Generator(_)) {
        open(receiver.clone())
    } else {
        from(std::slice::from_ref(receiver))
    }
}

pub(crate) fn to_array(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    let receiver = receiver.ok_or_else(not_iterable)?;
    let iterator = receiver_iterator(receiver)?;
    Ok(Value::array(collect_rest(&iterator)?))
}

pub(crate) fn map(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let receiver = receiver.ok_or_else(not_iterable)?;
    let mapper = arguments.first().cloned().unwrap_or(Value::Undefined);
    if !crate::conversion::is_callable(&mapper) {
        return Err(crate::value::error::throw_type_error(
            "Iterator.prototype.map mapper is not callable",
        ));
    }
    let iterator = receiver_iterator(receiver)?;
    Ok(Value::Iterator(Rc::new(IteratorData::new(
        IteratorState::Mapped {
            iterator,
            mapper,
            index: 0,
            done: false,
        },
    ))))
}

pub(crate) fn filter(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let receiver = receiver.ok_or_else(not_iterable)?;
    let predicate = arguments.first().cloned().unwrap_or(Value::Undefined);
    if !crate::conversion::is_callable(&predicate) {
        return Err(crate::value::error::throw_type_error(
            "Iterator.prototype.filter predicate is not callable",
        ));
    }
    let iterator = receiver_iterator(receiver)?;
    Ok(Value::Iterator(Rc::new(IteratorData::new(
        IteratorState::Filtered {
            iterator,
            predicate,
            index: 0,
            done: false,
        },
    ))))
}

pub(crate) fn some(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    predicate_terminal(receiver, arguments, true)
}

pub(crate) fn every(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    predicate_terminal(receiver, arguments, false)
}

fn predicate_terminal(
    receiver: Option<&Value>,
    arguments: &[Value],
    stop_when_truthy: bool,
) -> Result<Value, crate::execute::VmError> {
    let receiver = receiver.ok_or_else(not_iterable)?;
    let iterator = receiver_iterator(receiver)?;
    let callback = arguments.first().ok_or_else(crate::vm::not_callable)?;
    if !crate::conversion::is_callable(callback) {
        return Err(crate::vm::not_callable());
    }
    let mut index = 0;
    loop {
        let Some(value) = step_value(&iterator)? else {
            return Ok(Value::Boolean(!stop_when_truthy));
        };
        let result = crate::functions::execute_target(
            callback,
            &Value::Undefined,
            &[value, Value::Number(index as f64), iterator.clone()],
        );
        match result {
            Ok(result) if crate::execute::is_truthy(&result) == stop_when_truthy => {
                let completion = close(iterator, crate::completion::Completion::Normal)?;
                return if matches!(completion, crate::completion::Completion::Normal) {
                    Ok(Value::Boolean(stop_when_truthy))
                } else {
                    completion.into_vm_error()
                };
            }
            Ok(_) => index += 1,
            Err(error) => {
                let completion = crate::completion::Completion::from_vm_error(error)?;
                return close(iterator, completion)
                    .and_then(|completion| completion.into_vm_error());
            }
        }
    }
}

pub(crate) fn next_string(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    let branded = matches!(receiver, Some(Value::Iterator(data)) if matches!(
        &*data.state.borrow(), IteratorState::String { .. }
    ));
    if !branded {
        return Err(crate::value::error::throw_type_error(
            "String iterator called on incompatible receiver",
        ));
    }
    next(receiver)
}
pub(crate) fn return_iterator(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let Some(Value::Iterator(data)) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "Iterator return called on incompatible receiver",
        ));
    };
    let value = arguments.first().cloned().unwrap_or(Value::Undefined);
    let (already_done, inner) = inner_iterators(data);
    if already_done {
        return Ok(result(value, true));
    }
    mark_done(data);
    *data.in_return.borrow_mut() = true;
    let completion = close_inner(data, inner, receiver)?;
    *data.in_return.borrow_mut() = false;
    match completion {
        crate::completion::Completion::Normal => Ok(result(value, true)),
        completion => completion.into_vm_error().map(|_| result(value, true)),
    }
}

/// Walk the iterator state and return a tuple of `(already_done, inner)`
/// where `inner` is the list of opened inner iterators that must be closed
/// before the receiver itself is reported as returned.
fn inner_iterators(data: &IteratorData) -> (bool, Vec<Value>) {
    let state = data.state.borrow();
    match &*state {
        IteratorState::Zip { done: true, .. } | IteratorState::Zip { .. } => {
            let iterators = zip_iterators(data).unwrap_or_default();
            let already_done = matches!(&*data.state.borrow(), IteratorState::Zip { done: true, .. });
            (already_done, iterators)
        }
        IteratorState::Concat { opened, done, .. } => {
            if *done {
                (true, Vec::new())
            } else {
                let inner = opened
                    .iter()
                    .filter_map(|slot| slot.clone())
                    .collect();
                (false, inner)
            }
        }
        _ => (false, Vec::new()),
    }
}

fn close_inner(
    data: &IteratorData,
    inner: Vec<Value>,
    receiver: Option<&Value>,
) -> Result<crate::completion::Completion, crate::execute::VmError> {
    if !inner.is_empty() {
        return close_iterators(inner, crate::completion::Completion::Normal);
    }
    if matches!(
        &*data.state.borrow(),
        IteratorState::Zip { .. }
    ) {
        if let Some(iterators) = zip_iterators(data) {
            return close_iterators(iterators, crate::completion::Completion::Normal);
        }
    }
    close(
        receiver.cloned().unwrap_or(Value::Undefined),
        crate::completion::Completion::Normal,
    )
}

pub(crate) fn flat_map(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let receiver = receiver.ok_or_else(not_iterable)?;
    let mapper = arguments.first().cloned().unwrap_or(Value::Undefined);
    if !crate::conversion::is_callable(&mapper) {
        return Err(crate::value::error::throw_type_error(
            "Iterator.prototype.flatMap mapper is not callable",
        ));
    }
    let inner = receiver_iterator(receiver)?;
    let outer = Value::Iterator(Rc::new(IteratorData::new(
        IteratorState::FlatMapped {
            inner,
            mapper,
            index: 0,
            current: None,
            done: false,
        },
    )));
    Ok(outer)
}

pub(crate) fn drop(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let receiver = receiver.ok_or_else(not_iterable)?;
    let limit = arguments
        .first()
        .cloned()
        .unwrap_or(Value::Undefined);
    let limit = crate::conversion::to_number(&limit)?.trunc();
    let n = if limit.is_nan() || limit < 0.0 { 0.0 } else { limit };
    let n = n.clamp(0.0, 9_007_199_254_740_991.0) as usize;
    let inner = receiver_iterator(receiver)?;
    Ok(Value::Iterator(Rc::new(IteratorData::new(
        IteratorState::Dropped {
            inner,
            skipped: 0,
            limit: n,
            done: false,
        },
    ))))
}

pub(crate) fn take(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let receiver = receiver.ok_or_else(not_iterable)?;
    let limit = arguments
        .first()
        .cloned()
        .unwrap_or(Value::Undefined);
    let limit = crate::conversion::to_number(&limit)?.trunc();
    let n = if limit.is_nan() || limit < 0.0 { 0.0 } else { limit };
    let n = n.clamp(0.0, 9_007_199_254_740_991.0) as u64;
    let inner = receiver_iterator(receiver)?;
    Ok(Value::Iterator(Rc::new(IteratorData::new(
        IteratorState::Take { inner, remaining: n },
    ))))
}

pub(crate) fn reduce(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let receiver = receiver.ok_or_else(not_iterable)?;
    let callback = arguments
        .first()
        .cloned()
        .ok_or_else(crate::vm::not_callable)?;
    if !crate::conversion::is_callable(&callback) {
        return Err(crate::vm::not_callable());
    }
    let iterator = receiver_iterator(receiver)?;
    let mut accumulator = match arguments.get(1) {
        Some(value) => value.clone(),
        None => match step_value(&iterator)? {
            Some(value) => value,
            None => {
                return Err(crate::value::error::throw_type_error(
                    "Iterator.prototype.reduce called on empty iterator with no initial value",
                ));
            }
        },
    };
    let mut index = 0;
    loop {
        let value = match step_value(&iterator) {
            Ok(Some(value)) => value,
            Ok(None) => return Ok(accumulator),
            Err(error) => {
                let _ = close(iterator.clone(), crate::completion::Completion::Normal);
                return Err(error);
            }
        };
        let result = crate::functions::execute_target(
            &callback,
            &Value::Undefined,
            &[accumulator, value, Value::Number(index as f64), iterator.clone()],
        );
        match result {
            Ok(value) => {
                accumulator = value;
                index += 1;
            }
            Err(error) => {
                let _ = close(iterator.clone(), crate::completion::Completion::Normal);
                return Err(error);
            }
        }
    }
}

pub(crate) fn find(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let receiver = receiver.ok_or_else(not_iterable)?;
    let callback = arguments
        .first()
        .cloned()
        .unwrap_or(Value::Undefined);
    if !crate::conversion::is_callable(&callback) {
        return Err(crate::value::error::throw_type_error(
            "Iterator.prototype.find predicate is not callable",
        ));
    }
    let iterator = receiver_iterator(receiver)?;
    let mut index = 0;
    loop {
        let value = match step_value(&iterator) {
            Ok(Some(value)) => value,
            Ok(None) => return Ok(Value::Undefined),
            Err(error) => {
                let _ = close(iterator.clone(), crate::completion::Completion::Normal);
                return Err(error);
            }
        };
        let result = crate::functions::execute_target(
            &callback,
            &Value::Undefined,
            &[value.clone(), Value::Number(index as f64), iterator.clone()],
        );
        let matched = match result {
            Ok(value) => crate::execute::is_truthy(&value),
            Err(error) => {
                let _ = close(iterator.clone(), crate::completion::Completion::Normal);
                return Err(error);
            }
        };
        if matched {
            let _ = close(iterator.clone(), crate::completion::Completion::Normal);
            return Ok(value);
        }
        index += 1;
    }
}

pub(crate) fn for_each(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let receiver = receiver.ok_or_else(not_iterable)?;
    let callback = arguments
        .first()
        .cloned()
        .unwrap_or(Value::Undefined);
    if !crate::conversion::is_callable(&callback) {
        return Err(crate::value::error::throw_type_error(
            "Iterator.prototype.forEach callback is not callable",
        ));
    }
    let iterator = receiver_iterator(receiver)?;
    let mut index = 0;
    loop {
        let value = match step_value(&iterator) {
            Ok(Some(value)) => value,
            Ok(None) => return Ok(Value::Undefined),
            Err(error) => {
                let _ = close(iterator.clone(), crate::completion::Completion::Normal);
                return Err(error);
            }
        };
        let result = crate::functions::execute_target(
            &callback,
            &Value::Undefined,
            &[value, Value::Number(index as f64), iterator.clone()],
        );
        if let Err(error) = result {
            let _ = close(iterator.clone(), crate::completion::Completion::Normal);
            return Err(error);
        }
        index += 1;
    }
}


