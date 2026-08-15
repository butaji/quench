//! Iterator stepping — one observable `next` fetch per protocol iterator (GetIterator semantics).

use super::{iterator_map, iterator_protocol, mark_done, native_step, not_iterable};
use crate::value::{IteratorData, IteratorState, Value};

pub(crate) fn step_value(value: &Value) -> Result<Option<Value>, crate::execute::VmError> {
    let Value::Iterator(data) = value else {
        return Err(not_iterable());
    };
    if let Some(iterator) = drop_target(data) {
        return drop_step(data, iterator);
    }
    match step_target(data)? {
        StepTarget::Value(value) => Ok(value),
        StepTarget::Protocol(iterator, cached) => {
            let next = resolve_next(data, &iterator, cached)?;
            let result = protocol_next(data, &next, &iterator)?;
            protocol_result(data, result)
        }
    }
}

fn drop_target(data: &IteratorData) -> Option<Value> {
    let state = data.state.borrow();
    let IteratorState::Drop { iterator, .. } = &*state else {
        return None;
    };
    Some(iterator.clone())
}

fn drop_step(
    data: &IteratorData,
    iterator: Value,
) -> Result<Option<Value>, crate::execute::VmError> {
    let mut remaining = {
        let state = data.state.borrow();
        let IteratorState::Drop {
            remaining, done, ..
        } = &*state
        else {
            return Ok(None);
        };
        if *done {
            return Ok(None);
        }
        *remaining
    };
    while remaining > 0 {
        if step_source(&iterator)?.is_none() {
            mark_drop_done(data);
            return Ok(None);
        }
        remaining -= 1;
        set_drop_remaining(data, remaining);
    }
    let value = step_source(&iterator)?;
    if value.is_none() {
        mark_drop_done(data);
    }
    Ok(value)
}

fn step_source(value: &Value) -> Result<Option<Value>, crate::execute::VmError> {
    if matches!(value, Value::Iterator(_)) {
        return step_value(value);
    }
    let result = crate::generator::next(Some(value), &[])?;
    let done = crate::execute::get_property_result(&result, "done")?;
    if crate::execute::is_truthy(&done) {
        return Ok(None);
    }
    Ok(Some(crate::execute::get_property_result(&result, "value")?))
}

fn set_drop_remaining(data: &IteratorData, remaining: u64) {
    if let IteratorState::Drop {
        remaining: slot, ..
    } = &mut *data.state.borrow_mut()
    {
        *slot = remaining;
    }
}

fn mark_drop_done(data: &IteratorData) {
    if let IteratorState::Drop { done, .. } = &mut *data.state.borrow_mut() {
        *done = true;
    }
}

fn protocol_next(
    data: &IteratorData,
    next: &Value,
    iterator: &Value,
) -> Result<Value, crate::execute::VmError> {
    {
        let mut state = data.state.borrow_mut();
        let IteratorState::Protocol { executing, .. } = &mut *state else {
            return iterator_protocol::call_next(data, next, iterator);
        };
        if *executing {
            return Err(crate::value::error::throw_type_error(
                "Iterator next called while iterator is executing",
            ));
        }
        *executing = true;
    }
    let result = iterator_protocol::call_next(data, next, iterator);
    if let IteratorState::Protocol { executing, .. } = &mut *data.state.borrow_mut() {
        *executing = false;
    }
    result
}

enum StepTarget {
    Value(Option<Value>),
    Protocol(Value, Value),
}

fn step_target(data: &IteratorData) -> Result<StepTarget, crate::execute::VmError> {
    if matches!(&*data.state.borrow(), IteratorState::Concat { .. }) {
        return concat_step(data);
    }
    let mut state = data.state.borrow_mut();
    Ok(match &mut *state {
        IteratorState::Native {
            values,
            index,
            done,
        } => StepTarget::Value(native_step(values, index, done)),
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
        IteratorState::Concat { .. } => StepTarget::Value(None),
        IteratorState::Drop { .. } => StepTarget::Value(None),
    })
}

fn concat_step(data: &IteratorData) -> Result<StepTarget, crate::execute::VmError> {
    loop {
        let item = {
            let mut state = data.state.borrow_mut();
            let IteratorState::Concat {
                items,
                index,
                current,
                done,
                ..
            } = &mut *state
            else {
                return Ok(StepTarget::Value(None));
            };
            if *done {
                return Ok(StepTarget::Value(None));
            }
            if current.is_none() {
                let Some((value, method)) = items.get(*index).cloned() else {
                    *done = true;
                    return Ok(StepTarget::Value(None));
                };
                *index += 1;
                *current = Some(super::open_with_method(value, method)?);
            }
            current.clone()
        };
        let Some(item) = item else { continue };
        if let Some(value) = super::step_value(&item)? {
            return Ok(StepTarget::Value(Some(value)));
        }
        if let IteratorState::Concat { current, .. } = &mut *data.state.borrow_mut() {
            *current = None;
        }
    }
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
