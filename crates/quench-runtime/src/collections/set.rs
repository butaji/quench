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

fn canonicalize_value(value: &Value) -> Value {
    match value {
        Value::Number(number) if *number == 0.0 => Value::Number(0.0),
        _ => value.clone(),
    }
}

pub fn property(key: &str) -> Value {
    match key {
        "add" => Value::Builtin(Builtin::SetAdd),
        "has" => Value::Builtin(Builtin::SetHas),
        "delete" => Value::Builtin(Builtin::SetDelete),
        "clear" => Value::Builtin(Builtin::SetClear),
        "forEach" => Value::Builtin(Builtin::SetForEach),
        "keys" | "values" => Value::Builtin(Builtin::SetIterator),
        "entries" => Value::Builtin(Builtin::SetEntries),
        "difference" => Value::Builtin(Builtin::SetDifference),
        "intersection" => Value::Builtin(Builtin::SetIntersection),
        "symmetricDifference" => Value::Builtin(Builtin::SetSymmetricDifference),
        "union" => Value::Builtin(Builtin::SetUnion),
        "isDisjointFrom" => Value::Builtin(Builtin::SetIsDisjointFrom),
        "isSubsetOf" => Value::Builtin(Builtin::SetIsSubsetOf),
        "isSupersetOf" => Value::Builtin(Builtin::SetIsSupersetOf),
        "Symbol.iterator" => Value::Builtin(Builtin::SetIterator),
        "Symbol.toStringTag"
            if !crate::builtins::builtin_prototype_property_is_removed(
                Builtin::SetPrototype,
                "Symbol.toStringTag",
            ) =>
        {
            Value::String("Set".into())
        }
        _ => Value::Undefined,
    }
}

