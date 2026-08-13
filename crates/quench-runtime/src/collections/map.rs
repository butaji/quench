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

fn canonicalize_key(value: &Value) -> Value {
    match value {
        Value::Number(number) if *number == 0.0 => Value::Number(0.0),
        _ => value.clone(),
    }
}

pub fn property(key: &str) -> Value {
    match key {
        "constructor" => Value::Builtin(Builtin::Map),
        "set" => Value::Builtin(Builtin::MapSet),
        "get" => Value::Builtin(Builtin::MapGet),
        "has" => Value::Builtin(Builtin::MapHas),
        "delete" => Value::Builtin(Builtin::MapDelete),
        "clear" => Value::Builtin(Builtin::MapClear),
        "forEach" => Value::Builtin(Builtin::MapForEach),
        "entries" => Value::Builtin(Builtin::MapEntries),
        "keys" => Value::Builtin(Builtin::MapKeys),
        "values" => Value::Builtin(Builtin::MapValues),
        "getOrInsert" => Value::Builtin(Builtin::MapGetOrInsert),
        "getOrInsertComputed" => Value::Builtin(Builtin::MapGetOrInsertComputed),
        "Symbol.iterator" => Value::Builtin(Builtin::MapEntries),
        "Symbol.toStringTag"
            if !crate::builtins::builtin_prototype_property_is_removed(
                Builtin::MapPrototype,
                "Symbol.toStringTag",
            ) =>
        {
            Value::String("Map".into())
        }
        _ => Value::Undefined,
    }
}

pub(crate) fn map_size(receiver: Option<&Value>) -> Result<Value, VmError> {
    let Some(Value::Map(data)) =
        receiver.filter(|value| matches!(value, Value::Map(data) if !data.weak))
    else {
        return Err(crate::value::error::throw_type_error(
            "Method get size called on incompatible receiver",
        ));
    };
    Ok(Value::Number(data.keys.borrow().len() as f64))
}

pub(crate) fn map_group_by(arguments: &[Value]) -> Result<Value, VmError> {
    let iterable = arguments.first().cloned().unwrap_or(Value::Undefined);
    let callback = arguments.get(1).cloned().unwrap_or(Value::Undefined);
    if !crate::conversion::is_callable(&callback) {
        return Err(crate::value::error::throw_type_error(
            "Map.groupBy callback is not callable",
        ));
    }
    let values = crate::collections::iterator::collect_iterable(iterable)?;
    let mut result = MapData {
        weak: false,
        keys: std::cell::RefCell::new(VecDeque::new()),
        values: std::cell::RefCell::new(Vec::new()),
        prototype: std::cell::RefCell::new(None),
    };
    for (index, value) in values.into_iter().enumerate() {
        let key = crate::functions::execute_target(
            &callback,
            &Value::Undefined,
            &[value.clone(), Value::Number(index as f64)],
        )?;
        let position = result
            .keys
            .borrow()
            .iter()
            .position(|item| same_value_zero(item, &key));
        if let Some(position) = position {
            if let Some(Value::Array(array)) = result.values.get_mut().get_mut(position) {
                let next = array.logical_len();
                Rc::make_mut(array).set_index(next, value);
            }
        } else {
            result.keys.get_mut().push_back(key);
            result.values.get_mut().push(Value::array(vec![value]));
        }
    }
    Ok(Value::Map(Rc::new(result)))
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

pub(crate) fn map_new(arguments: &[Value]) -> Result<Value, VmError> {
    let map = Value::Map(Rc::new(MapData {
        weak: false,
        keys: std::cell::RefCell::new(VecDeque::new()),
        values: std::cell::RefCell::new(Vec::new()),
        prototype: std::cell::RefCell::new(None),
    }));
    let Some(iterable) = arguments.first().cloned() else {
        return Ok(map);
    };
    if matches!(iterable, Value::Undefined | Value::Null) {
        return Ok(map);
    }
    let setter =
        crate::execute::get_property_result(&Value::Builtin(Builtin::MapPrototype), "set")?;
    if !crate::conversion::is_callable(&setter) {
        return Err(crate::value::error::throw_type_error(
            "Map.prototype.set is not callable",
        ));
    }
    crate::collections::iterator::for_each_iterable(iterable, |entry| {
        if !crate::value::is_object(&entry) {
            return Err(crate::value::error::throw_type_error(
                "Iterator value is not an entry object",
            ));
        }
        let key = crate::execute::get_property_result(&entry, "0")?;
        let value = crate::execute::get_property_result(&entry, "1")?;
        crate::functions::execute_target(&setter, &map, &[key, value])?;
        Ok(())
    })?;
    Ok(map)
}

pub(crate) fn weak_map_new(arguments: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Object(iterable)) = arguments.first() {
        return weak_map_from_iterable(Value::Object(iterable.clone()));
    }
    let entries = match arguments.first() {
        None | Some(Value::Undefined | Value::Null) => Vec::new(),
        Some(Value::Array(entries)) => entries.iter().cloned().collect(),
        Some(Value::Object(_)) => crate::collections::iterator::collect_iterable(
            arguments.first().cloned().unwrap_or(Value::Undefined),
        )?,
        Some(_) => {
            return Err(crate::value::error::throw_type_error(
                "WeakMap iterator is not callable",
            ))
        }
    };
    let set = Value::Map(Rc::new(MapData {
        weak: true,
        keys: std::cell::RefCell::new(VecDeque::new()),
        values: std::cell::RefCell::new(Vec::new()),
        prototype: std::cell::RefCell::new(None),
    }));
    if entries.is_empty() {
        return Ok(set);
    }
    populate_weak_map(set, entries)
}

