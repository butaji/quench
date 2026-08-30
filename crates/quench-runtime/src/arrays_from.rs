pub(crate) fn from(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    from_impl(receiver, arguments, false)
}

pub(crate) fn from_async(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    match from_async_inner(receiver, arguments) {
        Ok(value) => Ok(value),
        Err(crate::execute::VmError::Thrown(reason)) => Ok(rejected_promise(reason)),
        Err(crate::execute::VmError::NotCallable) => {
            if let crate::execute::VmError::Thrown(reason) = crate::value::error::throw_type_error(
                "Array.fromAsync iterator method is not callable",
            ) {
                return Ok(rejected_promise(reason));
            }
            unreachable!()
        }
        Err(error) => Err(error),
    }
}

fn rejected_promise(reason: Value) -> Value {
    let promise = match crate::promise::new_promise() {
        Value::Promise(promise) => promise,
        _ => unreachable!(),
    };
    crate::promise::reject_promise(&promise, reason);
    Value::Promise(promise)
}

fn from_async_inner(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let source = arguments.first().cloned().unwrap_or(Value::Undefined);
    reject_source(&source)?;
    let async_method = crate::execute::get_property_result(&source, "Symbol.asyncIterator")?;
    if !matches!(async_method, Value::Undefined | Value::Null)
        && !crate::conversion::is_callable(&async_method)
    {
        return Err(crate::value::error::throw_type_error(
            "Array.fromAsync async iterator method is not callable",
        ));
    }
    if crate::conversion::is_callable(&async_method) {
        let method = async_method;
        let iterator = crate::execute::call(&method, &source, &[])?;
        let result = match crate::promise::new_promise() {
            Value::Promise(promise) => promise,
            _ => unreachable!(),
        };
        let state = (
            Rc::clone(&result),
            iterator,
            receiver.cloned(),
            mapper(arguments)?,
            arguments.get(2).cloned().unwrap_or(Value::Undefined),
            Vec::new(),
            0,
            None,
            crate::value::ArrayFromAsyncPending::None,
            initial_async_target(receiver)?,
        );
        start_async_step(state);
        return Ok(Value::Promise(result));
    }
    let sync_method = crate::execute::get_property_result(&source, "Symbol.iterator")?;
    if !matches!(sync_method, Value::Undefined | Value::Null) {
        let iterator = crate::collections::iterator::open_with_method(&source, sync_method)?;
        let result = match crate::promise::new_promise() {
            Value::Promise(promise) => promise,
            _ => unreachable!(),
        };
        let state = (
            Rc::clone(&result),
            iterator,
            receiver.cloned(),
            mapper(arguments)?,
            arguments.get(2).cloned().unwrap_or(Value::Undefined),
            Vec::new(),
            0,
            None,
            crate::value::ArrayFromAsyncPending::None,
            initial_async_target(receiver)?,
        );
        start_async_step(state);
        return Ok(Value::Promise(result));
    }
    let array_like_length = array_like_length(&source)?;
    if !uses_custom_result(receiver) && array_like_length > u32::MAX as usize {
        return Err(crate::value::error::throw_range_error(
            "Array.fromAsync array-like length is out of range",
        ));
    }
    let result = match crate::promise::new_promise() {
        Value::Promise(promise) => promise,
        _ => unreachable!(),
    };
    let state = (
        Rc::clone(&result),
        Value::Undefined,
        receiver.cloned(),
        mapper(arguments)?,
        arguments.get(2).cloned().unwrap_or(Value::Undefined),
        Vec::new(),
        0,
        Some((source, array_like_length)),
        crate::value::ArrayFromAsyncPending::None,
        initial_async_target(receiver)?,
    );
    start_async_step(state);
    Ok(Value::Promise(result))
}

type AsyncFromState = (
    Rc<crate::value::PromiseData>,
    Value,
    Option<Value>,
    Option<Value>,
    Value,
    Vec<Value>,
    usize,
    Option<(Value, usize)>,
    crate::value::ArrayFromAsyncPending,
    Option<Value>,
);

