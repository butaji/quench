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
        "entries" => Value::Builtin(Builtin::MapEntries),
        "keys" => Value::Builtin(Builtin::MapKeys),
        "values" => Value::Builtin(Builtin::MapValues),
        "Symbol.iterator" => Value::Builtin(Builtin::MapIterator),
        _ => Value::Undefined,
    }
}

pub(crate) fn weak_property(key: &str) -> Value {
    match key {
        "set" => Value::Builtin(Builtin::WeakMapSet),
        "get" => Value::Builtin(Builtin::WeakMapGet),
        "has" => Value::Builtin(Builtin::WeakMapHas),
        "delete" => Value::Builtin(Builtin::WeakMapDelete),
        "getOrInsert" => Value::Builtin(Builtin::WeakMapGetOrInsert),
        "getOrInsertComputed" => Value::Builtin(Builtin::WeakMapGetOrInsertComputed),
        _ => Value::Undefined,
    }
}

pub(crate) fn map_new(arguments: &[Value]) -> Value {
    let mut data = MapData {
        weak: false,
        keys: VecDeque::new(),
        values: Vec::new(),
        prototype: std::cell::RefCell::new(None),
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

pub(crate) fn weak_map_new(arguments: &[Value]) -> Result<Value, VmError> {
    let entries = match arguments.first() {
        None | Some(Value::Undefined | Value::Null) => Vec::new(),
        Some(Value::Array(entries)) => entries.iter().cloned().collect(),
        Some(_) => {
            return Err(crate::value::error::throw_type_error(
                "WeakMap iterator is not callable",
            ))
        }
    };
    let set = Value::Map(Rc::new(MapData {
        weak: true,
        keys: VecDeque::new(),
        values: Vec::new(),
        prototype: std::cell::RefCell::new(None),
    }));
    if entries.is_empty() {
        return Ok(set);
    }
    populate_weak_map(set, entries)
}

fn populate_weak_map(map: Value, entries: Vec<Value>) -> Result<Value, VmError> {
    let setter =
        crate::execute::get_property_result(&Value::Builtin(Builtin::WeakMapPrototype), "set")?;
    if !crate::conversion::is_callable(&setter) {
        return Err(crate::value::error::throw_type_error(
            "WeakMap.prototype.set is not callable",
        ));
    }
    let mut map = map;
    for entry in entries {
        let Value::Array(pair) = entry else {
            return Err(crate::value::error::throw_type_error(
                "Iterator value is not an entry object",
            ));
        };
        let key = pair.first().cloned().unwrap_or(Value::Undefined);
        let value = pair.get(1).cloned().unwrap_or(Value::Undefined);
        let result = crate::functions::execute_target(&setter, &map, &[key, value])?;
        if matches!(result, Value::Map(_)) {
            map = result;
        }
    }
    Ok(map)
}

pub(crate) fn weak_map_get_or_insert(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let key = weak_key(receiver, arguments.first())?;
    if matches!(
        map_has(receiver, std::slice::from_ref(&key))?,
        Value::Boolean(true)
    ) {
        return map_get(receiver, &[key]);
    }
    let value = arguments.get(1).cloned().unwrap_or(Value::Undefined);
    map_set(receiver, &[key, value.clone()])?;
    Ok(value)
}

pub(crate) fn weak_map_set(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let key = weak_key(receiver, arguments.first())?;
    map_set(
        receiver,
        &[key, arguments.get(1).cloned().unwrap_or(Value::Undefined)],
    )
}

pub(crate) fn weak_map_get(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    if !weak_receiver(receiver) {
        return Err(crate::value::error::throw_type_error(
            "WeakMap method called on incompatible receiver",
        ));
    }
    if arguments.first().is_some_and(|key| !is_weak_key(key)) {
        return Ok(Value::Undefined);
    }
    map_get(receiver, arguments)
}

pub(crate) fn weak_map_has(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    if !weak_receiver(receiver) {
        return Err(crate::value::error::throw_type_error(
            "WeakMap method called on incompatible receiver",
        ));
    }
    if arguments.first().is_some_and(|key| !is_weak_key(key)) {
        return Ok(Value::Boolean(false));
    }
    map_has(receiver, arguments)
}

pub(crate) fn weak_map_delete(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    if !weak_receiver(receiver) {
        return Err(crate::value::error::throw_type_error(
            "WeakMap method called on incompatible receiver",
        ));
    }
    if arguments.first().is_some_and(|key| !is_weak_key(key)) {
        return Ok(Value::Boolean(false));
    }
    map_delete(receiver, arguments)
}

pub(crate) fn weak_map_get_or_insert_computed(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let key = weak_key(receiver, arguments.first())?;
    if matches!(
        map_has(receiver, std::slice::from_ref(&key))?,
        Value::Boolean(true)
    ) {
        return map_get(receiver, &[key]);
    }
    let callback = arguments.get(1).ok_or_else(|| {
        crate::value::error::throw_type_error("WeakMap callback must be callable")
    })?;
    if !crate::conversion::is_callable(callback) {
        return Err(crate::value::error::throw_type_error(
            "WeakMap callback must be callable",
        ));
    }
    let value =
        crate::functions::execute_target(callback, &Value::Undefined, std::slice::from_ref(&key))?;
    map_set(receiver, &[key, value.clone()])?;
    Ok(value)
}

fn weak_key(receiver: Option<&Value>, key: Option<&Value>) -> Result<Value, VmError> {
    if !weak_receiver(receiver) {
        return Err(crate::value::error::throw_type_error(
            "WeakMap method called on incompatible receiver",
        ));
    }
    let Some(key) = key.filter(|value| is_weak_key(value)) else {
        return Err(crate::value::error::throw_type_error(
            "Invalid value used as weak map key",
        ));
    };
    Ok(key.clone())
}

fn weak_receiver(receiver: Option<&Value>) -> bool {
    matches!(receiver, Some(Value::Map(data)) if data.weak)
}

fn is_weak_key(value: &Value) -> bool {
    crate::value::is_object(value)
        || matches!(value, Value::String(text) if text.starts_with("Symbol.") && !text.starts_with("Symbol.for.") && text.contains('\0'))
}

pub(crate) fn map_set(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let Some(receiver @ Value::Map(data)) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "Map method called on incompatible receiver",
        ));
    };
    let Some(key) = arguments.first() else {
        return Ok(Value::Undefined);
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
    crate::locals::replace_value(receiver, &result);
    Ok(result)
}

