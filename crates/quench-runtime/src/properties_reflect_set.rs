pub(crate) fn set_with_receiver(
    target: &crate::value::Value,
    key: &str,
    value: &crate::value::Value,
    receiver: &crate::value::Value,
) -> Result<bool, crate::execute::VmError> {
    if !crate::value::is_object(receiver) {
        return Ok(false);
    }
    if crate::module_bindings::is_namespace(target)
        || crate::module_bindings::is_namespace(receiver)
    {
        return Ok(false);
    }
    let resolved_target = crate::locals::resolved_replacement(target.clone());
    if set_proven_own_data(&resolved_target, key, value, receiver) {
        return Ok(true);
    }
    if key == "length" && crate::regexp::has_regexp_internal_slot(&resolved_target) {
        return set_receiver_data(receiver, key, value);
    }
    let descriptor = inherited_descriptor(target, key)?;
    match descriptor {
        Some(descriptor) if descriptor_field_exists(&descriptor, "set") => {
            call_setter(&descriptor, receiver, value)
        }
        Some(descriptor) if !descriptor_writable(&descriptor)? => Ok(false),
        _ => set_receiver_data(receiver, key, value),
    }
}

fn set_proven_own_data(
    target: &crate::value::Value,
    key: &str,
    value: &crate::value::Value,
    receiver: &crate::value::Value,
) -> bool {
    let (crate::value::Value::Object(target), crate::value::Value::Object(receiver)) =
        (target, receiver)
    else {
        return false;
    };
    if !std::rc::Rc::ptr_eq(target, receiver) || !plain_writable_own_data(target, key) {
        return false;
    }
    let updated = crate::builtins::object_alias::set(
        std::rc::Rc::clone(target),
        key,
        value.clone(),
    );
    crate::locals::replace_value(&crate::value::Value::Object(std::rc::Rc::clone(target)), &updated);
    true
}

fn plain_writable_own_data(properties: &crate::value::ObjectData, key: &str) -> bool {
    let deleted = crate::builtins::deleted_key(key);
    let descriptor = crate::builtins::descriptor_key(key);
    let mut own = None;
    let mut metadata = None;
    for (name, value) in properties.iter().rev() {
        if name == &deleted {
            return false;
        }
        if own.is_none() && name == key {
            own = Some(value);
        }
        if metadata.is_none() && name == &descriptor {
            metadata = Some(value);
        }
    }
    let Some(own) = own else {
        return false;
    };
    if matches!(own, crate::value::Value::BindingCell(_)) {
        return false;
    }
    metadata.is_none_or(writable_data_descriptor)
}

fn writable_data_descriptor(value: &crate::value::Value) -> bool {
    let crate::value::Value::Object(fields) = value else {
        return false;
    };
    let mut writable = None;
    for (name, value) in fields.iter().rev() {
        if matches!(name.as_str(), "get" | "set") {
            return false;
        }
        if writable.is_none() && name == "writable" {
            writable = Some(matches!(value, crate::value::Value::Boolean(true)));
        }
    }
    writable.unwrap_or(true)
}

#[cfg(test)]
mod proven_own_data_tests {
    use super::plain_writable_own_data;
    use crate::value::{ObjectData, Value};

    fn object(metadata: Option<Value>) -> ObjectData {
        let mut entries = vec![("field".into(), Value::Number(1.0))];
        if let Some(metadata) = metadata {
            entries.push((crate::builtins::descriptor_key("field"), metadata));
        }
        ObjectData::new(entries)
    }

    fn descriptor(fields: Vec<(&str, Value)>) -> Value {
        Value::Object(std::rc::Rc::new(ObjectData::new(
            fields.into_iter().map(|(name, value)| (name.into(), value)).collect(),
        )))
    }

    #[test]
    fn proves_plain_and_explicitly_writable_data() {
        assert!(plain_writable_own_data(&object(None), "field"));
        let metadata = descriptor(vec![("writable", Value::Boolean(true))]);
        assert!(plain_writable_own_data(&object(Some(metadata)), "field"));
    }

