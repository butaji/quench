//! Set builtin — constructor and instance methods.

use std::collections::VecDeque;
use std::rc::Rc;

use crate::{
    execute::VmError,
    ops::Builtin,
    value::{SetData, Value},
};
include!("set_relations.rs");

fn same_value_zero(left: &Value, right: &Value) -> bool {
    crate::builtins::same_value_zero(left, right)
}

pub fn property(key: &str) -> Value {
    match key {
        "add" => Value::Builtin(Builtin::SetAdd),
        "has" => Value::Builtin(Builtin::SetHas),
        "delete" => Value::Builtin(Builtin::SetDelete),
        "clear" => Value::Builtin(Builtin::SetClear),
        "forEach" => Value::Builtin(Builtin::SetForEach),
        "keys" | "values" => Value::Builtin(Builtin::SetIterator),
        "Symbol.iterator" => Value::Builtin(Builtin::SetIterator),
        _ => Value::Undefined,
    }
}

pub(crate) fn weak_property(key: &str) -> Value {
    match key {
        "add" => Value::Builtin(Builtin::WeakSetAdd),
        "has" => Value::Builtin(Builtin::WeakSetHas),
        "delete" => Value::Builtin(Builtin::WeakSetDelete),
        _ => Value::Undefined,
    }
}

pub(crate) fn set_new(arguments: &[Value]) -> Value {
    let values = match arguments.first() {
        Some(Value::Undefined | Value::Null) => VecDeque::new(),
        Some(Value::Array(values)) => values.iter().cloned().collect(),
        _ => VecDeque::new(),
    };
    Value::Set(Rc::new(SetData {
        weak: false,
        values,
        prototype: std::cell::RefCell::new(None),
    }))
}

pub(crate) fn weak_set_new(arguments: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Object(iterable)) = arguments.first() {
        return weak_set_from_iterable(Value::Object(iterable.clone()));
    }
    let values = match arguments.first() {
        None | Some(Value::Undefined | Value::Null) => VecDeque::new(),
        Some(Value::Array(values)) => values.iter().cloned().collect(),
        Some(_) => {
            return Err(crate::value::error::throw_type_error(
                "WeakSet iterator is not callable",
            ))
        }
    };
    if values.iter().any(|value| !is_weakly_holdable(value)) {
        return Err(crate::value::error::throw_type_error(
            "Invalid value used in weak set",
        ));
    }
    let set = Value::Set(Rc::new(SetData {
        weak: true,
        values,
        prototype: std::cell::RefCell::new(None),
    }));
    if matches!(
        arguments.first(),
        None | Some(Value::Undefined | Value::Null)
    ) {
        return Ok(set);
    }
    populate_weak_set(set)
}

fn weak_set_from_iterable(iterable: Value) -> Result<Value, VmError> {
    let set = std::cell::RefCell::new(Value::Set(Rc::new(SetData {
        weak: true,
        values: VecDeque::new(),
        prototype: std::cell::RefCell::new(None),
    })));
    let adder =
        crate::execute::get_property_result(&Value::Builtin(Builtin::WeakSetPrototype), "add")?;
    if !crate::conversion::is_callable(&adder) {
        return Err(crate::value::error::throw_type_error(
            "WeakSet.prototype.add is not callable",
        ));
    }
    crate::collections::iterator::for_each_iterable(iterable, |value| {
        let current = set.borrow().clone();
        let result = crate::functions::execute_target(&adder, &current, &[value])?;
        if matches!(result, Value::Set(_)) {
            *set.borrow_mut() = result;
        }
        Ok(())
    })?;
    Ok(set.into_inner())
}

