//! Iterator stepping — one observable `next` fetch per protocol iterator (GetIterator semantics).

use super::{iterator_map, iterator_protocol, mark_done, native_step, not_iterable};
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
    let mut state = data.state.borrow_mut();
    step_target_state(data, &mut state)
}

fn step_target_state(
    _data: &IteratorData,
    state: &mut IteratorState,
) -> Result<StepTarget, crate::execute::VmError> {
    Ok(match state {
        IteratorState::Native {
            values,
            receiver,
            index,
            done,
        } => StepTarget::Value(native_step(values, receiver.as_ref(), index, done)),
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
