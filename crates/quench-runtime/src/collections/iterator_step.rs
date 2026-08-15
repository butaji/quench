//! Iterator stepping — one observable `next` fetch per protocol iterator (GetIterator semantics).

use super::{
    close, close_iterators, iterator_map, iterator_protocol, mark_done, native_step, not_iterable,
};
use crate::value::{IteratorData, IteratorState, Value};

pub(crate) fn step_value(value: &Value) -> Result<Option<Value>, crate::execute::VmError> {
    let Value::Iterator(data) = value else {
        return Err(not_iterable());
    };
    match step_target(data)? {
        StepTarget::Value(value) => Ok(value),
        StepTarget::Protocol(iterator, cached) => {
            let next = resolve_next(data, &iterator, cached)?;
            let result = iterator_protocol::call_next(data, &next, &iterator)?;
            protocol_result(data, result)
        }
    }
}

enum StepTarget {
    Value(Option<Value>),
    Protocol(Value, Value),
}

fn step_target(data: &IteratorData) -> Result<StepTarget, crate::execute::VmError> {
    if matches!(&*data.state.borrow(), IteratorState::Concat { .. }) {
        return concat_target(data);
    }
    let mut state = data.state.borrow_mut();
    step_target_state(data, &mut state)
}

fn concat_target(data: &IteratorData) -> Result<StepTarget, crate::execute::VmError> {
    let (items, mut index, mut current, done) = {
        let state = data.state.borrow();
        let IteratorState::Concat {
            items,
            index,
            current,
            done,
        } = &*state
        else {
            return Ok(StepTarget::Value(None));
        };
        (items.clone(), *index, current.clone(), *done)
    };
    if done {
        return Ok(StepTarget::Value(None));
    }
    loop {
        if let Some(iterator) = current.clone() {
            if let Some(value) = super::step_value(&iterator)? {
                update_concat(data, index, current, done);
                return Ok(StepTarget::Value(Some(value)));
            }
            current = None;
            index += 1;
        }
        let Some((receiver, method)) = items.get(index).cloned() else {
            update_concat(data, index, current, true);
            return Ok(StepTarget::Value(None));
        };
        let iterator = crate::functions::execute_target(&method, &receiver, &[])?;
        if !crate::value::is_object(&iterator) {
            return Err(not_iterable());
        }
        current = Some(if matches!(iterator, Value::Iterator(_)) {
            iterator
        } else {
            super::make_protocol(iterator)
        });
    }
}

fn update_concat(data: &IteratorData, index: usize, current: Option<Value>, done: bool) {
    if let IteratorState::Concat {
        index: state_index,
        current: state_current,
        done: state_done,
        ..
    } = &mut *data.state.borrow_mut()
    {
        *state_index = index;
        *state_current = current;
        *state_done = done;
    }
}

fn step_target_state(
    _data: &IteratorData,
    state: &mut IteratorState,
) -> Result<StepTarget, crate::execute::VmError> {
    Ok(match state {
        IteratorState::Native {
            values,
            receiver,
            typed_receiver,
            typed_keys,
            index,
            done,
        } => StepTarget::Value(native_step(
            values,
            receiver.as_ref(),
            typed_receiver.as_ref(),
            *typed_keys,
            index,
            done,
        )?),
        IteratorState::String { input, index, done } => {
            StepTarget::Value(string_step(input, index, done))
        }
        IteratorState::Set {
            data,
            index,
            kind,
            done,
        } => StepTarget::Value(set_step(data, index, *kind, done)),
        IteratorState::Map {
            data,
            index,
            kind,
            done,
        } => StepTarget::Value(iterator_map::step(data, index, kind, done)),
        IteratorState::Mapped {
            iterator,
            mapper,
            index,
            done,
        } => StepTarget::Value(mapped_step(iterator, mapper, index, done)?),
        IteratorState::Zip {
            iterators,
            mode,
            done,
        } => StepTarget::Value(zip_step(iterators, *mode, done)?),
        state => return step_target_tail(state),
    })
}

fn zip_step(
    iterators: &[Value],
    mode: u8,
    done: &mut bool,
) -> Result<Option<Value>, crate::execute::VmError> {
    if *done {
        return Ok(None);
    }
    let mut values = Vec::with_capacity(iterators.len());
    let mut ended = 0;
    let mut open = vec![true; iterators.len()];
    for (index, iterator) in iterators.iter().enumerate() {
        match super::step_value(iterator) {
            Ok(Some(value)) => values.push(value),
            Ok(None) => {
                ended += 1;
                open[index] = false;
                values.push(Value::Undefined);
                if mode == 0 {
                    break;
                }
            }
            Err(error) => return Err(close_remaining(&iterators[index + 1..], error)),
        }
    }
    if ended == iterators.len() {
        *done = true;
        return Ok(None);
    }
    if ended > 0 && mode != 1 {
        *done = true;
        if mode == 2 {
            return Err(crate::value::error::throw_type_error(
                "Iterator.zip iterators have different lengths",
            ));
        }
        if let Some(error) = close_shortest(iterators, &open)? {
            *done = true;
            return Err(error);
        }
        return Ok(None);
    }
    Ok(Some(Value::array(values)))
}

fn close_shortest(
    iterators: &[Value],
    open: &[bool],
) -> Result<Option<crate::execute::VmError>, crate::execute::VmError> {
    let open_iterators = iterators
        .iter()
        .zip(open)
        .filter_map(|(iterator, is_open)| is_open.then_some(iterator.clone()))
        .collect();
    let completion = close_iterators(
        open_iterators,
        crate::completion::Completion::Return(Value::Undefined),
    )?;
    Ok(completion.into_vm_error().err())
}

fn close_remaining(iterators: &[Value], error: crate::execute::VmError) -> crate::execute::VmError {
    let mut completion = crate::completion::Completion::from_vm_error(error.clone())
        .unwrap_or(crate::completion::Completion::Throw(Value::Undefined));
    for iterator in iterators.iter().rev() {
        completion = close(iterator.clone(), completion.clone()).unwrap_or(completion);
    }
    match completion.into_vm_error() {
        Err(close_error) => close_error,
        Ok(_) => error,
    }
}

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
