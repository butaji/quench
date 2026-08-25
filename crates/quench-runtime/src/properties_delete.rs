pub(crate) fn execute_delete_property(
    registers: &mut crate::register_file::RegisterFile,
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
    let raw_target = crate::execute::read_register(registers, *object)?.clone();
    let target = if crate::module_bindings::is_namespace(&raw_target) {
        crate::locals::resolved_replacement(raw_target.clone())
    } else {
        crate::vm::resolve_global_owner(&raw_target)
            .unwrap_or_else(|| crate::locals::resolved_replacement(raw_target.clone()))
    };
    let target = match target {
        crate::value::Value::ObjectAlias(alias) if alias.target().is_none() => {
            // Top-level `this` aliases are weak COW views. If their replaced
            // storage has been collected, re-anchor the operation to the
            // active realm global before publishing the deletion.
            let raw_global = crate::vm::current_global_object();
            let global = crate::locals::resolved_replacement(raw_global.clone());
            crate::vm::replace_global_object(&raw_global, &global);
            global
        }
        target => target,
    };
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
        let owned = crate::with_scope::has_property(&target, &key)?;
        if owned && *strict {
            return Err(crate::value::error::throw_type_error(
                "Cannot delete module namespace export",
            ));
        }
        crate::execute::write_value(
            registers,
            *dst,
            crate::value::Value::Boolean(!owned),
        );
        return Ok(());
    }
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
    // Publish through the semantic receiver, not only the syntax-level
    // alias.  A top-level `this`/global alias can retain an older COW view;
    // resolving the owner above gives the realm object whose replacement
    // must be visible to subsequent strict name checks.
    crate::vm::synchronize_global_object(registers, &target, &result);
    crate::locals::replace_value(&raw_target, &result);
    crate::execute::write_value(registers, *object, result);
    crate::execute::write_value(registers, *dst, crate::value::Value::Boolean(deleted));
    Ok(())
}
