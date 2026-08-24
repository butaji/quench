fn set_object_alias_property(
    alias: crate::value::ObjectAliasValue,
    key: &str,
    value: Value,
) -> Value {
    let Some(properties) = alias.0.borrow().upgrade() else {
        return Value::ObjectAlias(alias);
    };
    let previous = Rc::clone(&properties);
    let result = builtins_cells::set_object_property(properties, key, value);
    retarget_object_alias(&alias, &result);
    if let Value::Object(object) = &result {
        crate::vm::replace_realm_global_if_current(&previous, object);
    }
    result
}

fn retarget_object_alias(alias: &crate::value::ObjectAliasValue, value: &Value) {
    let Value::Object(object) = value else { return };
    *alias.0.borrow_mut() = Rc::downgrade(object);
}

fn set_prototype_slot(target: &Value, key: &str, value: Value) -> Option<Value> {
    if key != "\0prototype" {
        return None;
    }
    if let Value::Builtin(builtin) = target {
        crate::builtins::set_intrinsic_prototype_override(*builtin, value);
        return Some(target.clone());
    }
    Some(match target {
        Value::ArrayBuffer(buffer) => {
            buffer.set_prototype(value);
            Value::ArrayBuffer(buffer.clone())
        }
        Value::DataView(view) => {
            view.set_prototype(value);
            Value::DataView(view.clone())
        }
        Value::Map(data) => {
            data.set_prototype(value);
            Value::Map(data.clone())
        }
        Value::Set(data) => {
            data.set_prototype(value);
            Value::Set(data.clone())
        }
        Value::Array(data) => {
            data.set_prototype(value);
            Value::Array(data.clone())
        }
        Value::Promise(data) => {
            data.set_prototype(value);
            Value::Promise(data.clone())
        }
        other => return set_typed_array_prototype(other.clone(), value),
    })
}

fn set_typed_array_prototype(target: Value, value: Value) -> Option<Value> {
    macro_rules! set_view {
        ($variant:ident, $view:ident) => {{
            $view.set_prototype(value);
            Value::$variant($view)
        }};
    }
    Some(match target {
        Value::Float64Array(view) => set_view!(Float64Array, view),
        Value::Float32Array(view) => set_view!(Float32Array, view),
        Value::Int8Array(view) => set_view!(Int8Array, view),
        Value::Int16Array(view) => set_view!(Int16Array, view),
        Value::Int32Array(view) => set_view!(Int32Array, view),
        Value::Uint8Array(view) => set_view!(Uint8Array, view),
        Value::Uint16Array(view) => set_view!(Uint16Array, view),
        Value::Uint32Array(view) => set_view!(Uint32Array, view),
        Value::Uint8ClampedArray(view) => set_view!(Uint8ClampedArray, view),
        Value::BigInt64Array(view) => set_view!(BigInt64Array, view),
        Value::BigUint64Array(view) => set_view!(BigUint64Array, view),
        _ => return None,
    })
}
pub(crate) fn define_property(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let Some(target) = arguments.first() else {
        return Ok(Value::Undefined);
    };
    let target = crate::locals::resolved_replacement(target.clone());
    if !crate::value::is_object(&target) {
        return Err(crate::value::error::throw_type_error(
            "Object.defineProperty target must be an object",
        ));
    }
    let key = crate::conversion::to_property_key(arguments.get(1).unwrap_or(&Value::Undefined))?;
    if matches!(target, Value::Proxy(_)) {
        return crate::proxy::proxy_define_property(
            &target,
            &key,
            arguments.get(2).unwrap_or(&Value::Undefined),
        );
    }
    let Some(descriptor) = arguments.get(2) else {
        return Ok(target.clone());
    };
    if !crate::value::is_object(descriptor) {
        return Err(crate::value::error::throw_type_error(
            "Property descriptor must be an object",
        ));
    }
    let descriptor = descriptor_fields(descriptor)?;
    let result = define_own_property(&target, &key, &descriptor)?;
    crate::locals::replace_value(&target, &result);
    crate::super_scope::attach_home_objects(&result);
    Ok(result)
}
pub(crate) fn define_own_property(
    target: &Value,
    key: &str,
    descriptor: &[(String, Value)],
) -> Result<Value, crate::execute::VmError> {
    let target = crate::locals::resolved_replacement(target.clone());
    if matches!(target, Value::Proxy(_)) {
        let desc = Value::Object(Rc::new(ObjectData::new(descriptor.to_vec())));
        let result = crate::proxy::proxy_define_property(&target, key, &desc)?;
        if !crate::execute::is_truthy(&result) {
            return Err(crate::value::error::throw_type_error(
                "Proxy defineProperty returned false",
            ));
        }
        return Ok(target);
    }
    // ArraySetLength coerces its value before reading the current length
    // descriptor; the coercion may mutate the array.
    validate_array_length_descriptor(&target, key, descriptor)?;
    validate_array_index_length(&target, key)?;
    if let Some(result) = prepare_array_length_definition(&target, key, descriptor)? {
        return Ok(result);
    }
    let key_value = Value::String(key.to_string());
    let current = ordinary_own_descriptor(&target, key, &key_value)?;
    validate_redefinition(&current, descriptor)?;
    let descriptor = complete_descriptor(descriptor, &current);
    let value = descriptor
        .iter()
        .rev()
        .find(|(name, _)| name == "value")
        .map_or(Value::Undefined, |(_, value)| value.clone());
    let accessor = descriptor
        .iter()
        .any(|(name, _)| matches!(name.as_str(), "get" | "set"));
    // Per spec 10.2.4 ([[DefineOwnProperty]] for arguments objects),
    // redefining an index with an accessor (or with writable: false)
    // disconnects the parameter mapping. Skip the placeholder write in
    // that case so the placeholder value (`Undefined`) does not flow
    // through the parameter binding.
    let is_arguments = matches!(&target, Value::Array(values) if values.is_arguments());
    let needs_disconnect = is_arguments
        && (accessor
            || descriptor
                .iter()
                .rev()
                .any(|(name, value)| name == "writable"
                    && matches!(value, Value::Boolean(false))));
    let mut result = if accessor {
        if needs_disconnect {
            // Skip the placeholder write — the index has been, or is
            // about to be, disconnected. The descriptor is installed
            // below.
            target.clone()
        } else {
            define_accessor_placeholder(target.clone(), key)
        }
    } else if let Some(updated) = crate::typed_array_ops::set_property(&target, key, &value) {
        updated?
    } else {
        define_property_value(target.clone(), key, value)
    };
    store_descriptor_metadata(&mut result, key, &descriptor);
    define_array_descriptor(&mut result, key, descriptor);
    Ok(result)
}

