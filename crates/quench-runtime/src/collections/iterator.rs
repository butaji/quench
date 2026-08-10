use std::{cell::RefCell, rc::Rc};

use crate::value::{IteratorData, IteratorState, Value};

pub(crate) fn execute(
    registers: &mut Vec<Value>,
    op: &crate::ops::Op,
) -> Result<(), crate::execute::VmError> {
    use crate::ops::Op;
    match op {
        Op::RequireObjectCoercible { src } => require_object_coercible(read(registers, *src)?)?,
        Op::GetIterator { dst, iterable } => {
            let iterator = open(read(registers, *iterable)?)?;
            crate::execute::write_value(registers, *dst, iterator);
        }
        Op::IteratorStep { dst, iterator } => {
            let value = step(read(registers, *iterator)?)?;
            crate::execute::write_value(registers, *dst, value);
        }
        Op::IteratorRest { dst, iterator } => {
            let value = rest(read(registers, *iterator)?)?;
            crate::execute::write_value(registers, *dst, value);
        }
        _ => return Err(crate::execute::VmError::MissingReturn),
    }
    Ok(())
}

fn read(registers: &[Value], index: u16) -> Result<Value, crate::execute::VmError> {
    crate::execute::read_register(registers, index)
}

fn require_object_coercible(value: Value) -> Result<(), crate::execute::VmError> {
    if matches!(value, Value::Null | Value::Undefined) {
        return Err(crate::value::error::throw_type_error(
            "Cannot destructure null or undefined",
        ));
    }
    Ok(())
}

fn open(value: Value) -> Result<Value, crate::execute::VmError> {
    if matches!(value, Value::Iterator(_)) {
        return Ok(value);
    }
    let method = crate::execute::get_property_result(&value, "Symbol.iterator")?;
    if matches!(method, Value::Undefined) {
        if matches!(value, Value::Generator(_)) {
            return open_self_iterator(value);
        }
        return iterable_values(value).map(make);
    }
    let iterator = call(&method, &value)?;
    if !crate::value::is_object(&iterator) {
        return Err(not_iterable());
    }
    let next = crate::execute::get_property_result(&iterator, "next")?;
    if !crate::conversion::is_callable(&next) {
        return Err(not_iterable());
    }
    Ok(make_protocol(iterator, next))
}

fn open_self_iterator(iterator: Value) -> Result<Value, crate::execute::VmError> {
    let next = crate::execute::get_property_result(&iterator, "next")?;
    if !crate::conversion::is_callable(&next) {
        return Err(not_iterable());
    }
    Ok(make_protocol(iterator, next))
}

fn step(value: Value) -> Result<Value, crate::execute::VmError> {
    Ok(step_value(&value)?.unwrap_or(Value::Undefined))
}

fn rest(value: Value) -> Result<Value, crate::execute::VmError> {
    collect_iterable(value).map(Value::array)
}

pub(crate) fn collect(value: &Value) -> Result<Vec<Value>, crate::execute::VmError> {
    let mut values = Vec::new();
    while let Some(value) = step_value(value)? {
        values.push(value);
    }
    Ok(values)
}

pub(crate) fn collect_iterable(value: Value) -> Result<Vec<Value>, crate::execute::VmError> {
    let iterator = open(value)?;
    collect(&iterator)
}

fn step_value(value: &Value) -> Result<Option<Value>, crate::execute::VmError> {
    let Value::Iterator(data) = value else {
        return Err(not_iterable());
    };
    let protocol = {
        let mut state = data.state.borrow_mut();
        match &mut *state {
            IteratorState::Native {
                values,
                index,
                done,
            } => return Ok(native_step(values, index, done)),
            IteratorState::Protocol {
                iterator,
                next,
                done,
            } if !*done => Some((iterator.clone(), next.clone())),
            IteratorState::Protocol { .. } => None,
        }
    };
    let Some((iterator, next)) = protocol else {
        return Ok(None);
    };
    let result = call(&next, &iterator)?;
    protocol_result(data, result)
}

