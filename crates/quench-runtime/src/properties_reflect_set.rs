pub(crate) fn set_with_receiver(
    target: &crate::value::Value,
    key: &str,
    value: &crate::value::Value,
    receiver: &crate::value::Value,
) -> Result<bool, crate::execute::VmError> {
    if !crate::value::is_object(receiver) {
        if target.typed_array_meta().is_some()
            && crate::typed_array_ops::is_index_key(key)
            && (crate::typed_array_prototype::is_out_of_bounds(target)
                || crate::typed_array_ops::logical_len(target)
                    .is_some_and(|length| key.parse::<usize>().is_ok_and(|index| index >= length)))
        {
            return Ok(true);
        }
        return Ok(false);
    }
    if target.typed_array_meta().is_some() {
        let same_receiver = crate::builtins::same_value(Some(target), Some(receiver));
        if crate::typed_array_ops::canonical_numeric_index(key)
            && !crate::typed_array_ops::is_index_key(key)
        {
            return Ok(true);
        }
        if crate::typed_array_ops::is_index_key(key) {
            let out_of_bounds = crate::typed_array_prototype::is_out_of_bounds(target)
                || crate::typed_array_ops::logical_len(target).is_some_and(|length| {
                    key.parse::<usize>().map_or(true, |index| index >= length)
                });
            if !same_receiver && out_of_bounds {
                return Ok(true);
            }
            if same_receiver {
                if let Some(result) = crate::typed_array_ops::set_property(target, key, value) {
                    result?;
                    return Ok(true);
                }
            }
        }
    }
    if crate::module_bindings::is_namespace(target)
        || crate::module_bindings::is_namespace(receiver)
    {
        return Ok(false);
    }
    // Preserve identity for a direct write to an existing ordinary array
    // element. This is the array analogue of the proven object-slot path;
    // going through generic [[DefineOwnProperty]] can create a COW
    // representative whose dense tail is no longer visible to the original
    // array value.
    if let (crate::value::Value::Array(target_array), crate::value::Value::Array(receiver_array)) =
        (target, receiver)
    {
        if std::rc::Rc::ptr_eq(target_array, receiver_array) {
            if let Some(index) = crate::arrays::array_index(key).map(|index| index as usize) {
                if target_array.has_plain_dense_index(index) {
                    replace_plain_array_index(receiver, target_array, index, value);
                    return Ok(true);
                }
            }
        }
    }
    let resolved_target = crate::locals::resolved_replacement(target.clone());
    if set_proven_own_data(&resolved_target, key, value, receiver) {
        return Ok(true);
    }
    let own = crate::builtins::object::has_own_property(
        Some(&resolved_target),
        Some(&crate::value::Value::String(key.to_string())),
    ) == crate::value::Value::Boolean(true);
    if own {
        let descriptor = crate::builtins::object::descriptor(
            Some(&resolved_target),
            Some(&crate::value::Value::String(key.to_string())),
        )?;
        if descriptor_field_exists(&descriptor, "set") {
            return call_setter(&descriptor, receiver, value);
        }
        if !descriptor_writable(&descriptor)? {
            return Ok(false);
        }
        return set_receiver_data(receiver, key, value);
    }
    if key == "length" && crate::regexp::has_regexp_internal_slot(&resolved_target) {
        return set_receiver_data(receiver, key, value);
    }
    let parent = crate::builtins::object::get_prototype_of(Some(&resolved_target))?;
    if parent.typed_array_meta().is_some()
        && (crate::typed_array_ops::is_index_key(key)
            || crate::typed_array_ops::canonical_numeric_index(key))
    {
        let valid = crate::typed_array_ops::is_index_key(key)
            && !crate::typed_array_prototype::is_out_of_bounds(&parent)
            && crate::typed_array_ops::logical_len(&parent)
                .is_some_and(|length| key.parse::<usize>().is_ok_and(|index| index < length));
        return if valid {
            set_receiver_data(receiver, key, value)
        } else {
            if crate::typed_array_ops::is_index_key(key) && receiver.typed_array_meta().is_some() {
                if matches!(
                    receiver,
                    crate::value::Value::BigInt64Array(_) | crate::value::Value::BigUint64Array(_)
                ) {
                    crate::construct::bigint_bits(value)?;
                } else {
                    crate::conversion::to_number(value)?;
                }
            }
            Ok(true)
        };
    }
    if matches!(parent, crate::value::Value::Proxy(_)) {
        return crate::proxy::proxy_set(&parent, key, value, Some(receiver))
            .map(|result| crate::execute::is_truthy(&result));
    }
    let descriptor = inherited_descriptor(target, key)?;
    match descriptor {
        Some(descriptor) if descriptor_field_exists(&descriptor, "set") => {
            call_setter(&descriptor, receiver, value)
        }
        Some(descriptor) if !descriptor_writable(&descriptor)? => Ok(false),
        _ if !crate::properties::object_is_extensible(&resolved_target) => Ok(false),
        _ => set_receiver_data(receiver, key, value),
    }
}