fn start_async_step(state: AsyncFromState) {
    let (result, iterator, receiver, mapper, this_arg, values, index, array_like, pending, target) =
        state;
    if !matches!(pending, crate::value::ArrayFromAsyncPending::None) {
        return;
    }
    if let Some((source, length)) = array_like {
        if index < length {
            let item = match crate::execute::get_property_result(&source, &index.to_string()) {
                Ok(value) => value,
                Err(crate::execute::VmError::Thrown(reason)) => {
                    reject_async(&result, &iterator, reason);
                    return;
                }
                Err(_) => {
                    reject_async(&result, &iterator, Value::Undefined);
                    return;
                }
            };
            process_async_item(
                result,
                iterator,
                receiver,
                mapper,
                this_arg,
                values,
                index,
                Some((source, length)),
                item,
                target,
            );
            return;
        }
        match finish_async_result(receiver.as_ref(), values, target, index) {
            Ok(value) => crate::promise::resolve_promise(&result, value),
            Err(crate::execute::VmError::Thrown(reason)) => {
                reject_async(&result, &iterator, reason)
            }
            Err(_) => reject_async(&result, &iterator, Value::Undefined),
        }
        return;
    }
    let next = match crate::execute::get_property_result(&iterator, "next") {
        Ok(next) if crate::conversion::is_callable(&next) => next,
        Ok(_) => {
            if let crate::execute::VmError::Thrown(reason) =
                crate::value::error::throw_type_error("Async iterator next is not callable")
            {
                reject_async(&result, &iterator, reason);
            }
            return;
        }
        Err(crate::execute::VmError::Thrown(reason)) => {
            reject_async(&result, &iterator, reason);
            return;
        }
        Err(_) => {
            reject_async(&result, &iterator, Value::Undefined);
            return;
        }
    };
    let next_result = match crate::execute::call(&next, &iterator, &[]) {
        Ok(value) => value,
        Err(crate::execute::VmError::Thrown(reason)) => {
            reject_async(&result, &iterator, reason);
            return;
        }
        Err(_) => {
            reject_async(&result, &iterator, Value::Undefined);
            return;
        }
    };
    let continuation = |promise: &Rc<crate::value::PromiseData>| {
        promise.continuations.borrow_mut().push(
            crate::value::PromiseContinuation::ArrayFromAsync {
                result: Rc::clone(&result),
                iterator: iterator.clone(),
                receiver: receiver.clone(),
                mapper: mapper.clone(),
                this_arg: this_arg.clone(),
                values: values.clone(),
                index,
                array_like: array_like.clone(),
                pending: crate::value::ArrayFromAsyncPending::None,
                target: target.clone(),
            },
        );
    };
    match next_result {
        Value::Promise(promise) => match promise.state.borrow().clone() {
            crate::value::PromiseState::Pending => continuation(&promise),
            state => process_async_continuation(
                result, iterator, receiver, mapper, this_arg, values, index, array_like, pending,
                target, &state,
            ),
        },
        value => process_async_value(
            result, iterator, receiver, mapper, this_arg, values, index, array_like, value, target,
        ),
    }
}

pub(crate) fn process_async_continuation(
    result: Rc<crate::value::PromiseData>,
    iterator: Value,
    receiver: Option<Value>,
    mapper: Option<Value>,
    this_arg: Value,
    values: Vec<Value>,
    index: usize,
    array_like: Option<(Value, usize)>,
    pending: crate::value::ArrayFromAsyncPending,
    target: Option<Value>,
    state: &crate::value::PromiseState,
) {
    if matches!(pending, crate::value::ArrayFromAsyncPending::Mapper) {
        match state {
            crate::value::PromiseState::Fulfilled(value) => {
                let mut values = values;
                let mut target = target;
                if let Some(current) = target.take() {
                    target = match write_result_element(current, index, value.clone(), false) {
                        Ok(updated) => Some(updated),
                        Err(crate::execute::VmError::Thrown(reason)) => {
                            reject_async(&result, &iterator, reason);
                            return;
                        }
                        Err(_) => {
                            reject_async(&result, &iterator, Value::Undefined);
                            return;
                        }
                    };
                } else {
                    values.push(value.clone());
                }
                start_async_step((
                    result,
                    iterator,
                    receiver,
                    mapper,
                    this_arg,
                    values,
                    index + 1,
                    array_like,
                    crate::value::ArrayFromAsyncPending::None,
                    target,
                ));
            }
            crate::value::PromiseState::Rejected(reason) => {
                reject_async(&result, &iterator, reason.clone())
            }
            crate::value::PromiseState::Pending => {}
        }
        return;
    }
    if matches!(pending, crate::value::ArrayFromAsyncPending::Input) {
        match state {
            crate::value::PromiseState::Fulfilled(value) => {
                let mapped = match map_item(mapper.as_ref(), &this_arg, value.clone(), index) {
                    Ok(value) => value,
                    Err(crate::execute::VmError::Thrown(reason)) => {
                        reject_async(&result, &iterator, reason);
                        return;
                    }
                    Err(_) => {
                        reject_async(&result, &iterator, Value::Undefined);
                        return;
                    }
                };
                continue_after_map(
                    result, iterator, receiver, mapper, this_arg, values, index, array_like,
                    mapped, target,
                );
            }
            crate::value::PromiseState::Rejected(reason) => {
                reject_async(&result, &iterator, reason.clone())
            }
            crate::value::PromiseState::Pending => {}
        }
        return;
    }
    match state {
        crate::value::PromiseState::Fulfilled(value) => process_async_value(
            result,
            iterator,
            receiver,
            mapper,
            this_arg,
            values,
            index,
            array_like,
            value.clone(),
            target,
        ),
        crate::value::PromiseState::Rejected(reason) => {
            reject_async(&result, &iterator, reason.clone())
        }
        crate::value::PromiseState::Pending => {}
    }
}

