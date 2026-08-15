pub(crate) fn execute_delete_property(
    registers: &mut Vec<crate::value::Value>,
    op: &Op,
) -> Result<(), crate::execute::VmError> {
    let Op::DeleteProperty {
        dst,
        object,
        key,
        strict,
    } = op
    else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    let target = crate::execute::read_register(registers, *object)?.clone();
    if matches!(
        target,
        crate::value::Value::Null | crate::value::Value::Undefined
    ) {
        return Err(crate::value::error::throw_type_error(
            "Cannot delete a property of null or undefined",
        ));
    }
    let key = dynamic_property_key(&crate::execute::read_register(registers, *key)?)?;
    if matches!(target, crate::value::Value::Proxy(_)) {
        return delete_proxy_property(registers, *dst, &target, &key, *strict);
    }
    let (result, deleted) = crate::builtins::delete_property(target.clone(), &key);
    if !deleted && *strict {
        return Err(crate::value::error::throw_type_error(
            "Cannot delete non-configurable property",
        ));
    }
    crate::locals::replace_value(&target, &result);
    crate::vm::synchronize_global_object(registers, &target, &result);
    crate::execute::write_value(registers, *object, result);
    crate::execute::write_value(registers, *dst, crate::value::Value::Boolean(deleted));
    Ok(())
}
