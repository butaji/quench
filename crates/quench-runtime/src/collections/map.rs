//! Map builtin — constructor and instance methods.

use std::collections::VecDeque;
use std::rc::Rc;

use crate::{
    execute::VmError,
    ops::Builtin,
    value::{MapData, Value},
};

fn same_value_zero(left: &Value, right: &Value) -> bool {
    crate::builtins::same_value_zero(left, right)
}

pub fn property(key: &str) -> Value {
    match key {
        "set" => Value::Builtin(Builtin::MapSet),
        "get" => Value::Builtin(Builtin::MapGet),
        "has" => Value::Builtin(Builtin::MapHas),
        "delete" => Value::Builtin(Builtin::MapDelete),
        "clear" => Value::Builtin(Builtin::MapClear),
        "forEach" => Value::Builtin(Builtin::MapForEach),
        "Symbol.iterator" => Value::Builtin(Builtin::MapIterator),
        _ => Value::Undefined,
    }
}

pub(crate) fn map_new(arguments: &[Value]) -> Value {
    let mut data = MapData {
        keys: VecDeque::new(),
        values: Vec::new(),
    };
    if let Some(Value::Array(entries)) = arguments.first() {
        for entry in entries.iter() {
            if let Value::Array(pair) = entry {
                data.keys
                    .push_back(pair.first().cloned().unwrap_or(Value::Undefined));
                data.values
                    .push(pair.get(1).cloned().unwrap_or(Value::Undefined));
            }
        }
    }
    Value::Map(Rc::new(data))
}

pub(crate) fn map_set(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Value::Map(data) = receiver.unwrap() else {
        return Value::Undefined;
    };
    let Some(key) = arguments.first() else {
        return Value::Undefined;
    };
    let value = arguments.get(1).cloned().unwrap_or(Value::Undefined);
    let mut data = (**data).clone();
    if let Some(pos) = data.keys.iter().position(|k| same_value_zero(k, key)) {
        data.values[pos] = value;
    } else {
        data.keys.push_back(key.clone());
        data.values.push(value);
    }
    let result = Value::Map(Rc::new(data));
    crate::locals::replace_value(receiver.unwrap(), &result);
    result
}

pub(crate) fn map_get(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Value::Map(data) = receiver.unwrap() else {
        return Value::Undefined;
    };
    let Some(key) = arguments.first() else {
        return Value::Undefined;
    };
    data.keys
        .iter()
        .position(|k| same_value_zero(k, key))
        .and_then(|pos| data.values.get(pos).cloned())
        .unwrap_or(Value::Undefined)
}

pub(crate) fn map_has(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Value::Map(data) = receiver.unwrap() else {
        return Value::Boolean(false);
    };
    let Some(key) = arguments.first() else {
        return Value::Boolean(false);
    };
    Value::Boolean(data.keys.iter().any(|k| same_value_zero(k, key)))
}

pub(crate) fn map_delete(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Value::Map(data) = receiver.unwrap() else {
        return Value::Boolean(false);
    };
    let Some(key) = arguments.first() else {
        return Value::Boolean(false);
    };
    let mut data = (**data).clone();
    if let Some(pos) = data.keys.iter().position(|k| same_value_zero(k, key)) {
        data.keys.remove(pos);
        data.values.remove(pos);
        Value::Boolean(true)
    } else {
        Value::Boolean(false)
    }
}

pub(crate) fn map_clear(receiver: Option<&Value>) -> Value {
    if !matches!(receiver, Some(Value::Map(_))) {
        return Value::Undefined;
    }
    Value::Map(Rc::new(MapData {
        keys: VecDeque::new(),
        values: Vec::new(),
    }))
}

pub(crate) fn map_for_each(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let Value::Map(data) = receiver.unwrap() else {
        return Ok(Value::Undefined);
    };
    let Some(callback) = arguments.first() else {
        return Ok(Value::Undefined);
    };
    let this_arg = arguments.get(1).cloned().unwrap_or(Value::Undefined);
    let map = receiver.cloned().unwrap_or(Value::Undefined);
    let data = (**data).clone();
    for (key, value) in data.keys.iter().zip(data.values.iter()) {
        let args = [value.clone(), key.clone(), map.clone()];
        crate::functions::execute_target(callback, &this_arg, &args)?;
    }
    Ok(Value::Undefined)
}