fn populate_weak_set(mut set: Value) -> Result<Value, VmError> {
    let adder =
        crate::execute::get_property_result(&Value::Builtin(Builtin::WeakSetPrototype), "add")?;
    if !crate::conversion::is_callable(&adder) {
        return Err(crate::value::error::throw_type_error(
            "WeakSet.prototype.add is not callable",
        ));
    }
    let values = match &set {
        Value::Set(data) => data.values.iter().cloned().collect(),
        _ => Vec::new(),
    };
    for value in values {
        let result = crate::functions::execute_target(&adder, &set, &[value])?;
        if matches!(result, Value::Set(_)) {
            set = result;
        }
    }
    Ok(set)
}

pub(crate) fn weak_set_add(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    require_weak(receiver)?;
    require_object(arguments.first())?;
    set_add(receiver, arguments)
}

pub(crate) fn weak_set_has(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    require_weak(receiver)?;
    if let Some(value) = arguments.first() {
        if !is_weakly_holdable(value) {
            return Ok(Value::Boolean(false));
        }
    }
    set_has(receiver, arguments)
}

pub(crate) fn weak_set_delete(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    require_weak(receiver)?;
    if let Some(value) = arguments.first() {
        if !is_weakly_holdable(value) {
            return Ok(Value::Boolean(false));
        }
    }
    set_delete(receiver, arguments)
}

fn require_weak(receiver: Option<&Value>) -> Result<(), VmError> {
    if matches!(receiver, Some(Value::Set(data)) if data.weak) {
        Ok(())
    } else {
        Err(crate::value::error::throw_type_error(
            "WeakSet method called on incompatible receiver",
        ))
    }
}

fn require_object(value: Option<&Value>) -> Result<(), VmError> {
    if value.is_some_and(is_weakly_holdable) {
        Ok(())
    } else {
        Err(crate::value::error::throw_type_error(
            "Invalid value used in weak set",
        ))
    }
}

fn is_weakly_holdable(value: &Value) -> bool {
    crate::value::is_object(value)
        || matches!(value, Value::String(text) if text.starts_with("Symbol.") && !text.starts_with("Symbol.for.") && text.contains('\0'))
}

pub(crate) fn set_add(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let Some(receiver @ Value::Set(data)) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "Set method called on incompatible receiver",
        ));
    };
    let Some(value) = arguments.first() else {
        return Ok(Value::Undefined);
    };
    let mut data = (**data).clone();
    if !data.values.iter().any(|v| same_value_zero(v, value)) {
        data.values.push_back(value.clone());
    }
    let result = Value::Set(Rc::new(data));
    crate::locals::replace_value(receiver, &result);
    Ok(result)
}

pub(crate) fn set_has(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::Set(data)) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "Set method called on incompatible receiver",
        ));
    };
    let Some(value) = arguments.first() else {
        return Ok(Value::Boolean(false));
    };
    Ok(Value::Boolean(
        data.values.iter().any(|v| same_value_zero(v, value)),
    ))
}

pub(crate) fn set_delete(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let Some(receiver @ Value::Set(data)) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "Set method called on incompatible receiver",
        ));
    };
    let Some(value) = arguments.first() else {
        return Ok(Value::Boolean(false));
    };
    let mut data = (**data).clone();
    if let Some(pos) = data.values.iter().position(|v| same_value_zero(v, value)) {
        data.values.remove(pos);
        let result = Value::Boolean(true);
        let updated = Value::Set(Rc::new(data));
        crate::locals::replace_value(receiver, &updated);
        Ok(result)
    } else {
        Ok(Value::Boolean(false))
    }
}

pub(crate) fn set_clear(receiver: Option<&Value>) -> Result<Value, VmError> {
    if !matches!(receiver, Some(Value::Set(_))) {
        return Err(crate::value::error::throw_type_error(
            "Set method called on incompatible receiver",
        ));
    }
    let updated = Value::Set(Rc::new(SetData {
        weak: false,
        values: VecDeque::new(),
        prototype: std::cell::RefCell::new(None),
    }));
    if let Some(receiver) = receiver {
        crate::locals::replace_value(receiver, &updated);
    }
    Ok(Value::Undefined)
}

pub(crate) fn set_for_each(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let Some(Value::Set(data)) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "Set method called on incompatible receiver",
        ));
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