fn process_async_value(
    result: Rc<crate::value::PromiseData>,
    iterator: Value,
    receiver: Option<Value>,
    mapper: Option<Value>,
    this_arg: Value,
    values: Vec<Value>,
    index: usize,
    array_like: Option<(Value, usize)>,
    next: Value,
    target: Option<Value>,
) {
    if !crate::value::is_object(&next) {
        if let crate::execute::VmError::Thrown(reason) =
            crate::value::error::throw_type_error("Async iterator result is not an object")
        {
            reject_async(&result, &iterator, reason);
        }
        return;
    }
    let Ok(done) = crate::execute::get_property_result(&next, "done") else {
        reject_async(&result, &iterator, Value::Undefined);
        return;
    };
    if crate::execute::is_truthy(&done) {
        match finish_async_result(receiver.as_ref(), values, target, index) {
            Ok(value) => crate::promise::resolve_promise(&result, value),
            Err(crate::execute::VmError::Thrown(reason)) => {
                reject_async(&result, &iterator, reason)
            }
            Err(_) => reject_async(&result, &iterator, Value::Undefined),
        }
        return;
    }
    let item = match crate::execute::get_property_result(&next, "value") {
        Ok(value) => value,
        Err(crate::execute::VmError::Thrown(reason)) => {
            reject_async(&result, &iterator, reason);
            return;
        }
        Err(_) => {
            reject_async(&result, &iterator, Value::Undefined);
            return;
        }
    };
    process_async_item(
        result, iterator, receiver, mapper, this_arg, values, index, array_like, item, target,
    );
}

fn process_async_item(
    result: Rc<crate::value::PromiseData>,
    iterator: Value,
    receiver: Option<Value>,
    mapper: Option<Value>,
    this_arg: Value,
    values: Vec<Value>,
    index: usize,
    array_like: Option<(Value, usize)>,
    item: Value,
    target: Option<Value>,
) {
    let awaited = match await_mapped(item) {
        Ok(value) => value,
        Err(crate::execute::VmError::Thrown(reason)) => {
            reject_async(&result, &iterator, reason);
            return;
        }
        Err(_) => {
            reject_async(&result, &iterator, Value::Undefined);
            return;
        }
    };
    let Value::Promise(promise) = awaited else {
        let mapped = match map_item(mapper.as_ref(), &this_arg, awaited, index) {
            Ok(value) => value,
            Err(crate::execute::VmError::Thrown(reason)) => {
                reject_async(&result, &iterator, reason);
                return;
            }
            Err(_) => {
                reject_async(&result, &iterator, Value::Undefined);
                return;
            }
        };
        continue_after_map(
            result, iterator, receiver, mapper, this_arg, values, index, array_like, mapped, target,
        );
        return;
    };
    let state = promise.state.borrow().clone();
    if matches!(state, crate::value::PromiseState::Pending) {
        promise.continuations.borrow_mut().push(
            crate::value::PromiseContinuation::ArrayFromAsync {
                result,
                iterator,
                receiver,
                mapper,
                this_arg,
                values,
                index,
                array_like,
                pending: crate::value::ArrayFromAsyncPending::Input,
                target,
            },
        );
    } else {
        process_async_continuation(
            result,
            iterator,
            receiver,
            mapper,
            this_arg,
            values,
            index,
            array_like,
            crate::value::ArrayFromAsyncPending::Input,
            target,
            &state,
        );
    }
}

