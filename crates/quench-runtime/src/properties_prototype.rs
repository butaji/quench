pub(crate) fn execute_set_prototype(
    registers: &mut Vec<crate::value::Value>,
    op: &Op,
) -> Result<(), crate::execute::VmError> {
    let Op::SetPrototype { object, prototype } = op else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    let target = crate::execute::read_register(registers, *object)?.clone();
    let prototype = crate::execute::read_register(registers, *prototype)?.clone();
    if !matches!(prototype, crate::value::Value::Null | crate::value::Value::Object(_)) {
        return Ok(());
    }
    let updated = crate::builtins::set_property(target.clone(), "\0prototype", prototype);
    crate::locals::replace_value(&target, &updated);
    crate::vm::synchronize_global_object(registers, &target, &updated);
    crate::execute::write_value(registers, *object, updated);
    Ok(())
}

fn property_key(value: &crate::ops::Constant) -> Option<String> {
    match value {
        crate::ops::Constant::String(value) => Some(value.clone()),
        crate::ops::Constant::Number(value) => Some(value.to_string()),
        _ => None,
    }
}
pub(crate) fn propagate_updated_object(
    registers: &mut Vec<crate::value::Value>,
    argument: Option<u16>,
    old: &crate::value::Value,
    new: &crate::value::Value,
) {
    crate::locals::replace_value(old, new);
    crate::vm::synchronize_global_object(registers, old, new);
    if let Some(argument) = argument {
        crate::execute::write_value(registers, argument, new.clone());
    }
}
