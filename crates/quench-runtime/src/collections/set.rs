//! Set builtin — constructor and instance methods.

use std::collections::VecDeque;
use std::rc::Rc;

use crate::{
    execute::VmError,
    ops::Builtin,
    value::{SetData, Value},
};

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
        Some(Value::Array(values)) => values.iter().cloned().collect(),
        _ => VecDeque::new(),
    };
    Value::Set(Rc::new(SetData {
        weak: false,
        values,
        prototype: std::cell::RefCell::new(None),
    }))
}

pub(crate) fn weak_set_new(arguments: &[Value]) -> Value {
    let mut value = set_new(arguments);
    if let Value::Set(data) = &mut value {
        Rc::make_mut(data).weak = true;
    }
    value
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
