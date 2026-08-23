pub(crate) fn is_extensible_value(
    target: Option<&crate::value::Value>,
) -> Result<crate::value::Value, crate::execute::VmError> {
    let target = target.ok_or(crate::execute::VmError::NotCallable)?;
    if matches!(target, crate::value::Value::Proxy(_)) {
        return crate::proxy::proxy_is_extensible(target);
    }
    Ok(crate::value::Value::Boolean(object_is_extensible(target)))
}

include!("properties_integrity.rs");

pub(crate) fn prevent_extensions(
    target: Option<&crate::value::Value>,
) -> Result<crate::value::Value, crate::execute::VmError> {
    let Some(target) = target else {
        return Err(crate::value::error::throw_type_error("Object expected"));
    };
    if let crate::value::Value::BindingCell(cell) = target {
        let current = cell.borrow().clone();
        let updated = prevent_extensions(Some(&current))?;
        *cell.borrow_mut() = updated;
        return Ok(target.clone());
    }
    if matches!(target, crate::value::Value::Proxy(_)) {
        return crate::proxy::proxy_prevent_extensions(target);
    }
    let result = mark_non_extensible(target);
    crate::locals::replace_value(target, &result);
    if crate::vm::is_global_object(target) {
        let mut registers = Vec::new();
        crate::vm::synchronize_global_object(&mut registers, target, &result);
    }
    Ok(result)
}

fn mark_non_extensible(target: &crate::value::Value) -> crate::value::Value {
    match target {
        crate::value::Value::Object(properties) => {
            let mut sealed = properties.as_ref().clone();
            push_non_extensible(&mut sealed);
            let next = crate::value::Value::Object(std::rc::Rc::new(sealed));
            crate::module_bindings::rehome_evaluator(target, &next);
            next
        }
        crate::value::Value::Array(values) => {
            let mut values = std::rc::Rc::clone(values);
            std::rc::Rc::make_mut(&mut values)
                .set_property(NON_EXTENSIBLE, crate::value::Value::Boolean(true));
            crate::value::Value::Array(values)
        }
        crate::value::Value::Function(function) => {
            mark_properties(&mut function.properties.borrow_mut());
            target.clone()
        }
        crate::value::Value::BoundFunction(bound) => {
            mark_properties(&mut bound.properties.borrow_mut());
            target.clone()
        }
        _ => target.clone(),
    }
}

fn mark_properties(properties: &mut Vec<(String, crate::value::Value)>) {
    if !properties.iter().any(|(name, _)| name == NON_EXTENSIBLE) {
        properties.push((
            NON_EXTENSIBLE.to_string(),
            crate::value::Value::Boolean(true),
        ));
    }
}

fn reject_restricted_property_write(
    target: &crate::value::Value,
    key: &str,
) -> Result<(), crate::execute::VmError> {
    if matches!(&target, crate::value::Value::Array(values) if values.is_strict_arguments() && key == "callee")
    {
        return Err(crate::value::error::throw_type_error(
            "'callee' is unavailable on strict arguments",
        ));
    }
    if crate::vm::has_restricted_function_property(target, key) {
        return Err(crate::value::error::throw_type_error(
            "'caller' and 'arguments' are unavailable on this function",
        ));
    }
    Ok(())
}

fn inherited_write_blocked(target: &crate::value::Value, key: &str) -> bool {
    // Prototype objects do not truly own `length`/`name`; assigning them
    // creates an own property that shadows the callable metadata.
    let prototype_meta_key = matches!(key, "length" | "name")
        && matches!(target, crate::value::Value::Builtin(builtin) if crate::builtin_meta::is_prototype(*builtin));
    if !prototype_meta_key
        && crate::builtins::descriptor_flag(target, key, "writable") == Some(false)
    {
        return true;
    }
    matches!(
        crate::property_define::accessor(target, key, "writable"),
        Some(crate::value::Value::Boolean(false))
    )
}
fn write_failure(strict: bool) -> Result<(), crate::execute::VmError> {
    if strict {
        return Err(crate::value::error::throw_type_error(
            "Cannot assign to read-only property",
        ));
    }
    Ok(())
}

include!("properties_assign.rs");
include!("properties_copy_data.rs");
include!("properties_reflect_set.rs");

include!("properties_delete.rs");
include!("properties_methods.rs");
include!("properties_prototype.rs");