fn set_proven_own_data(
    target: &crate::value::Value,
    key: &str,
    value: &crate::value::Value,
    receiver: &crate::value::Value,
) -> bool {
    if let (crate::value::Value::Array(target), crate::value::Value::Array(receiver_resolved)) = (
        target,
        &crate::locals::resolved_replacement(receiver.clone()),
    ) {
        let index = crate::arrays::array_index(key).map(|index| index as usize);
        if std::rc::Rc::ptr_eq(target, receiver_resolved)
            && index.is_some_and(|index| target.has_plain_dense_index(index))
        {
            // This is an ordinary existing dense slot: no prototype,
            // accessor, descriptor, or length transition can observe the
            // write. Mutate the canonical array storage directly so a COW
            // replacement cannot lose the tail of an object-valued array.
            // The previous generic defineProperty path could materialize a
            // replacement with only the written prefix, which is observable
            // through Array#length and breaks ordinary indexed assignment.
            let index = index.expect("proven dense array index");
            replace_plain_array_index(receiver, receiver_resolved, index, value);
            return true;
        }
    }
    let (crate::value::Value::Object(target), crate::value::Value::Object(receiver)) =
        (target, receiver)
    else {
        return false;
    };
    if !std::rc::Rc::ptr_eq(target, receiver) || !plain_writable_own_data(target, key) {
        return false;
    }
    if let Some(slot) = target.hot_properties().position_rev(key) {
        if let Some(crate::value::Value::BindingCell(cell)) =
            target.hot_properties().slot_value(slot)
        {
            cell.store(value.clone());
            return true;
        }
        target.hot_properties().store_slot(slot, value.clone());
        return true;
    }
    false
}

fn plain_writable_own_data(properties: &crate::value::ObjectData, key: &str) -> bool {
    let deleted = crate::builtins::deleted_key(key);
    let descriptor = crate::builtins::descriptor_key(key);
    let mut own = None;
    let mut metadata = None;
    for (slot, name) in properties.hot_properties().names().enumerate().rev() {
        if name == &deleted {
            return false;
        }
        if own.is_none() && name == key {
            own = properties.hot_properties().slot_value(slot);
        }
        if metadata.is_none() && name == &descriptor {
            metadata = properties.hot_properties().slot_value(slot);
        }
    }
    if own.is_none() {
        return false;
    }
    metadata.map_or(true, |value| writable_data_descriptor(&value))
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
            fields
                .into_iter()
                .map(|(name, value)| (name.into(), value))
                .collect(),
        )))
    }

    #[test]
    fn proves_plain_and_explicitly_writable_data() {
        assert!(plain_writable_own_data(&object(None), "field"));
        let metadata = descriptor(vec![("writable", Value::Boolean(true))]);
        assert!(plain_writable_own_data(&object(Some(metadata)), "field"));
    }

    #[test]
    fn rejects_non_writable_accessor_and_deleted_properties() {
        let readonly = descriptor(vec![("writable", Value::Boolean(false))]);
        assert!(!plain_writable_own_data(&object(Some(readonly)), "field"));
        let getter = descriptor(vec![("get", Value::Undefined)]);
        assert!(!plain_writable_own_data(&object(Some(getter)), "field"));
        let mut deleted = object(None);
        deleted.properties.push((
            crate::builtins::deleted_key("field").into(),
            Value::Undefined,
        ));
        assert!(!plain_writable_own_data(&deleted, "field"));
        let cell = Value::BindingCell(crate::value::BindingCell::new(Value::Undefined));
        assert!(!plain_writable_own_data(&object(None), "missing"));
        assert!(plain_writable_own_data(
            &ObjectData::new(vec![("field".into(), cell)]),
            "field"
        ));
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
    let mut receiver_resolved = match receiver {
        crate::value::Value::BindingCell(cell) => crate::locals::resolved_replacement(cell.load()),
        _ => crate::locals::resolved_replacement(receiver.clone()),
    };
    if let (crate::value::Value::Object(target), value_target) = (&receiver_resolved, value) {
        let self_reference = match value_target {
            crate::value::Value::Object(value) => std::rc::Rc::ptr_eq(target, value),
            crate::value::Value::ObjectAlias(alias) => {
                alias.target().is_some_and(|value| std::rc::Rc::ptr_eq(target, &value))
            }
            _ => false,
        };
        if self_reference {
            let updated = crate::builtins::object_alias::set(
                std::rc::Rc::clone(target),
                key,
                value.clone(),
            );
            crate::locals::replace_value(receiver, &updated);
            return Ok(true);
        }
    }
    if let crate::value::Value::Array(values) = &receiver_resolved {
        if let Some(index) = crate::arrays::array_index(key).map(|index| index as usize) {
            let plain = values.has_plain_dense_index(index);
            if plain {
                replace_plain_array_index(receiver, values, index, value);
                return Ok(true);
            }
        }
    }
    if receiver_resolved.typed_array_meta().is_some()
        && crate::typed_array_ops::is_index_key(key)
        && (crate::typed_array_prototype::is_out_of_bounds(&receiver_resolved)
            || crate::typed_array_ops::logical_len(&receiver_resolved)
                .is_some_and(|length| key.parse::<usize>().is_ok_and(|index| index >= length)))
    {
        return Ok(false);
    }
    let mutable_target = match &receiver_resolved {
        crate::value::Value::Object(properties) => Some(std::rc::Rc::clone(properties)),
        crate::value::Value::ObjectAlias(alias) => alias.target(),
        _ => None,
    };
    if let Some(properties) = mutable_target {
        let host_mutable = properties.iter().any(|(name, value)| {
            name == "\0quench:async_hooks:mutable"
                && matches!(value, crate::value::Value::Boolean(true))
        });
        if host_mutable {
            // Async-hook resources are identity-bearing host objects. Keep
            // user properties written by init hooks on the canonical object
            // rather than publishing a COW replacement unreachable by the
            // host resource table.
            unsafe {
                (&mut *(std::rc::Rc::as_ptr(&properties) as *mut crate::value::ObjectData))
                    .set_property_in_place(key, value.clone());
            }
            return Ok(true);
        }
    }
    // OrdinarySetWithOwnDescriptor performs one [[GetOwnProperty]] on the
    // receiver.  Keep that lookup unified so a Proxy observes a single
    // getOwnPropertyDescriptor trap rather than a hasOwnProperty probe plus a
    // second descriptor lookup.
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
    if let crate::value::Value::Array(values) = &receiver_resolved {
        if let Some(index) = crate::arrays::array_index(key).map(|index| index as usize) {
            let plain = index < values.logical_len()
                && index < values.physical_len()
                && values.descriptor(key).is_none();
            if plain {
                replace_plain_array_index(receiver, values, index, value);
                return Ok(true);
            }
        }
    }
    let descriptor = receiver_data_descriptor(
        &receiver_resolved,
        value,
        matches!(current, crate::value::Value::Undefined),
    );
    let updated = match crate::builtins::define_own_property(&receiver_resolved, key, &descriptor) {
        Err(error)
            if key == "length"
                && matches!(receiver_resolved, crate::value::Value::Array(_))
                && non_configurable_redefinition(&error) =>
        {
            // ArraySetLength coerces the value before it reads the current
            // length descriptor.  If that coercion makes length read-only,
            // the already-started [[Set]] must report false (and let strict
            // assignment turn it into its own TypeError), not leak the
            // internal defineProperty rejection.
            return Ok(false);
        }
        result => result?,
    };
    crate::locals::replace_value(receiver, &updated);
    if let crate::value::Value::BindingCell(cell) = receiver {
        cell.replace(updated.clone());
    }
    Ok(true)
}

