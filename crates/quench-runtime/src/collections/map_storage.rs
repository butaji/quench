fn map_set_inner(
    receiver: Option<&Value>,
    arguments: &[Value],
    allow_weak: bool,
) -> Result<Value, VmError> {
    let Some(Value::Map(data)) =
        receiver.filter(|value| matches!(value, Value::Map(data) if allow_weak || !data.weak))
    else {
        return Err(crate::value::error::throw_type_error(
            "Map method called on incompatible receiver",
        ));
    };
    let Some(key) = arguments.first() else {
        return Ok(Value::Undefined);
    };
    let value = arguments.get(1).cloned().unwrap_or(Value::Undefined);
    let keys = data.keys.borrow();
    if let Some(pos) = keys.iter().position(|k| same_value_zero(k, key)) {
        drop(keys);
        data.values.borrow_mut()[pos] = value;
    } else {
        drop(keys);
        data.keys.borrow_mut().push_back(key.clone());
        data.values.borrow_mut().push(value);
    }
    Ok(Value::Map(Rc::clone(data)))
}

fn map_get_inner(
    receiver: Option<&Value>,
    arguments: &[Value],
    allow_weak: bool,
) -> Result<Value, VmError> {
    let Some(Value::Map(data)) =
        receiver.filter(|value| matches!(value, Value::Map(data) if allow_weak || !data.weak))
    else {
        return Err(crate::value::error::throw_type_error(
            "Map method called on incompatible receiver",
        ));
    };
    let Some(key) = arguments.first() else {
        return Ok(Value::Undefined);
    };
    Ok(data
        .keys
        .borrow()
        .iter()
        .position(|k| same_value_zero(k, key))
        .and_then(|pos| data.values.borrow().get(pos).cloned())
        .unwrap_or(Value::Undefined))
}

fn map_has_inner(
    receiver: Option<&Value>,
    arguments: &[Value],
    allow_weak: bool,
) -> Result<Value, VmError> {
    let Some(Value::Map(data)) =
        receiver.filter(|value| matches!(value, Value::Map(data) if allow_weak || !data.weak))
    else {
        return Err(crate::value::error::throw_type_error(
            "Map method called on incompatible receiver",
        ));
    };
    let Some(key) = arguments.first() else {
        return Ok(Value::Boolean(false));
    };
    Ok(Value::Boolean(
        data.keys.borrow().iter().any(|k| same_value_zero(k, key)),
    ))
}

fn map_delete_inner(
    receiver: Option<&Value>,
    arguments: &[Value],
    allow_weak: bool,
) -> Result<Value, VmError> {
    let Some(Value::Map(data)) =
        receiver.filter(|value| matches!(value, Value::Map(data) if allow_weak || !data.weak))
    else {
        return Err(crate::value::error::throw_type_error(
            "Map method called on incompatible receiver",
        ));
    };
    let Some(key) = arguments.first() else {
        return Ok(Value::Boolean(false));
    };
    let pos = data
        .keys
        .borrow()
        .iter()
        .position(|k| same_value_zero(k, key));
    if let Some(pos) = pos {
        data.keys.borrow_mut().remove(pos);
        data.values.borrow_mut().remove(pos);
        Ok(Value::Boolean(true))
    } else {
        Ok(Value::Boolean(false))
    }
}
