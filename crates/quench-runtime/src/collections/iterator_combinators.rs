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
        let next = crate::conversion::is_callable(&next)
            .then_some(next)
            .unwrap_or(Value::Undefined);
        return Ok(make_protocol_with_next(value, next));
    }
    open(value)
}

pub(crate) fn to_array(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    let receiver = receiver.ok_or_else(not_iterable)?;
    let iterator = if matches!(receiver, Value::Generator(_)) {
        open(receiver.clone())?
    } else {
        from(std::slice::from_ref(receiver))?
    };
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
    let iterator = from(std::slice::from_ref(receiver))?;
    Ok(Value::Iterator(Rc::new(IteratorData {
        state: RefCell::new(IteratorState::Mapped {
            iterator,
            mapper,
            index: 0,
            done: false,
        }),
    })))
}

pub(crate) fn some(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let receiver = receiver.ok_or_else(not_iterable)?;
    let iterator = from(std::slice::from_ref(receiver))?;
    let callback = arguments.first().ok_or_else(crate::vm::not_callable)?;
    if !crate::conversion::is_callable(callback) {
        return Err(crate::vm::not_callable());
    }
    let mut index = 0;
    loop {
        let Some(value) = step_value(&iterator)? else {
            return Ok(Value::Boolean(false));
        };
        let result = crate::functions::execute_target(
            callback,
            &Value::Undefined,
            &[value, Value::Number(index as f64), iterator.clone()],
        );
        match result {
            Ok(result) if crate::execute::is_truthy(&result) => {
                let completion = close(iterator, crate::completion::Completion::Normal)?;
                return match completion.into_vm_error() {
                    Err(error) => Err(error),
                    Ok(_) => Ok(Value::Boolean(true)),
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
    let completion = if let Some(iterators) = zip_iterators(data) {
        close_iterators(iterators, crate::completion::Completion::Normal)?
    } else {
        close(
            receiver.cloned().unwrap_or(Value::Undefined),
            crate::completion::Completion::Normal,
        )?
    };
    mark_done(data);
    match completion {
        crate::completion::Completion::Normal => Ok(result(value, true)),
        completion => completion.into_vm_error().map(|_| result(value, true)),
    }
}


