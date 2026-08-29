pub(crate) fn array_pop(
    receiver: Option<&Value>,
) -> Result<Value, crate::execute::VmError> {
    let receiver = crate::construct::to_object(receiver.unwrap_or(&Value::Undefined))?;
    let length = crate::builtins::map_length(&receiver)?;
    if length == 0 {
        let updated = crate::properties::assign_set_property(
            &receiver,
            "length",
            Value::Number(0.0),
        )?;
        crate::locals::replace_value(&receiver, &updated);
        return Ok(Value::Undefined);
    }
    let index = length - 1;
    let value = crate::execute::get_property_result(&receiver, &index.to_string())?;
    let target = crate::locals::resolved_replacement(receiver.clone());
    let (updated, deleted) = crate::builtins::delete_property(target, &index.to_string());
    if !deleted {
        return Err(crate::value::error::throw_type_error(
            "Cannot delete property",
        ));
    }
    let updated = crate::properties::assign_set_property(
        &crate::locals::resolved_replacement(updated),
        "length",
        Value::Number(index as f64),
    )?;
    crate::locals::replace_value(&receiver, &updated);
    Ok(value)
}
