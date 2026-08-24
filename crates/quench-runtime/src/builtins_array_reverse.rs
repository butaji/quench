pub(crate) fn array_reverse(
    receiver: Option<&Value>,
) -> Result<Value, crate::execute::VmError> {
    let Some(receiver) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "Array.prototype.reverse called on null or undefined",
        ));
    };
    let mut target = crate::construct::to_object(receiver)?;
    let length = crate::arrays::array_like_length(&target)?;
    if crate::typed_array_ops::is_view(&target)
        && crate::typed_array_prototype::is_out_of_bounds(&target)
    {
        return Ok(target);
    }
    let mut lower = 0usize;
    let mut upper = length.saturating_sub(1);
    while lower < upper {
        let lower_key = lower.to_string();
        let upper_key = upper.to_string();
        let lower_present = crate::with_scope::has_property(&target, &lower_key)?;
        let lower_value = lower_present
            .then(|| crate::execute::get_property_result(&target, &lower_key))
            .transpose()?;
        target = crate::locals::resolved_replacement(target);
        let upper_present = crate::with_scope::has_property(&target, &upper_key)?;
        let upper_value = upper_present
            .then(|| crate::execute::get_property_result(&target, &upper_key))
            .transpose()?;
        match (upper_present, upper_value, lower_present, lower_value) {
            (true, Some(value), true, Some(lower_value)) => {
                target = assign_reverse_property(target, &lower_key, value)?;
                target = assign_reverse_property(target, &upper_key, lower_value)?;
            }
            (true, Some(value), false, None) => {
                target = assign_reverse_property(target, &lower_key, value)?;
                let (updated, _) = delete_reverse_property(target, &upper_key)?;
                target = updated;
            }
            (false, None, true, Some(value)) => {
                let (updated, deleted) = delete_reverse_property(target, &lower_key)?;
                if !deleted {
                    return Err(crate::value::error::throw_type_error(
                        "Cannot delete property during reverse",
                    ));
                }
                target = assign_reverse_property(updated, &upper_key, value)?;
            }
            _ => {}
        }
        target = crate::locals::resolved_replacement(target);
        lower += 1;
        upper -= 1;
    }
    Ok(target)
}

fn assign_reverse_property(
    target: Value,
    key: &str,
    value: Value,
) -> Result<Value, crate::execute::VmError> {
    let updated = crate::properties::assign_set_property(&target, key, value)?;
    crate::locals::replace_value(&target, &updated);
    Ok(updated)
}

fn delete_reverse_property(
    target: Value,
    key: &str,
) -> Result<(Value, bool), crate::execute::VmError> {
    if matches!(target, Value::Proxy(_)) {
        let result = crate::proxy::proxy_delete(&target, key)?;
        return Ok((target, crate::execute::is_truthy(&result)));
    }
    let result = crate::builtins::delete_property(target.clone(), key);
    crate::locals::replace_value(&target, &result.0);
    Ok(result)
}
