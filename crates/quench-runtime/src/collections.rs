// Map and Set builtins — canonical JS semantics for insertion-order collections.

use std::collections::VecDeque;
use std::rc::Rc;

use crate::{
    execute::VmError,
    ops::Builtin,
    value::{MapData, SetData, Value},
};

fn same_value_zero(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Number(l), Value::Number(r)) => l.is_nan() && r.is_nan() || l == r,
        _ => left == right,
    }
}

pub(crate) fn map_new(_arguments: &[Value]) -> Value {
    Value::Map(Rc::new(MapData { keys: VecDeque::new(), values: Vec::new() }))
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
    Value::Map(Rc::new(data))
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
    let Value::Map(_data) = receiver.unwrap() else {
        return Value::Undefined;
    };
    Value::Map(Rc::new(MapData { keys: VecDeque::new(), values: Vec::new() }))
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

pub(crate) fn set_new(_arguments: &[Value]) -> Value {
    Value::Set(Rc::new(SetData { values: VecDeque::new() }))
}

pub(crate) fn set_add(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Value::Set(data) = receiver.unwrap() else {
        return Value::Undefined;
    };
    let Some(value) = arguments.first() else {
        return Value::Undefined;
    };
    let mut data = (**data).clone();
    if !data.values.iter().any(|v| same_value_zero(v, value)) {
        data.values.push_back(value.clone());
    }
    Value::Set(Rc::new(data))
}

pub(crate) fn set_has(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Value::Set(data) = receiver.unwrap() else {
        return Value::Boolean(false);
    };
    let Some(value) = arguments.first() else {
        return Value::Boolean(false);
    };
    Value::Boolean(data.values.iter().any(|v| same_value_zero(v, value)))
}

pub(crate) fn set_delete(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Value::Set(data) = receiver.unwrap() else {
        return Value::Boolean(false);
    };
    let Some(value) = arguments.first() else {
        return Value::Boolean(false);
    };
    let mut data = (**data).clone();
    if let Some(pos) = data.values.iter().position(|v| same_value_zero(v, value)) {
        data.values.remove(pos);
        Value::Boolean(true)
    } else {
        Value::Boolean(false)
    }
}

pub(crate) fn set_clear(receiver: Option<&Value>) -> Value {
    let Value::Set(_data) = receiver.unwrap() else {
        return Value::Undefined;
    };
    Value::Set(Rc::new(SetData { values: VecDeque::new() }))
}

pub(crate) fn set_for_each(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let Value::Set(data) = receiver.unwrap() else {
        return Ok(Value::Undefined);
    };
    let Some(callback) = arguments.first() else {
        return Ok(Value::Undefined);
    };
    let this_arg = arguments.get(1).cloned().unwrap_or(Value::Undefined);
    let set = receiver.cloned().unwrap_or(Value::Undefined);
    let values: Vec<Value> = data.values.iter().cloned().collect();
    for value in values {
        let args = [value.clone(), value, set.clone()];
        crate::functions::execute_target(callback, &this_arg, &args)?;
    }
    Ok(Value::Undefined)
}

pub(crate) fn execute_builtin(
    builtin: Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    use Builtin::*;
    match builtin {
        Map => Some(Ok(map_new(arguments))),
        Set => Some(Ok(set_new(arguments))),
        MapSet => Some(Ok(map_set(receiver, arguments))),
        MapGet => Some(Ok(map_get(receiver, arguments))),
        MapHas => Some(Ok(map_has(receiver, arguments))),
        MapDelete => Some(Ok(map_delete(receiver, arguments))),
        MapClear => Some(Ok(map_clear(receiver))),
        MapForEach => Some(map_for_each(receiver, arguments)),
        SetAdd => Some(Ok(set_add(receiver, arguments))),
        SetHas => Some(Ok(set_has(receiver, arguments))),
        SetDelete => Some(Ok(set_delete(receiver, arguments))),
        SetClear => Some(Ok(set_clear(receiver))),
        SetForEach => Some(set_for_each(receiver, arguments)),
        _ => None,
    }
}