    #[test]
    fn rejects_non_writable_accessor_deleted_and_cell_properties() {
        let readonly = descriptor(vec![("writable", Value::Boolean(false))]);
        assert!(!plain_writable_own_data(&object(Some(readonly)), "field"));
        let getter = descriptor(vec![("get", Value::Undefined)]);
        assert!(!plain_writable_own_data(&object(Some(getter)), "field"));
        let mut deleted = object(None);
        deleted.properties.push((crate::builtins::deleted_key("field"), Value::Undefined));
        assert!(!plain_writable_own_data(&deleted, "field"));
        let cell = Value::BindingCell(std::rc::Rc::new(std::cell::RefCell::new(Value::Undefined)));
        assert!(!plain_writable_own_data(&object(None), "missing"));
        assert!(!plain_writable_own_data(&ObjectData::new(vec![("field".into(), cell)]), "field"));
    }
}

fn inherited_descriptor(
    target: &crate::value::Value,
    key: &str,
) -> Result<Option<crate::value::Value>, crate::execute::VmError> {
    let mut current = target.clone();
    loop {
        let descriptor = crate::builtins::object::descriptor(
            Some(&current),
            Some(&crate::value::Value::String(key.to_string())),
        )?;
        if !matches!(descriptor, crate::value::Value::Undefined) {
            return Ok(Some(descriptor));
        }
        current = crate::builtins::object::get_prototype_of(Some(&current))?;
        if matches!(current, crate::value::Value::Null) {
            return Ok(None);
        }
    }
}

fn descriptor_field_exists(descriptor: &crate::value::Value, field: &str) -> bool {
    !matches!(
        crate::builtins::object::descriptor(
            Some(descriptor),
            Some(&crate::value::Value::String(field.to_string())),
        ),
        Ok(crate::value::Value::Undefined)
    )
}

fn descriptor_writable(descriptor: &crate::value::Value) -> Result<bool, crate::execute::VmError> {
    Ok(matches!(
        crate::execute::get_property_result(descriptor, "writable")?,
        crate::value::Value::Boolean(true)
    ))
}

fn call_setter(
    descriptor: &crate::value::Value,
    receiver: &crate::value::Value,
    value: &crate::value::Value,
) -> Result<bool, crate::execute::VmError> {
    let setter = crate::execute::get_property_result(descriptor, "set")?;
    if matches!(setter, crate::value::Value::Undefined) {
        return Ok(false);
    }
    crate::functions::execute_target_with_receiver(&setter, receiver, std::slice::from_ref(value))?;
    Ok(true)
}

fn set_receiver_data(
    receiver: &crate::value::Value,
    key: &str,
    value: &crate::value::Value,
) -> Result<bool, crate::execute::VmError> {
    if matches!(receiver, crate::value::Value::Proxy(_)) {
        return Ok(matches!(
            crate::proxy::proxy_set(receiver, key, value, Some(receiver))?,
            crate::value::Value::Boolean(true)
        ));
    }
    let receiver_resolved = crate::locals::resolved_replacement(receiver.clone());
    let current = crate::builtins::object::descriptor(
        Some(&receiver_resolved),
        Some(&crate::value::Value::String(key.to_string())),
    )?;
    if !matches!(current, crate::value::Value::Undefined) {
        if descriptor_field_exists(&current, "set") || !descriptor_writable(&current)? {
            return Ok(false);
        }
    } else if rejects_new_property(receiver, key) {
        return Ok(false);
    }
    let descriptor =
        receiver_data_descriptor(value, matches!(current, crate::value::Value::Undefined));
    let updated = crate::builtins::define_own_property(receiver, key, &descriptor)?;
    crate::locals::replace_value(receiver, &updated);
    Ok(true)
}

fn receiver_data_descriptor(
    value: &crate::value::Value,
    create: bool,
) -> Vec<(String, crate::value::Value)> {
    let mut descriptor = vec![("value".to_string(), value.clone())];
    if create {
        descriptor.extend([
            ("writable".to_string(), crate::value::Value::Boolean(true)),
            ("enumerable".to_string(), crate::value::Value::Boolean(true)),
            (
                "configurable".to_string(),
                crate::value::Value::Boolean(true),
            ),
        ]);
    }
    descriptor
}

fn ordinary_set(
    registers: &mut Vec<crate::value::Value>,
    object: u16,
    target: &crate::value::Value,
    key: &str,
    value: crate::value::Value,
    strict: bool,
) -> Result<(), crate::execute::VmError> {
    if !set_with_receiver(target, key, &value, target)? {
        return write_failure(strict);
    }
    if let Some(updated) = crate::locals::replacement(target) {
        crate::execute::write_value(registers, object, updated.clone());
        crate::vm::synchronize_global_object(registers, target, &updated);
    }
    Ok(())
}
