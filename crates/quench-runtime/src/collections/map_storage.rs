fn map_set_inner(
    receiver: Option<&Value>,
    arguments: &[Value],
    allow_weak: bool,
) -> Result<Value, VmError> {
    let Some(receiver @ Value::Map(data)) =
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
        .iter()
        .position(|k| same_value_zero(k, key))
        .and_then(|pos| data.values.get(pos).cloned())
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
        data.keys.iter().any(|k| same_value_zero(k, key)),
    ))
}

fn map_delete_inner(
    receiver: Option<&Value>,
    arguments: &[Value],
    allow_weak: bool,
) -> Result<Value, VmError> {
    let Some(receiver @ Value::Map(data)) =
        receiver.filter(|value| matches!(value, Value::Map(data) if allow_weak || !data.weak))
    else {
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