fn replace_plain_array_index(
    _receiver: &crate::value::Value,
    values: &std::rc::Rc<crate::value::ArrayData>,
    index: usize,
    value: &crate::value::Value,
) {
    // Indexed writes mutate the receiver's storage. Copy-on-write here would
    // detach aliases and make `a[0] = a` observably non-circular.
    unsafe {
        let data = &mut *(std::rc::Rc::as_ptr(values) as *mut crate::value::ArrayData);
        data.set_length(values.logical_len());
        data.set_index(index, value.clone());
    }
}

fn non_configurable_redefinition(error: &crate::execute::VmError) -> bool {
    let crate::execute::VmError::Thrown(crate::value::Value::Object(properties)) = error else {
        return false;
    };
    properties.iter().any(|(name, value)| {
        name == "message"
            && matches!(value, crate::value::Value::String(text) if text.starts_with("Cannot redefine") || text.starts_with("Cannot delete non-configurable array element") || text == "Cannot assign to read only array length")
    })
}

fn receiver_data_descriptor(
    _receiver: &crate::value::Value,
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
    registers: &mut crate::register_file::RegisterFile,
    object: u16,
    target: &crate::value::Value,
    key: &str,
    value: crate::value::Value,
    strict: bool,
) -> Result<(), crate::execute::VmError> {
    // A script's global-object view is an execution-facing snapshot, while
    // `this` and global bindings share the realm's canonical global object.
    // Route writes through that owner so a COW view cannot hide a global var
    // binding (for example `this.x = 1` followed by `var x`).
    let owner = crate::vm::resolve_global_owner(target).unwrap_or_else(|| target.clone());
    if !set_with_receiver(&owner, key, &value, &owner)? {
        return write_failure(strict);
    }
    if let Some(updated) = crate::locals::replacement(&owner) {
        crate::execute::write_value(registers, object, updated.clone());
        crate::vm::synchronize_global_object(registers, &owner, &updated);
    }
    Ok(())
}