fn weak_map_from_iterable(iterable: Value) -> Result<Value, VmError> {
    let map = std::cell::RefCell::new(Value::Map(Rc::new(MapData {
        weak: true,
        keys: std::cell::RefCell::new(VecDeque::new()),
        values: std::cell::RefCell::new(Vec::new()),
        prototype: std::cell::RefCell::new(None),
    })));
    let setter =
        crate::execute::get_property_result(&Value::Builtin(Builtin::WeakMapPrototype), "set")?;
    if !crate::conversion::is_callable(&setter) {
        return Err(crate::value::error::throw_type_error(
            "WeakMap.prototype.set is not callable",
        ));
    }
    crate::collections::iterator::for_each_iterable(iterable, |entry| {
        if !crate::value::is_object(&entry) {
            return Err(crate::value::error::throw_type_error(
                "Iterator value is not an entry object",
            ));
        }
        let key = crate::execute::get_property_result(&entry, "0")?;
        let value = crate::execute::get_property_result(&entry, "1")?;
        let current = map.borrow().clone();
        let result = crate::functions::execute_target(&setter, &current, &[key, value])?;
        if matches!(result, Value::Map(_)) {
            *map.borrow_mut() = result;
        }
        Ok(())
    })?;
    Ok(map.into_inner())
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
        map_has_inner(receiver, std::slice::from_ref(&key), true)?,
        Value::Boolean(true)
    ) {
        return map_get_inner(receiver, &[key], true);
    }
    let value = arguments.get(1).cloned().unwrap_or(Value::Undefined);
    map_set_inner(receiver, &[key, value.clone()], true)?;
    Ok(value)
}

