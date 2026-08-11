pub(crate) fn set_relation(
    builtin: Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let Some(Value::Set(data)) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "Set method called on incompatible receiver",
        ));
    };
    if data.weak {
        return Err(crate::value::error::throw_type_error(
            "Set method called on incompatible receiver",
        ));
    }
    let other = arguments.first().cloned().unwrap_or(Value::Undefined);
    let values = set_like_values(other)?;
    let contains = |value: &Value| values.iter().any(|item| same_value_zero(item, value));
    let own: Vec<Value> = data.values.iter().cloned().collect();
    match builtin {
        Builtin::SetDifference => Ok(new_set(own.into_iter().filter(|v| !contains(v)).collect())),
        Builtin::SetIntersection => Ok(new_set(own.into_iter().filter(contains).collect())),
        Builtin::SetSymmetricDifference => symmetric_difference(&own, values.clone(), &contains),
        Builtin::SetUnion => Ok(new_set(union(own, values))),
        Builtin::SetIsDisjointFrom => Ok(Value::Boolean(own.iter().all(|v| !contains(v)))),
        Builtin::SetIsSubsetOf => Ok(Value::Boolean(own.iter().all(contains))),
        Builtin::SetIsSupersetOf => Ok(Value::Boolean(
            values
                .iter()
                .all(|v| own.iter().any(|item| same_value_zero(item, v))),
        )),
        _ => Err(VmError::MissingReturn),
    }
}

fn set_like_values(value: Value) -> Result<Vec<Value>, VmError> {
    if let Value::Set(data) = &value {
        return Ok(data.values.iter().cloned().collect());
    }
    let size = crate::execute::get_property_result(&value, "size")?;
    if !matches!(size, Value::Number(number) if number.is_finite() && number >= 0.0) {
        return Err(crate::value::error::throw_type_error(
            "Set-like size must be a non-negative number",
        ));
    }
    let keys = crate::execute::get_property_result(&value, "keys")?;
    if !crate::conversion::is_callable(&keys) {
        return Err(crate::value::error::throw_type_error(
            "Set-like keys is not callable",
        ));
    }
    let iterator = crate::functions::execute_target(&keys, &value, &[])?;
    crate::collections::iterator::collect_iterable(iterator)
}

fn symmetric_difference(
    own: &[Value],
    values: Vec<Value>,
    contains: &impl Fn(&Value) -> bool,
) -> Result<Value, VmError> {
    let mut result: Vec<Value> = own.iter().filter(|v| !contains(v)).cloned().collect();
    result.extend(
        values
            .into_iter()
            .filter(|v| !own.iter().any(|x| same_value_zero(x, v))),
    );
    Ok(new_set(result))
}

fn union(mut own: Vec<Value>, values: Vec<Value>) -> Vec<Value> {
    for value in values {
        if !own.iter().any(|item| same_value_zero(item, &value)) {
            own.push(value);
        }
    }
    own
}

fn new_set(values: Vec<Value>) -> Value {
    Value::Set(Rc::new(SetData {
        weak: false,
        values: values.into_iter().collect(),
        prototype: std::cell::RefCell::new(None),
    }))
}
