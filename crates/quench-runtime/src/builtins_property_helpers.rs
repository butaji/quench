fn set_object_alias_property(
    alias: crate::value::ObjectAliasValue,
    key: &str,
    value: Value,
) -> Value {
    let Some(properties) = alias.0.borrow().upgrade() else {
        return Value::ObjectAlias(alias);
    };
    let result = builtins_cells::set_object_property(properties, key, value);
    retarget_object_alias(&alias, &result);
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
        _ => return None,
    })
}
pub(crate) fn define_property(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let Some(target) = arguments.first() else {
        return Ok(Value::Undefined);
    };
    if !crate::value::is_object(target) {
        return Err(crate::value::error::throw_type_error(
            "Object.defineProperty target must be an object",
        ));
    }
    let key = crate::conversion::to_property_key(arguments.get(1).unwrap_or(&Value::Undefined))?;
    if matches!(target, Value::Proxy(_)) {
        return crate::proxy::proxy_define_property(
            target,
            &key,
            arguments.get(2).unwrap_or(&Value::Undefined),
        );
    }
    let Some(descriptor) = arguments.get(2) else {
        return Ok(target.clone());
    };
    let descriptor = descriptor_fields(descriptor)?;
    let result = define_own_property(target, &key, &descriptor)?;
    crate::locals::replace_value(target, &result);
    crate::super_scope::attach_home_objects(&result);
    Ok(result)
}
pub(crate) fn define_own_property(
    target: &Value,
    key: &str,
    descriptor: &[(String, Value)],
) -> Result<Value, crate::execute::VmError> {
    validate_descriptor_kind(descriptor)?;
    let key_value = Value::String(key.to_string());
    let current = crate::builtins::object::descriptor(Some(target), Some(&key_value))?;
    if matches!(current, Value::Undefined) && crate::properties::rejects_new_property(target, key) {
        return Err(crate::value::error::throw_type_error(
            "Cannot define a property on a non-extensible object",
        ));
    }
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
    let mut result = if accessor {
        define_accessor_placeholder(target.clone(), key)
    } else {
        define_property_value(target.clone(), key, value)
    };
    store_descriptor_metadata(&mut result, key, &descriptor);
    define_array_descriptor(&mut result, key, descriptor);
    Ok(result)
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
    let metadata = Value::Object(Rc::new(ObjectData::new(descriptor.to_vec())));
    let descriptor_key = descriptor_key(key);
    match result {
        Value::Object(properties) => {
            let properties = Rc::make_mut(properties);
            properties.retain(|(name, _)| name != &descriptor_key);
            properties.push((descriptor_key, metadata));
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
fn define_accessor_placeholder(target: Value, key: &str) -> Value {
    if matches!(
        target,
        Value::Object(_)
            | Value::Function(_)
            | Value::Builtin(_)
            | Value::Promise(_)
            | Value::BoundFunction(_)
            | Value::ArrayBuffer(_)
    ) {
        return define_property_value(target, key, Value::Undefined);
    }
    target
}

include!("builtins_array.rs");
include!("builtins_descriptor.rs");
include!("builtins_define_properties.rs");


