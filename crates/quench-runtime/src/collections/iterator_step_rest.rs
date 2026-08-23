fn mapped_step(
    iterator: &Value,
    mapper: &Value,
    index: &mut usize,
    done: &mut bool,
) -> Result<Option<Value>, crate::execute::VmError> {
    if *done {
        return Ok(None);
    }
    let value = match super::step_value(iterator) {
        Ok(Some(value)) => value,
        Ok(None) => {
            *done = true;
            return Ok(None);
        }
        Err(error) => return Err(close_mapped_error(iterator, error)),
    };
    let result = match crate::functions::execute_target(
        mapper,
        &Value::Undefined,
        &[value, Value::Number(*index as f64), iterator.clone()],
    ) {
        Ok(result) => result,
        Err(error) => return Err(close_mapped_error(iterator, error)),
    };
    *index += 1;
    Ok(Some(result))
}

fn filtered_step(
    iterator: &Value,
    predicate: &Value,
    index: &mut usize,
    done: &mut bool,
) -> Result<Option<Value>, crate::execute::VmError> {
    loop {
        let Some(value) =
            super::step_value(iterator).map_err(|e| close_mapped_error(iterator, e))?
        else {
            *done = true;
            return Ok(None);
        };
        let result = crate::functions::execute_target(
            predicate,
            &Value::Undefined,
            &[
                value.clone(),
                Value::Number(*index as f64),
                iterator.clone(),
            ],
        )
        .map_err(|e| close_mapped_error(iterator, e))?;
        *index += 1;
        if crate::execute::is_truthy(&result) {
            return Ok(Some(value));
        }
    }
}

fn flat_mapped_step(
    inner: &Value,
    mapper: &Value,
    index: &mut usize,
    current: &mut Option<Value>,
    done: &mut bool,
) -> Result<Option<Value>, crate::execute::VmError> {
    if *done {
        return Ok(None);
    }
    loop {
        if let Some(iter) = current.clone() {
            match super::step_value(&iter) {
                Ok(Some(value)) => return Ok(Some(value)),
                Ok(None) => {
                    *current = None;
                }
                Err(error) => {
                    *done = true;
                    return Err(error);
                }
            }
        }
        let value = match super::step_value(inner) {
            Ok(Some(value)) => value,
            Ok(None) => {
                *done = true;
                return Ok(None);
            }
            Err(error) => {
                *done = true;
                return Err(error);
            }
        };
        let result = match crate::functions::execute_target(
            mapper,
            &Value::Undefined,
            &[value, Value::Number(*index as f64), inner.clone()],
        ) {
            Ok(value) => value,
            Err(error) => {
                *done = true;
                return Err(error);
            }
        };
        *index += 1;
        if let Some(iter) = open_iterator(result)? {
            *current = Some(iter);
        }
    }
}

fn dropped_step(
    inner: &Value,
    skipped: &mut usize,
    limit: usize,
    done: &mut bool,
) -> Result<Option<Value>, crate::execute::VmError> {
    if *done {
        return Ok(None);
    }
    while *skipped < limit {
        match super::step_value(inner) {
            Ok(Some(_)) => *skipped += 1,
            Ok(None) => {
                *done = true;
                return Ok(None);
            }
            Err(error) => return Err(error),
        }
    }
    match super::step_value(inner) {
        Ok(Some(value)) => Ok(Some(value)),
        Ok(None) => {
            *done = true;
            Ok(None)
        }
        Err(error) => Err(error),
    }
}
fn take_step(inner: &Value, remaining: &mut u64) -> Result<Option<Value>, crate::execute::VmError> {
    if *remaining == 0 {
        return Ok(None);
    }
    let value = match super::step_value(inner) {
        Ok(Some(value)) => value,
        Ok(None) => return Ok(None),
        Err(error) => return Err(error),
    };
    *remaining -= 1;
    Ok(Some(value))
}

fn open_iterator(value: Value) -> Result<Option<Value>, crate::execute::VmError> {
    if !crate::value::is_object(&value) {
        return Err(crate::value::error::throw_type_error(
            "Iterator.prototype.flatMap mapper result is not iterable",
        ));
    }
    if matches!(value, Value::Iterator(_) | Value::Generator(_)) {
        return Ok(Some(value));
    }
    let iter = super::open(value)?;
    Ok(Some(iter))
}

fn close_mapped_error(iterator: &Value, error: crate::execute::VmError) -> crate::execute::VmError {
    let completion = match crate::completion::Completion::from_vm_error(error.clone()) {
        Ok(completion) => completion,
        Err(_) => return error,
    };
    match super::close(iterator.clone(), completion) {
        Ok(completion) => match completion.into_vm_error() {
            Err(error) => error,
            Ok(_) => error,
        },
        Err(close_error) => close_error,
    }
}

fn step_target_tail(state: &mut IteratorState) -> Result<StepTarget, crate::execute::VmError> {
    Ok(match state {
        IteratorState::RegExpString {
            regexp,
            input,
            global,
            unicode,
            done,
        } if !*done => {
            return crate::regexp::iterator_step(regexp, input, *global, *unicode, done)
                .map(StepTarget::Value)
        }
        IteratorState::RegExpString { .. } | IteratorState::Protocol { done: true, .. } => {
            StepTarget::Value(None)
        }
        IteratorState::Protocol { iterator, next, .. } => {
            StepTarget::Protocol(iterator.clone(), next.clone())
        }
        _ => StepTarget::Value(None),
    })
}

fn string_step(input: &[u16], index: &mut usize, done: &mut bool) -> Option<Value> {
    if *done || *index >= input.len() {
        *done = true;
        return None;
    }
    let start = *index;
    *index += if *index + 1 < input.len()
        && (0xD800..0xDC00).contains(&input[*index])
        && (0xDC00..0xE000).contains(&input[*index + 1])
    {
        2
    } else {
        1
    };
    Some(crate::strings::from_units(input[start..*index].to_vec()))
}

fn resolve_next(
    data: &IteratorData,
    iterator: &Value,
    cached: Value,
) -> Result<Value, crate::execute::VmError> {
    if crate::conversion::is_callable(&cached) {
        return Ok(cached);
    }
    let next = crate::execute::get_property_result(iterator, "next")?;
    if !crate::conversion::is_callable(&next) {
        return Err(not_iterable());
    }
    if let IteratorState::Protocol {
        next: slot,
        done: false,
        ..
    } = &mut *data.state.borrow_mut()
    {
        *slot = next.clone();
    }
    Ok(next)
}

fn set_step(
    data: &crate::value::SetData,
    index: &mut usize,
    kind: u8,
    done: &mut bool,
) -> Option<Value> {
    if *done {
        return None;
    }
    let value = data.values.borrow().get(*index).cloned();
    if value.is_none() {
        *done = true;
    } else {
        *index += 1;
    }
    value.map(|value| {
        if kind == 1 {
            Value::array(vec![value.clone(), value])
        } else {
            value
        }
    })
}

fn protocol_result(
    data: &IteratorData,
    result: Value,
) -> Result<Option<Value>, crate::execute::VmError> {
    if !crate::value::is_object(&result) {
        return Err(not_iterable());
    }
    let done = crate::execute::get_property_result(&result, "done")?;
    if crate::execute::is_truthy(&done) {
        mark_done(data);
        return Ok(None);
    }
    crate::execute::get_property_result(&result, "value").map(Some)
}