fn initial_async_target(
    receiver: Option<&Value>,
) -> Result<Option<Value>, crate::execute::VmError> {
    uses_custom_result(receiver)
        .then(|| construct_result(receiver, 0, true))
        .transpose()
}

fn finish_async_result(
    receiver: Option<&Value>,
    values: Vec<Value>,
    target: Option<Value>,
    length: usize,
) -> Result<Value, crate::execute::VmError> {
    let Some(target) = target else {
        return create_result(receiver, values, true);
    };
    if crate::typed_array_ops::is_view(&target) {
        return Ok(target);
    }
    set_result_length(target, length)
}

fn reject_async(result: &Rc<crate::value::PromiseData>, iterator: &Value, reason: Value) {
    if !matches!(iterator, Value::Undefined) {
        let _ = crate::collections::iterator::close(
            iterator.clone(),
            crate::completion::Completion::Throw(reason.clone()),
        );
    }
    crate::promise::reject_promise(result, reason);
}

fn continue_after_map(
    result: Rc<crate::value::PromiseData>,
    iterator: Value,
    receiver: Option<Value>,
    mapper: Option<Value>,
    this_arg: Value,
    mut values: Vec<Value>,
    index: usize,
    array_like: Option<(Value, usize)>,
    mapped: Value,
    mut target: Option<Value>,
) {
    let mapped = match await_mapped(mapped) {
        Ok(value) => value,
        Err(crate::execute::VmError::Thrown(reason)) => {
            reject_async(&result, &iterator, reason);
            return;
        }
        Err(_) => {
            reject_async(&result, &iterator, Value::Undefined);
            return;
        }
    };
    let Value::Promise(promise) = mapped else {
        if let Some(current) = target.take() {
            target = match write_result_element(current, index, mapped, false) {
                Ok(updated) => Some(updated),
                Err(crate::execute::VmError::Thrown(reason)) => {
                    reject_async(&result, &iterator, reason);
                    return;
                }
                Err(_) => {
                    reject_async(&result, &iterator, Value::Undefined);
                    return;
                }
            };
        } else {
            values.push(mapped);
        }
        start_async_step((
            result,
            iterator,
            receiver,
            mapper,
            this_arg,
            values,
            index + 1,
            array_like,
            crate::value::ArrayFromAsyncPending::None,
            target,
        ));
        return;
    };
    let state = promise.state.borrow().clone();
    match state {
        crate::value::PromiseState::Fulfilled(value) => {
            if let Some(current) = target.take() {
                target = match write_result_element(current, index, value, false) {
                    Ok(updated) => Some(updated),
                    Err(crate::execute::VmError::Thrown(reason)) => {
                        reject_async(&result, &iterator, reason);
                        return;
                    }
                    Err(_) => {
                        reject_async(&result, &iterator, Value::Undefined);
                        return;
                    }
                };
            } else {
                values.push(value);
            }
            start_async_step((
                result,
                iterator,
                receiver,
                mapper,
                this_arg,
                values,
                index + 1,
                array_like,
                crate::value::ArrayFromAsyncPending::None,
                target,
            ));
        }
        crate::value::PromiseState::Rejected(reason) => reject_async(&result, &iterator, reason),
        crate::value::PromiseState::Pending => {
            promise.continuations.borrow_mut().push(
                crate::value::PromiseContinuation::ArrayFromAsync {
                    result,
                    iterator,
                    receiver,
                    mapper,
                    this_arg,
                    values,
                    index,
                    array_like,
                    pending: crate::value::ArrayFromAsyncPending::Mapper,
                    target,
                },
            );
        }
    }
}

fn await_mapped(mapped: Value) -> Result<Value, crate::execute::VmError> {
    if matches!(mapped, Value::Promise(_)) || !crate::value::is_object(&mapped) {
        return Ok(mapped);
    }
    let then = crate::execute::get_property_result(&mapped, "then")?;
    if crate::conversion::is_callable(&then) {
        Ok(crate::promise::promise_resolve_with_then(mapped, then))
    } else {
        Ok(mapped)
    }
}

pub(crate) fn typed_from(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    from_impl(receiver, arguments, true)
}

