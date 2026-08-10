use std::{cell::RefCell, rc::Rc};

use crate::value::{IteratorData, Value};

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
