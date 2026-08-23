fn set_builtin_property(
    registers: &mut Vec<crate::value::Value>,
    object: u16,
    target: &crate::value::Value,
    key: &str,
    value: crate::value::Value,
) -> Result<(), crate::execute::VmError> {
    // Assignment to an existing property updates only its value; attributes
    // are preserved by complete_descriptor. New properties get the
    // assignment defaults (writable, enumerable, configurable).
    let key_value = crate::value::Value::String(key.to_string());
    let existing = crate::builtins::object::descriptor(Some(target), Some(&key_value))?;
    let exists = !matches!(existing, crate::value::Value::Undefined);
    let mut fields = vec![("value".to_string(), value)];
    if !exists {
        for name in ["writable", "enumerable", "configurable"] {
            fields.push((name.to_string(), crate::value::Value::Boolean(true)));
        }
    }
    let properties = std::rc::Rc::new(crate::value::ObjectData::new(fields));
    let updated = crate::builtins::define_own_property(target, key, properties.as_ref())?;
    crate::execute::write_value(registers, object, updated);
    Ok(())
}

pub(crate) fn rejects_new_property(target: &crate::value::Value, key: &str) -> bool {
    match target {
        crate::value::Value::Object(properties) => marked_without_key(properties, key),
        crate::value::Value::Function(function) => {
            let properties = function.properties.borrow();
            marked_without_key(&properties, key)
        }
        crate::value::Value::BoundFunction(bound) => {
            let properties = bound.properties.borrow();
            marked_without_key(&properties, key)
        }
        crate::value::Value::Array(values) => {
            let own = key == "length"
                || crate::arrays::array_index(key)
                    .is_some_and(|index| values.has_index(index as usize))
                || values.property(key).is_some();
            values.property(NON_EXTENSIBLE).is_some() && !own
        }
        _ => false,
    }
}

fn marked_without_key(properties: &[(String, crate::value::Value)], key: &str) -> bool {
    properties.iter().any(|(name, _)| name == NON_EXTENSIBLE)
        && !properties.iter().any(|(name, _)| name == key)
}

pub(crate) fn object_is_extensible(target: &crate::value::Value) -> bool {
    let target = crate::locals::resolved_replacement(target.clone());
    if let crate::value::Value::BindingCell(cell) = &target {
        return object_is_extensible(&cell.borrow());
    }
    match &target {
        crate::value::Value::Builtin(crate::ops::Builtin::ThrowTypeError) => false,
        crate::value::Value::Object(properties) => {
            !properties.iter().any(|(name, _)| name == NON_EXTENSIBLE)
        }
        crate::value::Value::Array(values) => values.property(NON_EXTENSIBLE).is_none(),
        crate::value::Value::Function(function) => !function
            .properties
            .borrow()
            .iter()
            .any(|(name, _)| name == NON_EXTENSIBLE),
        crate::value::Value::BoundFunction(bound) => !bound
            .properties
            .borrow()
            .iter()
            .any(|(name, _)| name == NON_EXTENSIBLE),
        value => crate::value::is_object(value),
    }
}