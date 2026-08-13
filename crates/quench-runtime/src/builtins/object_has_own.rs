// Object.hasOwn / hasOwnProperty target resolution and own-property detection.

fn has_own_target<'a>(
    receiver: Option<&'a Value>,
    arguments: &'a [Value],
) -> (Option<&'a Value>, Option<&'a Value>) {
    match receiver {
        None => static_target(arguments),
        _ => (receiver, arguments.first()),
    }
}
fn static_target(arguments: &[Value]) -> (Option<&Value>, Option<&Value>) {
    (arguments.first(), arguments.get(1))
}
fn has_own_property_result(
    receiver: Option<&Value>,
    key: Option<&Value>,
) -> Result<Value, VmError> {
    let receiver = require_object_coercible(receiver)?;
    let Some(key) = key else {
        return Ok(Value::Boolean(false));
    };
    let key = crate::properties::dynamic_property_key(key)?;
    Ok(Value::Boolean(owns_property(receiver, &key)?))
}
fn require_object_coercible(receiver: Option<&Value>) -> Result<&Value, VmError> {
    match receiver {
        Some(Value::Null) | Some(Value::Undefined) | None => {
            Err(VmError::Thrown(crate::builtins::error(
                Builtin::TypeError,
                &[Value::String(
                    "Cannot convert undefined or null to object".to_string(),
                )],
            )))
        }
        Some(value) => Ok(value),
    }
}
fn owns_property(receiver: &Value, key: &str) -> Result<bool, VmError> {
    Ok(match receiver {
        Value::Object(properties) => object_data_owns(properties, key),
        Value::ObjectAlias(alias) => alias
            .0
            .borrow()
            .upgrade()
            .is_some_and(|properties| object_owns(&properties, key)),
        Value::Array(values) => array_owns(values, key),
        Value::String(value) => {
            key == "length" || valid_index(key, crate::strings::utf16_len(value))
        }
        Value::Builtin(builtin) => builtin_owns_property(*builtin, key),
        Value::Function(function) => function
            .properties
            .borrow()
            .iter()
            .rev()
            .any(|(name, _)| name == key),
        Value::BoundFunction(bound) => bound
            .properties
            .borrow()
            .iter()
            .rev()
            .any(|(name, _)| name == key),
        Value::Proxy(_) => {
            crate::proxy::proxy_get_own_property_descriptor(receiver, key)? != Value::Undefined
        }
        Value::DataView(view) => view.own_property(key).is_some(),
        value if typed_array_owns(value, key) => true,
        _ => false,
    })
}

fn typed_array_owns(value: &Value, key: &str) -> bool {
    if let Ok(index) = key.parse::<usize>() {
        return crate::typed_array_prototype::index_exists(value, index);
    }
    crate::typed_array_prototype::own_property(value, key).is_some()
}

fn object_data_owns(properties: &Rc<ObjectData>, key: &str) -> bool {
    let deleted = properties
        .iter()
        .any(|(name, _)| name == &crate::builtins::deleted_key(key));
    properties
        .iter()
        .any(|(name, _)| name == key && !super::is_descriptor_key(name))
        || boxed_string_owns(properties, key)
        || (!deleted
            && crate::vm::is_global_object(&Value::Object(properties.clone()))
            && crate::vm::global_builtin_exists(key))
}

fn boxed_string_owns(properties: &[(String, Value)], key: &str) -> bool {
    let Some((_, Value::String(value))) = properties.iter().find(|(name, _)| name == "_value")
    else {
        return false;
    };
    if crate::conversion::is_symbol_string(value) {
        return false;
    }
    key == "length"
        || key
            .parse::<usize>()
            .is_ok_and(|index| index < crate::strings::utf16_len(value))
}

fn array_owns(values: &crate::value::ArrayData, key: &str) -> bool {
    (!values.is_arguments() && key == "length")
        || key
            .parse::<usize>()
            .is_ok_and(|index| values.has_index(index))
        || values.property(key).is_some()
        || values.descriptor(key).is_some()
        || (values.is_strict_arguments() && key == "callee")
}

fn object_owns(properties: &Rc<ObjectData>, key: &str) -> bool {
    object_data_owns(properties, key)
}
fn builtin_owns_property(builtin: Builtin, key: &str) -> bool {
    if crate::builtins::builtin_prototype_property_is_removed(builtin, key) {
        return false;
    }
    (builtin == Builtin::Object && key == "hasOwn")
        || builtin_descriptor(builtin, key).is_some()
        || super::callable_property(builtin, key).is_some()
        || super::special_property(builtin, key).is_some()
}
fn valid_index(key: &str, len: usize) -> bool {
    key.parse::<usize>().is_ok_and(|index| index < len)
}