fn from_impl(
    receiver: Option<&Value>,
    arguments: &[Value],
    typed_mode: bool,
) -> Result<Value, crate::execute::VmError> {
    let source = arguments.first().cloned().unwrap_or(Value::Undefined);
    reject_source(&source)?;
    let mapper = mapper(arguments)?;
    if typed_mode {
        let method = crate::execute::get_property_result(&source, "Symbol.iterator")?;
        if !matches!(method, Value::Undefined | Value::Null) {
            return from_typed_iterable(receiver, source, mapper.as_ref(), arguments, method);
        }
    }
    let iterable = if typed_mode {
        false
    } else {
        !matches!(source, Value::ArrayBuffer(_) | Value::DataView(_)) && has_iterator(&source)?
    };
    if iterable {
        if is_default_array_iterator(&source)? {
            return from_live_array(receiver, source, mapper.as_ref(), arguments);
        }
        return from_iterable(receiver, source, mapper.as_ref(), arguments);
    }
    if uses_custom_result(receiver) {
        let length = array_like_length(&source)?;
        let this_arg = arguments.get(2).cloned().unwrap_or(Value::Undefined);
        let mut result = if typed_mode {
            construct_typed_result(receiver, length)?
        } else {
            construct_result(receiver, length, false)?
        };
        if typed_array_result_unwritable(&result, length > 0) {
            return Err(crate::value::error::throw_type_error(
                "Cannot set an element on an invalid typed array",
            ));
        }
        validate_initial_result_bounds(&result, length, strict_result_bounds(receiver))?;
        for index in 0..length {
            let item = if let Value::Array(array) = &source {
                array.get_index(index).unwrap_or(Value::Undefined)
            } else {
                crate::execute::get_property_result(&source, &index.to_string())?
            };
            let value = map_item(mapper.as_ref(), &this_arg, item, index)?;
            result = write_result_element(result, index, value, false)?;
        }
        return Ok(result);
    }
    let mut values = Vec::new();
    if let Value::Array(array) = &source {
        collect_array_iterator(array, mapper.as_ref(), arguments, &mut values)?;
    } else {
        collect_array_like(&source, mapper.as_ref(), arguments, &mut values)?;
    }
    create_result(receiver, values, iterable)
}

fn from_typed_iterable(
    receiver: Option<&Value>,
    source: Value,
    mapper: Option<&Value>,
    arguments: &[Value],
    method: Value,
) -> Result<Value, crate::execute::VmError> {
    let mut source_values = Vec::new();
    let iterator = crate::collections::iterator::open_with_method(&source, method)?;
    crate::collections::iterator::for_each_open_iterator(iterator, |item| {
        source_values.push(item);
        Ok(())
    })?;
    let this_arg = arguments.get(2).cloned().unwrap_or(Value::Undefined);
    let mut result = construct_typed_result(receiver, source_values.len())?;
    if typed_array_result_unwritable(&result, !source_values.is_empty()) {
        return Err(crate::value::error::throw_type_error(
            "Cannot set an element on an invalid typed array",
        ));
    }
    validate_initial_result_bounds(&result, source_values.len(), strict_result_bounds(receiver))?;
    for (index, item) in source_values.into_iter().enumerate() {
        let value = map_item(mapper, &this_arg, item, index)?;
        result = write_result_element(result, index, value, false)?;
    }
    Ok(result)
}

pub(crate) fn of(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    if let Some(receiver) = receiver.filter(|value| is_constructor(value)) {
        let mut result = construct_result(Some(receiver), arguments.len(), false)?;
        for (index, value) in arguments.iter().cloned().enumerate() {
            result = write_result_element(result, index, value, false)?;
        }
        return set_result_length(result, arguments.len());
    }
    Ok(Value::array(arguments.to_vec()))
}

pub(crate) fn typed_of(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let Some(receiver) = receiver.filter(|value| is_constructor(value)) else {
        return Err(crate::value::error::throw_type_error(
            "TypedArray.of called on a non-constructor",
        ));
    };
    let mut result = construct_typed_result(Some(receiver), arguments.len())?;
    validate_initial_result_bounds(
        &result,
        arguments.len(),
        strict_result_bounds(Some(receiver)),
    )?;
    for (index, value) in arguments.iter().cloned().enumerate() {
        result = write_result_element(result, index, value, false)?;
    }
    Ok(result)
}

fn is_default_array_iterator(source: &Value) -> Result<bool, crate::execute::VmError> {
    Ok(matches!(source, Value::Array(_))
        && matches!(
            crate::execute::get_property_result(source, "Symbol.iterator")?,
            Value::Builtin(crate::ops::Builtin::ArrayIterator)
        ))
}

