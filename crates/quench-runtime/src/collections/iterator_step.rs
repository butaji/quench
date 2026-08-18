//! Iterator stepping — one observable `next` fetch per protocol iterator (GetIterator semantics).

use super::{
    close, close_iterators, iterator_map, iterator_protocol, mark_done, native_step, not_iterable,
};
use crate::value::{IteratorData, IteratorState, Value};

pub(crate) fn step_value(value: &Value) -> Result<Option<Value>, crate::execute::VmError> {
    let Value::Iterator(data) = value else {
        return Err(not_iterable());
    };
    if std::env::var_os("QDEBUG").is_some() {
        eprintln!(
            "step_value state={:?} addr={:p}",
            std::mem::discriminant(&*data.state.borrow()),
            data.state.as_ptr(),
        );
    }
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
    if std::env::var_os("QDEBUG").is_some() {
        eprintln!("step_target called, addr={:p}", data.state.as_ptr());
    }
    if matches!(&*data.state.borrow(), IteratorState::Concat { .. }) {
        return concat_target(data);
    }
    if let Some(step) = user_step_target(data)? {
        return Ok(step);
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

/// Combinator iterator states whose step runs user code. Snapshot the
/// fields, drop the borrow, then drive the step — so re-entrant calls
/// like `iter.next()` from inside a mapper predicate do not collide with
/// the mutable borrow on `state`.
enum Snapshot {
    Mapped {
        iterator: Value,
        mapper: Value,
        index: usize,
        done: bool,
    },
    Filtered {
        iterator: Value,
        predicate: Value,
        index: usize,
        done: bool,
    },
    FlatMapped {
        inner: Value,
        mapper: Value,
        index: usize,
        current: Option<Value>,
        done: bool,
    },
    Dropped {
        inner: Value,
        skipped: usize,
        done: bool,
    },
    Take {
        inner: Value,
        remaining: u64,
    },
    Zip {
        iterators: Vec<Value>,
        mode: u8,
        done: bool,
    },
}

fn snapshot_reentry(data: &IteratorData) -> Option<Snapshot> {
    let state = data.state.borrow();
    Some(match &*state {
        IteratorState::Mapped {
            iterator,
            mapper,
            index,
            done,
        } => Snapshot::Mapped {
            iterator: iterator.clone(),
            mapper: mapper.clone(),
            index: *index,
            done: *done,
        },
        IteratorState::Filtered {
            iterator,
            predicate,
            index,
            done,
        } => Snapshot::Filtered {
            iterator: iterator.clone(),
            predicate: predicate.clone(),
            index: *index,
            done: *done,
        },
        IteratorState::FlatMapped {
            inner,
            mapper,
            index,
            current,
            done,
        } => Snapshot::FlatMapped {
            inner: inner.clone(),
            mapper: mapper.clone(),
            index: *index,
            current: current.clone(),
            done: *done,
        },
        IteratorState::Dropped {
            inner,
            skipped,
            done,
        } => Snapshot::Dropped {
            inner: inner.clone(),
            skipped: *skipped,
            done: *done,
        },
        IteratorState::Take {
            inner,
            remaining,
        } => Snapshot::Take {
            inner: inner.clone(),
            remaining: *remaining,
        },
        IteratorState::Zip {
            iterators,
            mode,
            done,
        } => Snapshot::Zip {
            iterators: iterators.clone(),
            mode: *mode,
            done: *done,
        },
        _ => return None,
    })
}

fn write_snapshot(data: &IteratorData, snapshot: &Snapshot) {
    let mut state = data.state.borrow_mut();
    match snapshot {
        Snapshot::Mapped { index, done, .. } => {
            if let IteratorState::Mapped {
                index: idx, done: d, ..
            } = &mut *state
            {
                *idx = *index;
                *d = *done;
            }
        }
        Snapshot::Filtered { index, done, .. } => {
            if let IteratorState::Filtered {
                index: idx, done: d, ..
            } = &mut *state
            {
                *idx = *index;
                *d = *done;
            }
        }
        Snapshot::FlatMapped {
            index,
            current,
            done,
            ..
        } => {
            if let IteratorState::FlatMapped {
                index: idx,
                current: cur,
                done: d,
                ..
            } = &mut *state
            {
                *idx = *index;
                *cur = current.clone();
                *d = *done;
            }
        }
        Snapshot::Dropped {
            skipped, done, ..
        } => {
            if let IteratorState::Dropped {
                skipped: sk, done: d, ..
            } = &mut *state
            {
                *sk = *skipped;
                *d = *done;
            }
        }
        Snapshot::Take { remaining, .. } => {
            if let IteratorState::Take {
                remaining: rem, ..
            } = &mut *state
            {
                *rem = *remaining;
            }
        }
        Snapshot::Zip { done, .. } => {
            if let IteratorState::Zip { done: d, .. } = &mut *state {
                *d = *done;
            }
        }
    }
}

fn user_step_target(data: &IteratorData) -> Result<Option<StepTarget>, crate::execute::VmError> {
    let Some(snapshot) = snapshot_reentry(data) else {
        return Ok(None);
    };
    if std::env::var_os("QDEBUG").is_some() {
        eprintln!("user_step_target snapshot taken");
    }
    let mut owned = snapshot;
    let result = match &mut owned {
        Snapshot::Mapped {
            iterator,
            mapper,
            index,
            done,
        } => mapped_step(iterator, mapper, index, done).map(StepTarget::Value)?,
        Snapshot::Filtered {
            iterator,
            predicate,
            index,
            done,
        } => filtered_step(iterator, predicate, index, done).map(StepTarget::Value)?,
        Snapshot::FlatMapped {
            inner,
            mapper,
            index,
            current,
            done,
        } => flat_mapped_step(inner, mapper, index, current, done).map(StepTarget::Value)?,
        Snapshot::Dropped {
            inner,
            skipped,
            done,
        } => dropped_step(inner, skipped, done).map(StepTarget::Value)?,
        Snapshot::Take {
            inner,
            remaining,
        } => take_step(inner, remaining).map(StepTarget::Value)?,
        Snapshot::Zip {
            iterators,
            mode,
            done,
        } => zip_step(iterators, *mode, done).map(StepTarget::Value)?,
    };
    if std::env::var_os("QDEBUG").is_some() {
        eprintln!("user_step_target writing snapshot");
    }
    write_snapshot(data, &owned);
    if std::env::var_os("QDEBUG").is_some() {
        eprintln!("user_step_target done");
    }
    Ok(Some(result))
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
    let mut all_done = true;
    for (index, iterator) in iterators.iter().enumerate() {
        match super::step_value(iterator) {
            Ok(Some(value)) => {
                values.push(value);
                if mode == 2 && ended > 0 {
                    break;
                }
                all_done = false;
            }
            Ok(None) => {
                ended += 1;
                open[index] = false;
                values.push(Value::Undefined);
                if mode == 0 || (mode == 2 && !all_done) {
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
            return Err(close_strict(iterators, &open));
        }
        if let Some(error) = close_shortest(iterators, &open)? {
            *done = true;
            return Err(error);
        }
        return Ok(None);
    }
    Ok(Some(Value::array(values)))
}

fn close_strict(iterators: &[Value], open: &[bool]) -> crate::execute::VmError {
    let open_iterators = iterators
        .iter()
        .zip(open)
        .filter_map(|(iterator, is_open)| is_open.then_some(iterator.clone()))
        .collect();
    let error =
        crate::value::error::throw_type_error("Iterator.zip iterators have different lengths");
    let completion = match crate::completion::Completion::from_vm_error(error.clone()) {
        Ok(completion) => completion,
        Err(_) => return error,
    };
    match close_iterators(open_iterators, completion) {
        Ok(completion) => match completion.into_vm_error() {
            Err(error) => error,
            Ok(_) => error,
        },
        Err(error) => error,
    }
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
    done: &mut bool,
) -> Result<Option<Value>, crate::execute::VmError> {
    if *done {
        return Ok(None);
    }
    let value = match super::step_value(inner) {
        Ok(Some(value)) => value,
        Ok(None) => {
            *done = true;
            return Ok(None);
        }
        Err(error) => return Err(error),
    };
    *skipped += 1;
    Ok(Some(value))
}

fn take_step(
    inner: &Value,
    remaining: &mut u64,
) -> Result<Option<Value>, crate::execute::VmError> {
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
    if matches!(value, Value::Undefined | Value::Null) {
        return Ok(None);
    }
    if !crate::value::is_object(&value) {
        return Ok(None);
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