pub(crate) fn weak_map_set(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let key = weak_key(receiver, arguments.first())?;
    map_set_inner(
        receiver,
        &[key, arguments.get(1).cloned().unwrap_or(Value::Undefined)],
        true,
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
    map_get_inner(receiver, arguments, true)
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
    map_has_inner(receiver, arguments, true)
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
    map_delete_inner(receiver, arguments, true)
}

pub(crate) fn weak_map_get_or_insert_computed(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let key = weak_key(receiver, arguments.first())?;
    let callback = arguments.get(1).ok_or_else(|| {
        crate::value::error::throw_type_error("WeakMap callback must be callable")
    })?;
    if !crate::conversion::is_callable(callback) {
        return Err(crate::value::error::throw_type_error(
            "WeakMap callback must be callable",
        ));
    }
    if matches!(
        map_has_inner(receiver, std::slice::from_ref(&key), true)?,
        Value::Boolean(true)
    ) {
        return map_get_inner(receiver, &[key], true);
    }
    let value =
        crate::functions::execute_target(callback, &Value::Undefined, std::slice::from_ref(&key))?;
    map_set_inner(receiver, &[key, value.clone()], true)?;
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

include!("map_storage.rs");

pub(crate) fn map_set(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    map_set_inner(receiver, arguments, false)
}

pub(crate) fn map_get(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    map_get_inner(receiver, arguments, false)
}

pub(crate) fn map_get_or_insert(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let key = arguments
        .first()
        .map(canonicalize_key)
        .unwrap_or(Value::Undefined);
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

pub(crate) fn map_get_or_insert_computed(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let key = arguments
        .first()
        .map(canonicalize_key)
        .unwrap_or(Value::Undefined);
    let callback = arguments
        .get(1)
        .ok_or_else(|| crate::value::error::throw_type_error("Map callback must be callable"))?;
    if !crate::conversion::is_callable(callback) {
        return Err(crate::value::error::throw_type_error(
            "Map callback must be callable",
        ));
    }
    if matches!(
        map_has(receiver, std::slice::from_ref(&key))?,
        Value::Boolean(true)
    ) {
        return map_get(receiver, &[key]);
    }
    let value =
        crate::functions::execute_target(callback, &Value::Undefined, std::slice::from_ref(&key))?;
    map_set(receiver, &[key, value.clone()])?;
    Ok(value)
}

pub(crate) fn map_has(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    map_has_inner(receiver, arguments, false)
}

pub(crate) fn map_delete(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    map_delete_inner(receiver, arguments, false)
}

pub(crate) fn map_clear(receiver: Option<&Value>) -> Result<Value, VmError> {
    let Some(Value::Map(data)) =
        receiver.filter(|value| matches!(value, Value::Map(data) if !data.weak))
    else {
        return Err(crate::value::error::throw_type_error(
            "Map method called on incompatible receiver",
        ));
    };
    data.keys.borrow_mut().clear();
    data.values.borrow_mut().clear();
    Ok(Value::Undefined)
}

pub(crate) fn map_for_each(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let Some(Value::Map(data)) =
        receiver.filter(|value| matches!(value, Value::Map(data) if !data.weak))
    else {
        return Err(crate::value::error::throw_type_error(
            "Map method called on incompatible receiver",
        ));
    };
    let Some(callback) = arguments.first() else {
        return Err(crate::value::error::throw_type_error(
            "Map callback must be callable",
        ));
    };
    if !crate::conversion::is_callable(callback) {
        return Err(crate::value::error::throw_type_error(
            "Map callback must be callable",
        ));
    };
    let this_arg = arguments.get(1).cloned().unwrap_or(Value::Undefined);
    let map = receiver.cloned().unwrap_or(Value::Undefined);
    let mut index = 0;
    loop {
        let pair = map_pair(data, index);
        let Some((key, value)) = pair else {
            break;
        };
        let args = [value, key, map.clone()];
        crate::functions::execute_target(callback, &this_arg, &args)?;
        let current_key = data.keys.borrow().get(index).cloned();
        if current_key.is_some_and(|current| same_value_zero(&current, &args[1])) {
            index += 1;
        }
    }
    Ok(Value::Undefined)
}

fn map_pair(data: &MapData, index: usize) -> Option<(Value, Value)> {
    let key = data.keys.borrow().get(index).cloned();
    let value = data.values.borrow().get(index).cloned();
    key.zip(value)
}
