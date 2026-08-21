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
    crate::module_bindings::exports(&target, &key)?;
    if crate::module_bindings::is_namespace(&target) {
        return delete_namespace_property(registers, *dst, &target, &key, *strict);
    }
    if matches!(target, crate::value::Value::Proxy(_)) {
        return delete_proxy_property(registers, *dst, &target, &key, *strict);
    }
    return delete_regular_property(registers, *object, *dst, &target, &key, *strict);
}

fn delete_regular_property(
    registers: &mut Vec<crate::value::Value>,
    object: u16,
    dst: u16,
    target: &crate::value::Value,
    key: &str,
    strict: bool,
) -> Result<(), crate::execute::VmError> {
    let (result, deleted) = crate::builtins::delete_property(target.clone(), key);
    if !deleted && strict {
        return Err(crate::value::error::throw_type_error(
            "Cannot delete non-configurable property",
        ));
    }
    crate::locals::replace_value(target, &result);
    crate::vm::synchronize_global_object(registers, target, &result);
    crate::execute::write_value(registers, object, result);
    crate::execute::write_value(registers, dst, crate::value::Value::Boolean(deleted));
    Ok(())
}
fn delete_namespace_property(
    registers: &mut Vec<crate::value::Value>,
    dst: u16,
    target: &crate::value::Value,
    key: &str,
    strict: bool,
) -> Result<(), crate::execute::VmError> {
    let owned = crate::with_scope::has_property(target, key)?;
    if owned && strict {
        return Err(crate::value::error::throw_type_error(
            "Cannot delete module namespace export",
        ));
    }
    crate::execute::write_value(
        registers,
        dst,
        crate::value::Value::Boolean(!owned),
    );
    Ok(())
}
