pub(crate) fn array_shift(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    let Some(receiver) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "Array.prototype.shift called on null or undefined",
        ));
    };
    let receiver = crate::construct::to_object(receiver)?;
    let length = crate::builtins::map_length(&receiver)?;
    if length == 0 {
        let updated =
            crate::properties::assign_set_property(&receiver, "length", Value::Number(0.0))?;
        crate::locals::replace_value(&receiver, &updated);
        return Ok(Value::Undefined);
    }
    if let Value::Array(array) = &receiver {
        if array.is_packed_ordinary() && array.get_index(0).is_some() {
            let first = array.first().unwrap_or(Value::Undefined);
            let mut values = array.snapshot();
            values.remove(0);
            let updated = Value::array(values);
            crate::locals::replace_value(&receiver, &updated);
            return Ok(first);
        }
    }
    let first = match &receiver {
        Value::Array(array) if array.get_index(0).is_some() => {
            array.first().unwrap_or(Value::Undefined)
        }
        _ => crate::execute::get_property_result(&receiver, "0")?,
    };
    let mut target = crate::locals::resolved_replacement(receiver.clone());
    for index in 1..length {
        let from = index.to_string();
        let to = (index - 1).to_string();
        if crate::with_scope::has_property(&target, &from)? {
            let value = crate::execute::get_property_result(&target, &from)?;
            let updated = crate::properties::assign_set_property(&target, &to, value)?;
            crate::locals::replace_value(&target, &updated);
            target = updated;
        } else {
            let previous = target.clone();
            let (updated, deleted) = crate::builtins::delete_property(target, &to);
            if !deleted {
                return Err(crate::value::error::throw_type_error(
                    "Cannot delete property during shift",
                ));
            }
            crate::locals::replace_value(&previous, &updated);
            target = updated;
        }
    }
    let last = (length - 1).to_string();
    let previous = target.clone();
    let (updated, deleted) = crate::builtins::delete_property(target, &last);
    if !deleted {
        return Err(crate::value::error::throw_type_error(
            "Cannot delete property during shift",
        ));
    }
    crate::locals::replace_value(&previous, &updated);
    target = updated;
    let updated = crate::properties::assign_set_property(
        &target,
        "length",
        Value::Number((length - 1) as f64),
    )?;
    crate::locals::replace_value(&receiver, &updated);
    Ok(first)
}