fn from_live_array(
    receiver: Option<&Value>,
    source: Value,
    mapper: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let this_arg = arguments.get(2).cloned().unwrap_or(Value::Undefined);
    let mut values = Vec::new();
    let mut index = 0;
    loop {
        let current = crate::locals::resolved_replacement(source.clone());
        let Value::Array(array) = current else { break };
        if index >= array.logical_len() {
            break;
        }
        let item = array.get_index(index).unwrap_or(Value::Undefined);
        let value = map_item(mapper, &this_arg, item, index)?;
        values.push(value);
        index += 1;
    }
    create_result(receiver, values, true)
}

fn from_iterable(
    receiver: Option<&Value>,
    source: Value,
    mapper: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let custom_result = uses_custom_result(receiver);
    let mut result = custom_result
        .then(|| construct_result(receiver, 0, true))
        .transpose()?;
    let mut source_values = Vec::new();
    let mut source_length = 0;
    let this_arg = arguments.get(2).cloned().unwrap_or(Value::Undefined);
    let _receiver_guard = crate::collections::iterator::ReceiverUpdateGuard::install();
    crate::collections::iterator::for_each_iterable(source, |item| {
        let index = source_length;
        source_length += 1;
        let value = map_item(mapper, &this_arg, item, index)?;
        if let Some(target) = result.take() {
            result = Some(write_result_element(target, index, value, false)?);
        } else {
            source_values.push(value);
        }
        Ok(())
    })?;
    source_length = source_length.max(source_values.len());
    if let Some(result) = result {
        if crate::typed_array_ops::is_view(&result) {
            return Ok(result);
        }
        return set_result_length(result, source_length);
    }
    Ok(Value::array(source_values))
}

fn uses_custom_result(receiver: Option<&Value>) -> bool {
    matches!(
        receiver,
        Some(value) if !matches!(value, Value::Null | Value::Undefined)
            && !matches!(value, Value::Builtin(crate::ops::Builtin::Array))
    )
}

fn strict_result_bounds(receiver: Option<&Value>) -> bool {
    receiver.is_some_and(|value| matches!(value, Value::Function(_) | Value::BoundFunction(_)))
}

fn collect_array_iterator(
    array: &crate::value::ArrayData,
    mapper: Option<&Value>,
    arguments: &[Value],
    values: &mut Vec<Value>,
) -> Result<(), crate::execute::VmError> {
    let this_arg = arguments.get(2).cloned().unwrap_or(Value::Undefined);
    let mut index = 0;
    while index < array.logical_len() {
        let item = array.get_index(index).unwrap_or(Value::Undefined);
        values.push(map_item(mapper, &this_arg, item, index)?);
        index += 1;
    }
    Ok(())
}

fn reject_source(source: &Value) -> Result<(), crate::execute::VmError> {
    if matches!(source, Value::Null | Value::Undefined) {
        return Err(crate::value::error::throw_type_error(
            "Array.from requires an array-like object",
        ));
    }
    Ok(())
}

fn mapper(arguments: &[Value]) -> Result<Option<Value>, crate::execute::VmError> {
    let Some(mapper) = arguments.get(1).cloned() else {
        return Ok(None);
    };
    if !crate::conversion::is_callable(&mapper) {
        return Err(crate::value::error::throw_type_error(
            "Array.from mapper must be callable",
        ));
    }
    Ok(Some(mapper))
}

fn has_iterator(source: &Value) -> Result<bool, crate::execute::VmError> {
    let method = crate::execute::get_property_result(source, "Symbol.iterator")?;
    Ok(!matches!(method, Value::Undefined | Value::Null))
}

fn collect_array_like(
    source: &Value,
    mapper: Option<&Value>,
    arguments: &[Value],
    values: &mut Vec<Value>,
) -> Result<(), crate::execute::VmError> {
    let length = array_like_length(source)?;
    let this_arg = arguments.get(2).cloned().unwrap_or(Value::Undefined);
    for index in 0..length {
        let item = crate::execute::get_property_result(source, &index.to_string())?;
        values.push(map_item(mapper, &this_arg, item, index)?);
    }
    Ok(())
}

pub(crate) fn array_like_length(source: &Value) -> Result<usize, crate::execute::VmError> {
    if matches!(source, Value::ArrayBuffer(_) | Value::DataView(_)) {
        return Ok(0);
    }
    let value = crate::execute::get_property_result(source, "length")?;
    let number = crate::conversion::to_number(&value)?;
    if number.is_nan() || number <= 0.0 {
        return Ok(0);
    }
    Ok(number.floor().min(9_007_199_254_740_991.0) as usize)
}