pub(crate) fn set_species(receiver: Option<&Value>) -> Result<Value, VmError> {
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

pub(crate) fn set_size(receiver: Option<&Value>) -> Result<Value, VmError> {
    let Some(Value::Set(data)) =
        receiver.filter(|value| matches!(value, Value::Set(data) if !data.weak))
    else {
        return Err(crate::value::error::throw_type_error(
            "Method get size called on incompatible receiver",
        ));
    };
    Ok(Value::Number(data.values.borrow().len() as f64))
}

pub(crate) fn weak_property(key: &str) -> Value {
    match key {
        "add" => Value::Builtin(Builtin::WeakSetAdd),
        "has" => Value::Builtin(Builtin::WeakSetHas),
        "delete" => Value::Builtin(Builtin::WeakSetDelete),
        _ => Value::Undefined,
    }
}

pub(crate) fn set_new(arguments: &[Value]) -> Result<Value, VmError> {
    let set = Value::Set(Rc::new(SetData {
        weak: false,
        values: std::cell::RefCell::new(VecDeque::new()),
        prototype: std::cell::RefCell::new(None),
    }));
    let Some(iterable) = arguments.first().cloned() else {
        return Ok(set);
    };
    if matches!(iterable, Value::Undefined | Value::Null) {
        return Ok(set);
    }
    let adder = crate::execute::get_property_result(&Value::Builtin(Builtin::SetPrototype), "add")?;
    if !crate::conversion::is_callable(&adder) {
        return Err(crate::value::error::throw_type_error(
            "Set.prototype.add is not callable",
        ));
    }
    crate::collections::iterator::for_each_iterable(iterable, |value| {
        crate::functions::execute_target(&adder, &set, &[value])?;
        Ok(())
    })?;
    Ok(set)
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
        values: std::cell::RefCell::new(values),
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
    let set = Value::Set(Rc::new(SetData {
        weak: true,
        values: std::cell::RefCell::new(VecDeque::new()),
        prototype: std::cell::RefCell::new(None),
    }));
    let adder =
        crate::execute::get_property_result(&Value::Builtin(Builtin::WeakSetPrototype), "add")?;
    if !crate::conversion::is_callable(&adder) {
        return Err(crate::value::error::throw_type_error(
            "WeakSet.prototype.add is not callable",
        ));
    }
    crate::collections::iterator::for_each_iterable(iterable, |value| {
        crate::functions::execute_target(&adder, &set, &[value])?;
        Ok(())
    })?;
    Ok(set)
}

fn populate_weak_set(set: Value) -> Result<Value, VmError> {
    let adder =
        crate::execute::get_property_result(&Value::Builtin(Builtin::WeakSetPrototype), "add")?;
    if !crate::conversion::is_callable(&adder) {
        return Err(crate::value::error::throw_type_error(
            "WeakSet.prototype.add is not callable",
        ));
    }
    let values = match &set {
        Value::Set(data) => data.values.borrow().iter().cloned().collect(),
        _ => Vec::new(),
    };
    for value in values {
        crate::functions::execute_target(&adder, &set, &[value])?;
    }
    Ok(set)
}

pub(crate) fn weak_set_add(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let data = require_weak(receiver)?;
    require_object(arguments.first())?;
    set_add_value(data, arguments)
}

pub(crate) fn weak_set_has(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let data = require_weak(receiver)?;
    if let Some(value) = arguments.first() {
        if !is_weakly_holdable(value) {
            return Ok(Value::Boolean(false));
        }
    }
    Ok(set_has_value(data, arguments))
}

pub(crate) fn weak_set_delete(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let data = require_weak(receiver)?;
    if let Some(value) = arguments.first() {
        if !is_weakly_holdable(value) {
            return Ok(Value::Boolean(false));
        }
    }
    Ok(set_delete_value(data, arguments))
}

fn require_weak(receiver: Option<&Value>) -> Result<&Rc<SetData>, VmError> {
    if let Some(Value::Set(data)) = receiver {
        if data.weak {
            return Ok(data);
        }
    }
    Err(crate::value::error::throw_type_error(
        "WeakSet method called on incompatible receiver",
    ))
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

fn require_set(receiver: Option<&Value>) -> Result<&Rc<SetData>, VmError> {
    let Some(Value::Set(data)) =
        receiver.filter(|value| matches!(value, Value::Set(data) if !data.weak))
    else {
        return Err(crate::value::error::throw_type_error(
            "Set method called on incompatible receiver",
        ));
    };
    Ok(data)
}

pub(crate) fn set_add(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let data = require_set(receiver)?;
    set_add_value(data, arguments)
}

fn set_add_value(data: &Rc<SetData>, arguments: &[Value]) -> Result<Value, VmError> {
    let Some(value) = arguments.first() else {
        return Ok(Value::Undefined);
    };
    let mut values = data.values.borrow_mut();
    if !values.iter().any(|v| same_value_zero(v, value)) {
        values.push_back(canonicalize_value(value));
    }
    Ok(Value::Set(Rc::clone(data)))
}

pub(crate) fn set_has(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let data = require_set(receiver)?;
    Ok(set_has_value(data, arguments))
}

fn set_has_value(data: &Rc<SetData>, arguments: &[Value]) -> Value {
    let Some(value) = arguments.first() else {
        return Value::Boolean(false);
    };
    Value::Boolean(
        data.values
            .borrow()
            .iter()
            .any(|v| same_value_zero(v, value)),
    )
}

pub(crate) fn set_delete(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let data = require_set(receiver)?;
    Ok(set_delete_value(data, arguments))
}

fn set_delete_value(data: &Rc<SetData>, arguments: &[Value]) -> Value {
    let Some(value) = arguments.first() else {
        return Value::Boolean(false);
    };
    let mut values = data.values.borrow_mut();
    if let Some(pos) = values.iter().position(|v| same_value_zero(v, value)) {
        values.remove(pos);
        Value::Boolean(true)
    } else {
        Value::Boolean(false)
    }
}

pub(crate) fn set_clear(receiver: Option<&Value>) -> Result<Value, VmError> {
    let data = require_set(receiver)?;
    data.values.borrow_mut().clear();
    Ok(Value::Undefined)
}

pub(crate) fn set_for_each(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let Some(Value::Set(data)) =
        receiver.filter(|value| matches!(value, Value::Set(data) if !data.weak))
    else {
        return Err(crate::value::error::throw_type_error(
            "Set method called on incompatible receiver",
        ));
    };
    let Some(callback) = arguments.first() else {
        return Ok(Value::Undefined);
    };
    let this_arg = arguments.get(1).cloned().unwrap_or(Value::Undefined);
    let set = receiver.cloned().unwrap_or(Value::Undefined);
    let mut index = 0;
    loop {
        let Some(value) = data.values.borrow().get(index).cloned() else {
            break;
        };
        let args = [value.clone(), value, set.clone()];
        crate::functions::execute_target(callback, &this_arg, &args)?;
        let still_at_index = data
            .values
            .borrow()
            .get(index)
            .is_some_and(|current| same_value_zero(current, &args[0]));
        if still_at_index {
            index += 1;
        }
    }
    Ok(Value::Undefined)
}
