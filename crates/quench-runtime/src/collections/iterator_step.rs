//! Iterator stepping — one observable `next` fetch per protocol iterator (GetIterator semantics).

use super::{iterator_map, iterator_protocol, mark_done, native_step, not_iterable};
use crate::value::{IteratorData, IteratorState, Value};

pub(crate) fn step_value(value: &Value) -> Result<Option<Value>, crate::execute::VmError> {
    let Value::Iterator(data) = value else {
        return Err(not_iterable());
    };
    match step_target(data)? {
        StepTarget::Value(value) => Ok(value),
        StepTarget::ArrayLike(value, index) => array_like_step(data, value, index),
        StepTarget::Protocol(iterator, cached) => {
            let next = resolve_next(data, &iterator, cached)?;
            let result = iterator_protocol::call_next(data, &next, &iterator)?;
            protocol_result(data, result)
        }
    }
}

enum StepTarget {
    Value(Option<Value>),
    ArrayLike(Value, usize),
    Protocol(Value, Value),
}

fn step_target(data: &IteratorData) -> Result<StepTarget, crate::execute::VmError> {
    let mut state = data.state.borrow_mut();
    Ok(match &mut *state {
        IteratorState::Native {
            values,
            index,
            done,
        } => StepTarget::Value(native_step(values, index, done)),
        IteratorState::ArrayLike { value, index, done } if !*done => {
            StepTarget::ArrayLike(value.clone(), *index)
        }
        IteratorState::ArrayLike { .. } => StepTarget::Value(None),
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
    })
}

fn array_like_step(
    data: &IteratorData,
    value: Value,
    index: usize,
) -> Result<Option<Value>, crate::execute::VmError> {
    let length_value = crate::execute::get_property_result(&value, "length")?;
    let length = crate::conversion::to_number(&length_value)?;
    let length = if length.is_finite() && length > 0.0 {
        length.floor().min(9_007_199_254_740_991.0) as usize
    } else {
        0
    };
    if index >= length {
        mark_done(data);
        return Ok(None);
    }
    let result = crate::execute::get_property_result(&value, &index.to_string())?;
    if let IteratorState::ArrayLike { index, .. } = &mut *data.state.borrow_mut() {
        *index += 1;
    }
    Ok(Some(result))
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
