pub fn delegate_start(value: Value) -> Result<Value, crate::execute::VmError> {
    open(value)
}
pub fn delegate_next(
    record: &Value,
    input: Value,
) -> Result<DelegationResult, crate::execute::VmError> {
    let Value::Iterator(data) = record else {
        return Err(not_iterable());
    };
    let protocol = {
        let mut state = data.state.borrow_mut();
        match &mut *state {
            IteratorState::Native {
                values,
                receiver,
                typed_receiver,
                typed_keys,
                index,
                done,
            } => {
                return native_delegation_step(
                    values,
                    receiver.as_ref(),
                    typed_receiver.as_ref(),
                    *typed_keys,
                    index,
                    done,
                );
            }
            IteratorState::Set { .. }
            | IteratorState::Map { .. }
            | IteratorState::String { .. }
            | IteratorState::RegExpString { .. } => {
                return Ok(DelegationResult::Done(Value::Undefined));
            }
            IteratorState::Protocol { iterator, done, .. } if !*done => Some(iterator.clone()),
            IteratorState::Protocol { .. } => None,
            IteratorState::Mapped { .. } => None,
            IteratorState::Concat { .. } => None,
            IteratorState::Zip { .. } => None,
        }
    };
    let Some(iterator) = protocol else {
        return Ok(DelegationResult::Done(Value::Undefined));
    };
    let next = crate::execute::get_property_result(&iterator, "next")?;
    if !crate::conversion::is_callable(&next) {
        return Err(not_iterable());
    }
    let result = call_with_arguments(&next, &iterator, std::slice::from_ref(&input))?;
    delegation_result(data, result)
}
pub fn delegate_return(
    record: &Value,
    input: Value,
) -> Result<DelegationResult, crate::execute::VmError> {
    let Some((data, iterator)) = delegation_target(record)? else {
        return Ok(DelegationResult::Done(input));
    };
    let Some(method) = get_return_method(&iterator)? else {
        mark_done(data);
        return Ok(DelegationResult::Done(input));
    };
    let result = call_with_arguments(&method, &iterator, std::slice::from_ref(&input))?;
    delegation_result(data, result)
}
pub fn delegate_throw(
    record: &Value,
    input: Value,
) -> Result<DelegationResult, crate::execute::VmError> {
    let Some((data, iterator)) = delegation_target(record)? else {
        return Err(missing_throw_method());
    };
    let Some(method) = get_method(&iterator, "throw")? else {
        close_after_missing_throw(data, &iterator)?;
        return Err(missing_throw_method());
    };
    let result = call_with_arguments(&method, &iterator, std::slice::from_ref(&input))?;
    delegation_result(data, result)
}
fn close_after_missing_throw(
    data: &IteratorData,
    iterator: &Value,
) -> Result<(), crate::execute::VmError> {
    if let Some(method) = get_return_method(iterator)? {
        let _ = call_with_arguments(&method, iterator, &[])?;
    }
    mark_done(data);
    Ok(())
}
fn native_delegation_step(
    values: &[Value],
    receiver: Option<&Rc<crate::value::ArrayData>>,
    typed_receiver: Option<&Value>,
    typed_keys: bool,
    index: &mut usize,
    done: &mut bool,
) -> Result<DelegationResult, crate::execute::VmError> {
    let value = native_step(values, receiver, typed_receiver, typed_keys, index, done)?
        .unwrap_or(Value::Undefined);
    if *done {
        Ok(DelegationResult::Done(value))
    } else {
        Ok(DelegationResult::Ongoing {
            value,
            passthrough: false,
        })
    }
}
fn delegation_target(
    record: &Value,
) -> Result<Option<(&IteratorData, Value)>, crate::execute::VmError> {
    let Value::Iterator(data) = record else {
        return Err(not_iterable());
    };
    let iterator = match &*data.state.borrow() {
        IteratorState::Protocol { iterator, .. } => Some(iterator.clone()),
        IteratorState::Native { .. }
        | IteratorState::Set { .. }
        | IteratorState::Map { .. }
        | IteratorState::String { .. }
        | IteratorState::RegExpString { .. } => None,
        IteratorState::Mapped { .. } => None,
        IteratorState::Concat { .. } => None,
        IteratorState::Zip { .. } => None,
    };
    Ok(iterator.map(|iterator| (data.as_ref(), iterator)))
}
fn delegation_result(
    data: &IteratorData,
    result: Value,
) -> Result<DelegationResult, crate::execute::VmError> {
    if !crate::value::is_object(&result) {
        return Err(not_iterable());
    }
    let done = crate::execute::is_truthy(&crate::execute::get_property_result(&result, "done")?);
    if !done {
        return Ok(DelegationResult::Ongoing {
            value: result,
            passthrough: true,
        });
    }
    let value = crate::execute::get_property_result(&result, "value")?;
    mark_done(data);
    Ok(DelegationResult::Done(value))
}

