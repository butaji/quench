pub(crate) fn array_unshift(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let Some(receiver) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "Array.prototype.unshift called on null or undefined",
        ));
    };
    let receiver = crate::construct::to_object(receiver)?;
    let length = crate::builtins::map_length(&receiver)?;
    let argument_count = arguments.len();
    let new_length = length.checked_add(argument_count).ok_or_else(|| {
        crate::value::error::throw_range_error("Invalid array length")
    })?;
    if (new_length as u64) > 9_007_199_254_740_991u64 {
        return Err(crate::value::error::throw_type_error("Invalid array length"));
    }
    let mut target = crate::locals::resolved_replacement(receiver.clone());
    if argument_count > 0 {
        for index in (1..=length).rev() {
            let from = (index - 1).to_string();
            let to = (index + argument_count - 1).to_string();
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
                        "Cannot delete property during unshift",
                    ));
                }
                crate::locals::replace_value(&previous, &updated);
                target = updated;
            }
        }
        for (index, value) in arguments.iter().cloned().enumerate() {
            let updated = crate::properties::assign_set_property(&target, &index.to_string(), value)?;
            crate::locals::replace_value(&target, &updated);
            target = updated;
        }
    }
    target = crate::locals::resolved_replacement(target);
    let updated = crate::properties::assign_set_property(
        &target,
        "length",
        Value::Number(new_length as f64),
    )?;
    crate::locals::replace_value(&receiver, &updated);
    Ok(Value::Number(new_length as f64))
}