fn map_item(
    mapper: Option<&Value>,
    this_arg: &Value,
    item: Value,
    index: usize,
) -> Result<Value, crate::execute::VmError> {
    let Some(mapper) = mapper else {
        return Ok(item);
    };
    crate::functions::execute_target(mapper, this_arg, &[item, Value::Number(index as f64)])
}

fn create_result(
    receiver: Option<&Value>,
    values: Vec<Value>,
    iterable: bool,
) -> Result<Value, crate::execute::VmError> {
    let length = values.len();
    let plain_array = match receiver {
        None | Some(Value::Null | Value::Undefined) => true,
        Some(value) => matches!(value, Value::Builtin(crate::ops::Builtin::Array)),
    };
    if plain_array {
        return Ok(Value::array(values));
    }
    // Allocate the final typed-array extent before writing elements.  An
    // empty iterable construction cannot grow a typed-array view; assigning
    // its `length` afterward only changes metadata and leaves a zero-byte
    // backing buffer.
    let strict_bounds = strict_result_bounds(receiver);
    let mut result = construct_result(receiver, length, iterable)?;
    validate_initial_result_bounds(&result, length, strict_bounds)?;
    for (index, value) in values.into_iter().enumerate() {
        result = write_result_element(result, index, value, false)?;
    }
    if crate::typed_array_ops::is_view(&result) {
        return Ok(result);
    }
    result = set_result_length(result, length)?;
    Ok(result)
}

fn typed_array_result_unwritable(result: &Value, require_element: bool) -> bool {
    if !crate::typed_array_ops::is_view(result) {
        return false;
    }
    if require_element && crate::typed_array_prototype::is_out_of_bounds(result) {
        return true;
    }
    matches!(result, Value::Float64Array(view) if view.buffer.immutable)
        || matches!(result, Value::Float32Array(view) if view.buffer.immutable)
        || matches!(result, Value::Int8Array(view) if view.buffer.immutable)
        || matches!(result, Value::Int16Array(view) if view.buffer.immutable)
        || matches!(result, Value::Int32Array(view) if view.buffer.immutable)
        || matches!(result, Value::Uint8Array(view) if view.buffer.immutable)
        || matches!(result, Value::Uint8ClampedArray(view) if view.buffer.immutable)
        || matches!(result, Value::Uint16Array(view) if view.buffer.immutable)
        || matches!(result, Value::Uint32Array(view) if view.buffer.immutable)
        || matches!(result, Value::BigInt64Array(view) if view.buffer.immutable)
        || matches!(result, Value::BigUint64Array(view) if view.buffer.immutable)
}

fn set_result_length(result: Value, length: usize) -> Result<Value, crate::execute::VmError> {
    let updated =
        crate::properties::assign_set_property(&result, "length", Value::Number(length as f64))?;
    crate::locals::replace_value(&result, &updated);
    Ok(updated)
}

fn write_result_element(
    result: Value,
    index: usize,
    value: Value,
    strict_bounds: bool,
) -> Result<Value, crate::execute::VmError> {
    let key = index.to_string();
    // Typed-array elements are integer-indexed exotic properties: they are
    // writable but non-configurable, so defining a fresh property would
    // incorrectly trip the ordinary-object read-only check.
    if crate::typed_array_ops::is_view(&result) {
        if typed_array_result_unwritable(&result, false) {
            return Err(crate::value::error::throw_type_error(
                "Cannot set an element on an invalid typed array",
            ));
        }
        if strict_bounds
            && crate::typed_array_ops::logical_len(&result).is_some_and(|length| index >= length)
        {
            return Err(crate::value::error::throw_type_error(
                "Cannot set an element on an out-of-bounds typed array",
            ));
        }
        if let Some(updated) = crate::typed_array_ops::set_property(&result, &key, &value) {
            let updated = updated?;
            crate::locals::replace_value(&result, &updated);
            return Ok(updated);
        }
    }
    let current =
        crate::builtins::object::descriptor(Some(&result), Some(&Value::String(key.clone())))?;
    if !crate::properties::object_is_extensible(&result) && matches!(current, Value::Undefined) {
        return Err(crate::value::error::throw_type_error(
            "Cannot create property on a non-extensible object",
        ));
    }
    let result = if matches!(
        crate::builtins::descriptor_flag(&result, &key, "configurable"),
        Some(true)
    ) {
        let (updated, _) = crate::builtins::delete_property(result, &key);
        updated
    } else {
        result
    };
    let descriptor = vec![
        ("value".to_string(), value),
        ("writable".to_string(), Value::Boolean(true)),
        ("enumerable".to_string(), Value::Boolean(true)),
        ("configurable".to_string(), Value::Boolean(true)),
    ];
    if matches!(result, Value::Proxy(_)) {
        let descriptor = Value::Object(std::rc::Rc::new(crate::value::ObjectData::new(descriptor)));
        crate::builtins::define_property(&[result.clone(), Value::String(key), descriptor])?;
        return Ok(result);
    }
    let updated = crate::builtins::define_own_property(&result, &key, &descriptor)?;
    crate::locals::replace_value(&result, &updated);
    Ok(updated)
}