fn native_step(values: &[Value], index: &mut usize, done: &mut bool) -> Option<Value> {
    if *done {
        return None;
    }
    let value = values.get(*index).cloned();
    *index = index.saturating_add(1);
    *done = value.is_none();
    value
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

fn mark_done(data: &IteratorData) {
    match &mut *data.state.borrow_mut() {
        IteratorState::Native { done, .. } | IteratorState::Protocol { done, .. } => *done = true,
    }
}

fn call(callee: &Value, receiver: &Value) -> Result<Value, crate::execute::VmError> {
    match callee {
        Value::Proxy(_) => crate::proxy::proxy_apply(callee, receiver, &[]),
        _ => crate::functions::execute_target(callee, receiver, &[]),
    }
}

fn iterable_values(value: Value) -> Result<Vec<Value>, crate::execute::VmError> {
    match value {
        Value::Array(values) => Ok(values.snapshot()),
        Value::String(value) => Ok(value
            .chars()
            .map(|character| Value::String(character.to_string()))
            .collect()),
        value => typed_values(value),
    }
}

macro_rules! number_values {
    ($data:expr) => {
        collect_typed(
            $data.logical_len(),
            $data.buffer.byte_length() < $data.byte_offset,
            |index| $data.get(index).map(|value| Value::Number(value.into())),
        )
    };
}

macro_rules! bigint_values {
    ($data:expr) => {
        collect_typed(
            $data.logical_len(),
            $data.buffer.byte_length() < $data.byte_offset,
            |index| {
                $data
                    .get(index)
                    .map(|value| Value::BigInt(value.to_string()))
            },
        )
    };
}

fn typed_values(value: Value) -> Result<Vec<Value>, crate::execute::VmError> {
    match value {
        Value::Float64Array(data) => number_values!(data),
        Value::Float32Array(data) => number_values!(data),
        Value::Int8Array(data) => number_values!(data),
        Value::Int16Array(data) => number_values!(data),
        Value::Int32Array(data) => number_values!(data),
        Value::Uint8Array(data) => number_values!(data),
        Value::Uint8ClampedArray(data) => number_values!(data),
        Value::Uint16Array(data) => number_values!(data),
        Value::Uint32Array(data) => number_values!(data),
        Value::BigInt64Array(data) => bigint_values!(data),
        Value::BigUint64Array(data) => bigint_values!(data),
        _ => Err(not_iterable()),
    }
}

fn collect_typed(
    length: usize,
    out_of_bounds: bool,
    mut get: impl FnMut(usize) -> Option<Value>,
) -> Result<Vec<Value>, crate::execute::VmError> {
    if out_of_bounds {
        return Err(not_iterable());
    }
    (0..length)
        .map(|index| get(index).ok_or_else(not_iterable))
        .collect()
}

fn not_iterable() -> crate::execute::VmError {
    crate::value::error::throw_type_error("value is not iterable")
}

pub(crate) fn from_map(receiver: Option<&Value>) -> Value {
    let Some(Value::Map(data)) = receiver else {
        return empty();
    };
    let values = data
        .keys
        .iter()
        .zip(&data.values)
        .map(|(key, value)| Value::array(vec![key.clone(), value.clone()]))
        .collect();
    make(values)
}

pub(crate) fn from_set(receiver: Option<&Value>) -> Value {
    let Some(Value::Set(data)) = receiver else {
        return empty();
    };
    make(data.values.iter().cloned().collect())
}

pub(crate) fn next(receiver: Option<&Value>) -> Value {
    let Some(iterator @ Value::Iterator(_)) = receiver else {
        return result(Value::Undefined, true);
    };
    match step_value(iterator) {
        Ok(Some(value)) => result(value, false),
        Ok(None) => result(Value::Undefined, true),
        Err(_) => result(Value::Undefined, true),
    }
}

pub(crate) fn property(key: &str) -> Value {
    match key {
        "next" => Value::Builtin(crate::ops::Builtin::IteratorNext),
        _ => Value::Undefined,
    }
}

pub(crate) fn make(values: Vec<Value>) -> Value {
    Value::Iterator(Rc::new(IteratorData {
        state: RefCell::new(IteratorState::Native {
            values,
            index: 0,
            done: false,
        }),
    }))
}

fn make_protocol(iterator: Value, next: Value) -> Value {
    Value::Iterator(Rc::new(IteratorData {
        state: RefCell::new(IteratorState::Protocol {
            iterator,
            next,
            done: false,
        }),
    }))
}

fn empty() -> Value {
    make(Vec::new())
}

fn result(value: Value, done: bool) -> Value {
    Value::Object(Rc::new(vec![
        ("value".to_string(), value),
        ("done".to_string(), Value::Boolean(done)),
    ]))
}
