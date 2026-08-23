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
    let accumulator = match arguments.get(1) {
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
    reduce_loop(callback, iterator, accumulator)
}

fn reduce_loop(
    callback: Value,
    iterator: Value,
    mut accumulator: Value,
) -> Result<Value, crate::execute::VmError> {
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
    find_in_iterator(receiver, &callback)
}

fn find_in_iterator(
    receiver: &Value,
    callback: &Value,
) -> Result<Value, crate::execute::VmError> {
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
            callback,
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



