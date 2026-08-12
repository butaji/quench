pub(crate) fn from_map(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    let Some(Value::Map(data)) =
        receiver.filter(|value| matches!(value, Value::Map(data) if !data.weak))
    else {
        return Err(crate::value::error::throw_type_error(
            "Map iterator called on incompatible receiver",
        ));
    };
    Ok(make_map(Rc::clone(data), 0))
}

pub(crate) fn from_map_keys(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    let Some(Value::Map(data)) =
        receiver.filter(|value| matches!(value, Value::Map(data) if !data.weak))
    else {
        return Err(crate::value::error::throw_type_error(
            "Map iterator called on incompatible receiver",
        ));
    };
    Ok(make_map(Rc::clone(data), 1))
}

pub(crate) fn from_map_values(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    let Some(Value::Map(data)) =
        receiver.filter(|value| matches!(value, Value::Map(data) if !data.weak))
    else {
        return Err(crate::value::error::throw_type_error(
            "Map iterator called on incompatible receiver",
        ));
    };
    Ok(make_map(Rc::clone(data), 2))
}

pub(crate) fn from_set(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    let Some(Value::Set(data)) =
        receiver.filter(|value| matches!(value, Value::Set(data) if !data.weak))
    else {
        return Err(crate::value::error::throw_type_error(
            "Set iterator called on incompatible receiver",
        ));
    };
    Ok(make_set(Rc::clone(data)))
}

pub(crate) fn next(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    let Some(iterator @ Value::Iterator(_)) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "Iterator.prototype.next called on incompatible receiver",
        ));
    };
    match step_value(iterator) {
        Ok(Some(value)) => Ok(result(value, false)),
        Ok(None) => Ok(result(Value::Undefined, true)),
        Err(error) => Err(error),
    }
}

pub(crate) fn property(key: &str) -> Value {
    match key {
        "next" => Value::Builtin(crate::ops::Builtin::IteratorNext),
        "Symbol.iterator" => Value::Builtin(crate::ops::Builtin::IteratorSelf),
        _ => Value::Undefined,
    }
}

pub(crate) fn property_for(value: &Value, key: &str) -> Value {
    if key != "Symbol.toStringTag" {
        return property(key);
    }
    let Value::Iterator(data) = value else {
        return Value::Undefined;
    };
    let tag = match &*data.state.borrow() {
        IteratorState::Native { .. } => "Array Iterator",
        IteratorState::Set { .. } => "Set Iterator",
        IteratorState::Map { .. } => "Map Iterator",
        IteratorState::Protocol { .. } => "Iterator",
    };
    Value::String(tag.to_string())
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

fn make_set(data: Rc<crate::value::SetData>) -> Value {
    Value::Iterator(Rc::new(IteratorData {
        state: RefCell::new(IteratorState::Set {
            data,
            index: 0,
            done: false,
        }),
    }))
}

fn make_map(data: Rc<crate::value::MapData>, kind: u8) -> Value {
    Value::Iterator(Rc::new(IteratorData {
        state: RefCell::new(IteratorState::Map {
            data,
            index: 0,
            kind,
            done: false,
        }),
    }))
}

fn result(value: Value, done: bool) -> Value {
    Value::Object(Rc::new(crate::value::ObjectData::new(vec![
        ("value".to_string(), value),
        ("done".to_string(), Value::Boolean(done)),
    ])))
}
use super::step_value;
use crate::value::{IteratorData, IteratorState, Value};
use std::{cell::RefCell, rc::Rc};