pub(crate) fn map_get(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::Map(data)) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "Map method called on incompatible receiver",
        ));
    };
    let Some(key) = arguments.first() else {
        return Ok(Value::Undefined);
    };
    Ok(data
        .keys
        .iter()
        .position(|k| same_value_zero(k, key))
        .and_then(|pos| data.values.get(pos).cloned())
        .unwrap_or(Value::Undefined))
}

pub(crate) fn map_has(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::Map(data)) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "Map method called on incompatible receiver",
        ));
    };
    let Some(key) = arguments.first() else {
        return Ok(Value::Boolean(false));
    };
    Ok(Value::Boolean(
        data.keys.iter().any(|k| same_value_zero(k, key)),
    ))
}

pub(crate) fn map_delete(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let Some(receiver @ Value::Map(data)) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "Map method called on incompatible receiver",
        ));
    };
    let Some(key) = arguments.first() else {
        return Ok(Value::Boolean(false));
    };
    let mut data = (**data).clone();
    if let Some(pos) = data.keys.iter().position(|k| same_value_zero(k, key)) {
        data.keys.remove(pos);
        data.values.remove(pos);
        let result = Value::Boolean(true);
        let updated = Value::Map(Rc::new(data));
        crate::locals::replace_value(receiver, &updated);
        Ok(result)
    } else {
        Ok(Value::Boolean(false))
    }
}

pub(crate) fn map_clear(receiver: Option<&Value>) -> Result<Value, VmError> {
    if !matches!(receiver, Some(Value::Map(data)) if !data.weak) {
        return Err(crate::value::error::throw_type_error(
            "Map method called on incompatible receiver",
        ));
    }
    let updated = Value::Map(Rc::new(MapData {
        weak: false,
        keys: VecDeque::new(),
        values: Vec::new(),
        prototype: std::cell::RefCell::new(None),
    }));
    if let Some(receiver) = receiver {
        crate::locals::replace_value(receiver, &updated);
    }
    Ok(Value::Undefined)
}

pub(crate) fn map_for_each(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let Some(Value::Map(data)) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "Map method called on incompatible receiver",
        ));
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
