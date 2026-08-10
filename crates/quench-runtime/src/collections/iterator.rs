use std::{cell::RefCell, rc::Rc};

use crate::value::{IteratorData, Value};

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
    let values = iterable_values(value)?;
    Ok(make(values))
}

fn step(value: Value) -> Result<Value, crate::execute::VmError> {
    let Value::Iterator(data) = value else {
        return Err(not_iterable());
    };
    let mut index = data.index.borrow_mut();
    let value = data.values.get(*index).cloned().unwrap_or(Value::Undefined);
    *index = index.saturating_add(1);
    Ok(value)
}

fn rest(value: Value) -> Result<Value, crate::execute::VmError> {
    let Value::Iterator(data) = value else {
        return Err(not_iterable());
    };
    let mut index = data.index.borrow_mut();
    let values = data.values.get(*index..).unwrap_or_default().to_vec();
    *index = data.values.len();
    Ok(Value::array(values))
}

fn iterable_values(value: Value) -> Result<Vec<Value>, crate::execute::VmError> {
    match value {
        Value::Array(values) => Ok(values.snapshot()),
        Value::String(value) => Ok(value
            .chars()
            .map(|character| Value::String(character.to_string()))
            .collect()),
        Value::Iterator(data) => Ok(data.values.clone()),
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
    let Some(Value::Iterator(data)) = receiver else {
        return result(Value::Undefined, true);
    };
    let mut index = data.index.borrow_mut();
    let value = data.values.get(*index).cloned();
    if value.is_some() {
        *index += 1;
    }
    let done = value.is_none();
    result(value.unwrap_or(Value::Undefined), done)
}

pub(crate) fn property(key: &str) -> Value {
    match key {
        "next" => Value::Builtin(crate::ops::Builtin::IteratorNext),
        _ => Value::Undefined,
    }
}

fn make(values: Vec<Value>) -> Value {
    Value::Iterator(Rc::new(IteratorData {
        values,
        index: RefCell::new(0),
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
