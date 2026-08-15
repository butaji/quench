pub(crate) fn set_property(target: Value, key: &str, value: Value) -> Value {
    if let Some(result) = crate::typed_array_prototype::set(&target, key, value.clone()) {
        return result;
    }
    if let Some(result) = crate::typed_array_ops::set_property(&target, key, &value) {
        return result.unwrap_or(target);
    }
    if let Some(result) = set_prototype_slot(&target, key, value.clone()) {
        return result;
    }
    if let Some(result) = set_promise_property(&target, key, value.clone()) {
        return result;
    }
    match target {
        Value::Object(properties)
            if descriptor_flag_in(&properties, key, "writable") == Some(false) =>
        {
            Value::Object(properties)
        }
        Value::Object(properties) => builtins_cells::set_object_property(properties, key, value),
        Value::ObjectAlias(alias) => set_object_alias_property(alias, key, value),
        Value::Array(values) if array_descriptor_flag(&values, key, "writable") == Some(false) => {
            Value::Array(values)
        }
        Value::Array(values) => set_array_property(values, key, value),
        Value::Function(function) => set_function_property(function, key, value),
        Value::BoundFunction(bound) => {
            {
                let mut properties = bound.properties.borrow_mut();
                properties.retain(|(name, _)| name != key);
                properties.push((key.to_string(), value));
            }
            Value::BoundFunction(bound)
        }
        Value::DataView(view) => {
            view.set_own_property(key, value);
            Value::DataView(view)
        }
        other => other,
    }
}

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
    let key = crate::conversion::to_property_key(arguments.get(1).unwrap_or(&Value::Undefined))?;
    if matches!(target, Value::Proxy(_)) {
        return crate::proxy::proxy_define_property(
            target,
            &key,
            arguments.get(2).unwrap_or(&Value::Undefined),
        );
    }
    let Some(Value::Object(descriptor)) = arguments.get(2) else {
        return Ok(target.clone());
    };
    let result = define_own_property(target, &key, descriptor)?;
    crate::locals::replace_value(target, &result);
    crate::super_scope::attach_home_objects(&result);
    Ok(result)
}
pub(crate) fn define_own_property(
    target: &Value,
    key: &str,
    descriptor: &[(String, Value)],
) -> Result<Value, crate::execute::VmError> {
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
    ) {
        return define_property_value(target, key, Value::Undefined);
    }
    target
}

include!("builtins_array.rs");
include!("builtins_descriptor.rs");
include!("builtins_define_properties.rs");

fn set_function_property(
    function: Rc<crate::value::FunctionValue>,
    key: &str,
    value: Value,
) -> Value {
    if descriptor_flag_in(&function.properties.borrow(), key, "writable") == Some(false) {
        return Value::Function(function);
    }
    {
        let mut properties = function.properties.borrow_mut();
        if let Some((_, current)) = properties.iter_mut().rev().find(|(name, _)| name == key) {
            *current = value;
        } else {
            properties.push((key.to_string(), value));
        }
    }
    Value::Function(function)
}

include!("builtins/function_name.rs");
include!("builtins_prototype.rs");
include!("builtins_value_string.rs");