fn validate_initial_result_bounds(
    result: &Value,
    length: usize,
    strict_bounds: bool,
) -> Result<(), crate::execute::VmError> {
    if strict_bounds
        && crate::typed_array_ops::is_view(result)
        && crate::typed_array_ops::logical_len(result).is_some_and(|actual| actual < length)
    {
        return Err(crate::value::error::throw_type_error(
            "Cannot set an element on an out-of-bounds typed array",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod arrays_from_tests {
    use super::map_item;
    use crate::value::Value;

    #[test]
    fn map_item_without_mapper_returns_item_without_touching_this_arg() {
        let item = Value::String("canonical".into());
        let this_arg = Value::String("receiver".into());

        let result = map_item(None, &this_arg, item.clone(), 0).expect("mapping succeeds");
        assert!(matches!(&result, Value::String(value) if value == "canonical"));
    }
}

fn construct_result(
    receiver: Option<&Value>,
    length: usize,
    iterable: bool,
) -> Result<Value, crate::execute::VmError> {
    construct_result_inner(receiver, length, iterable, false)
}

fn construct_typed_result(
    receiver: Option<&Value>,
    length: usize,
) -> Result<Value, crate::execute::VmError> {
    let result = construct_result_inner(receiver, length, true, true)?;
    if !is_typed_array_result(&result) {
        return Err(crate::value::error::throw_type_error(
            "TypedArray constructor did not return a TypedArray",
        ));
    }
    Ok(result)
}

fn is_typed_array_result(value: &Value) -> bool {
    matches!(
        value,
        Value::Float64Array(_)
            | Value::Float32Array(_)
            | Value::Int8Array(_)
            | Value::Int16Array(_)
            | Value::Int32Array(_)
            | Value::Uint8Array(_)
            | Value::Uint8ClampedArray(_)
            | Value::Uint16Array(_)
            | Value::Uint32Array(_)
            | Value::BigInt64Array(_)
            | Value::BigUint64Array(_)
    )
}

fn construct_result_inner(
    receiver: Option<&Value>,
    length: usize,
    iterable: bool,
    force_length: bool,
) -> Result<Value, crate::execute::VmError> {
    let Some(constructor) = receiver else {
        return Ok(Value::array(Vec::new()));
    };
    if !is_constructor(constructor) {
        return Err(crate::value::error::throw_type_error(
            "TypedArray.from receiver is not a constructor",
        ));
    }
    // Built-in typed arrays must reserve the final extent before iterable
    // elements are written: unlike a custom constructor, their views cannot
    // grow after construction. Custom constructors retain the observable
    // iterable-vs-array-like calling convention.
    let builtin_typed = matches!(
        constructor,
        Value::Builtin(
            crate::ops::Builtin::Float64Array
                | crate::ops::Builtin::Float32Array
                | crate::ops::Builtin::Int8Array
                | crate::ops::Builtin::Int16Array
                | crate::ops::Builtin::Int32Array
                | crate::ops::Builtin::Uint8Array
                | crate::ops::Builtin::Uint16Array
                | crate::ops::Builtin::Uint32Array
                | crate::ops::Builtin::Uint8ClampedArray
                | crate::ops::Builtin::BigInt64Array
                | crate::ops::Builtin::BigUint64Array
        )
    );
    let arguments = if force_length || builtin_typed || !iterable {
        vec![Value::Number(length as f64)]
    } else {
        Vec::new()
    };
    crate::construct::construct_value(constructor, &arguments)
}

fn is_constructor(value: &Value) -> bool {
    match value {
        Value::Function(function) => crate::functions::is_constructible(function),
        Value::BoundFunction(bound) => is_constructor(&bound.target),
        Value::Builtin(crate::ops::Builtin::TypedArray) => true,
        Value::Builtin(builtin) => crate::builtin_meta::constructor_name(*builtin).is_some(),
        Value::Proxy(proxy) => is_constructor(&proxy.target),
        _ => false,
    }
}