fn ordinary_own_descriptor(
    target: &Value,
    key: &str,
    key_value: &Value,
) -> Result<Value, crate::execute::VmError> {
    let current = crate::builtins::object::descriptor(Some(target), Some(key_value))?;
    if !matches!(current, Value::Undefined) {
        crate::execution_trace::descriptor_object("current");
        return Ok(current);
    }
    let owned = crate::builtins::object::has_own_property(Some(target), Some(key_value));
    if owned != Value::Boolean(true) {
        return Ok(Value::Undefined);
    }
    let value = crate::execute::get_property_result(target, key)?;
    Ok(Value::Object(Rc::new(ObjectData::new(vec![
        ("value".to_string(), value),
        ("writable".to_string(), Value::Boolean(true)),
        ("enumerable".to_string(), Value::Boolean(true)),
        ("configurable".to_string(), Value::Boolean(true)),
    ]))))
}

fn validate_descriptor_kind(descriptor: &[(String, Value)]) -> Result<(), crate::execute::VmError> {
    let accessor = descriptor
        .iter()
        .any(|(name, _)| matches!(name.as_str(), "get" | "set"));
    let data = descriptor
        .iter()
        .any(|(name, _)| matches!(name.as_str(), "value" | "writable"));
    if accessor && data {
        return Err(crate::value::error::throw_type_error(
            "Invalid property descriptor",
        ));
    }
    for field in ["get", "set"] {
        let Some(value) = descriptor
            .iter()
            .rev()
            .find_map(|(name, value)| (name == field).then_some(value))
        else {
            continue;
        };
        if !matches!(value, Value::Undefined) && !crate::conversion::is_callable(value) {
            return Err(crate::value::error::throw_type_error(
                "Accessor descriptor must be callable",
            ));
        }
    }
    Ok(())
}
fn store_descriptor_metadata(result: &mut Value, key: &str, descriptor: &[(String, Value)]) {
    let descriptor_key = descriptor_key(key);
    if let Value::Object(properties) = result {
        if default_ordinary_descriptor(descriptor) {
            Rc::make_mut(properties).retain(|(name, _)| name != &descriptor_key);
            return;
        }
    }
    let metadata = Value::Object(Rc::new(ObjectData::new(descriptor.to_vec())));
    crate::execution_trace::descriptor_object("metadata");
    match result {
        Value::Object(properties) => {
            let properties = Rc::make_mut(properties);
            properties.retain(|(name, _)| name != &descriptor_key);
            properties.push((descriptor_key.into(), metadata));
        }
        Value::Function(function) => {
            let mut properties = function.properties.borrow_mut();
            properties.retain(|(name, _)| name != &descriptor_key);
            properties.push((descriptor_key, metadata));
        }
        Value::Promise(promise) => {
            let mut properties = promise.properties.borrow_mut();
            properties.retain(|(name, _)| name != &descriptor_key);
            properties.push((descriptor_key, metadata));
        }
        Value::Builtin(builtin) => write_intrinsic_override(*builtin, key, metadata),
        Value::ArrayBuffer(buffer) => buffer.set_own_property(&descriptor_key, metadata),
        Value::DataView(view) => view.set_own_property(&descriptor_key, metadata),
        Value::BoundFunction(bound) => {
            let mut properties = bound.properties.borrow_mut();
            properties.retain(|(name, _)| name != &descriptor_key);
            properties.push((descriptor_key, metadata));
        }
        _ => {}
    }
}

fn default_ordinary_descriptor(descriptor: &[(String, Value)]) -> bool {
    if descriptor.len() != 4 {
        return false;
    }
    ["writable", "enumerable", "configurable"]
        .into_iter()
        .all(|field| {
            descriptor
                .iter()
                .rev()
                .find_map(|(name, value)| (name == field).then_some(value))
                .is_some_and(|value| matches!(value, Value::Boolean(true)))
        })
        && descriptor.iter().any(|(name, _)| name == "value")
}
fn define_accessor_placeholder(target: Value, key: &str) -> Value {
    if matches!(
        target,
        Value::Object(_)
            | Value::Function(_)
            | Value::Builtin(_)
            | Value::Promise(_)
            | Value::BoundFunction(_)
            | Value::ArrayBuffer(_)
            | Value::Array(_)
    ) {
        return define_property_value(target, key, Value::Undefined);
    }
    target
}

include!("builtins_array.rs");
include!("builtins_descriptor.rs");
include!("builtins_define_properties.rs");
