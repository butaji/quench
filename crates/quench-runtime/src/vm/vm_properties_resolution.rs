pub fn get_property_result(value: &Value, key: &str) -> Result<Value, VmError> {
    let value = crate::locals::resolved_replacement(value.clone());
    get_property_with_receiver(&value, key, &value)
}

pub(crate) fn get_property_with_receiver(
    value: &Value,
    key: &str,
    receiver: &Value,
) -> Result<Value, VmError> {
    if let Some(result) = early_property_result(value, key, receiver) {
        return result;
    }
    if let Some(result) = array_property_result(value, key, receiver) {
        return result;
    }
    if let Some(result) = function_inherited_property_result(value, key, receiver) {
        return result;
    }
    if let Some(result) = object_inherited_property_result(value, key, receiver) {
        return result;
    }
    if let Some(result) = crate::disposable_stack::accessor(value, key, receiver) {
        return result;
    }
    if let Some(result) = data_view_instance_accessor(value, key) {
        return result;
    }
    if let Some(result) = descriptor_property_result(value, key, receiver) {
        return result;
    }
    finish_property_access(value, key, receiver)
}

fn function_inherited_property_result(
    value: &Value,
    key: &str,
    receiver: &Value,
) -> Option<Result<Value, VmError>> {
    let Value::Function(function) = value else {
        return None;
    };
    let properties = function.properties.borrow();
    if properties.iter().any(|(name, _)| name == key) {
        return None;
    }
    let prototype = properties
        .iter()
        .rev()
        .find_map(|(name, value)| (name == "\0prototype").then(|| value.clone()))?;
    Some(get_property_with_receiver(&prototype, key, receiver))
}

fn object_inherited_property_result(
    value: &Value,
    key: &str,
    receiver: &Value,
) -> Option<Result<Value, VmError>> {
    let Value::Object(properties) = value else {
        return None;
    };
    if properties.iter().any(|(name, _)| name == key) {
        return None;
    }
    let prototype = properties
        .iter()
        .rev()
        .find_map(|(name, value)| (name == "\0prototype").then_some(value))?;
    if matches!(prototype, Value::Null) {
        return Some(Ok(Value::Undefined));
    }
    match get_property_with_receiver(prototype, key, receiver) {
        Ok(Value::Undefined) => None,
        result => Some(result),
    }
}

fn finish_property_access(value: &Value, key: &str, receiver: &Value) -> Result<Value, VmError> {
    match crate::property_define::accessor(value, key, "get") {
        None => Ok(receiver_property(value, key, receiver)),
        Some(Value::Undefined) => Ok(Value::Undefined),
        Some(getter) => invoke_accessor(&getter, receiver),
    }
}

fn early_property_result(
    value: &Value,
    key: &str,
    receiver: &Value,
) -> Option<Result<Value, VmError>> {
    if matches!(value, Value::Null | Value::Undefined) {
        return Some(Err(crate::value::error::throw_type_error(&format!(
            "Cannot read property `{key}` of null or undefined"
        ))));
    }
    if matches!(value, Value::Proxy(_)) {
        return Some(crate::proxy::proxy_get(value, key, Some(receiver)));
    }
    if matches!(value, Value::Array(values) if values.is_strict_arguments() && key == "callee") {
        return Some(Err(crate::value::error::throw_type_error(
            "'callee' is unavailable on strict arguments",
        )));
    }
    if has_restricted_function_property(value, key) {
        return Some(Err(crate::value::error::throw_type_error(
            "'caller' and 'arguments' are unavailable on this function",
        )));
    }
    if key == "buffer" && is_typed_array_prototype(value) {
        return Some(Err(crate::value::error::throw_type_error(
            "Receiver is not a TypedArray",
        )));
    }
    None
}

fn is_typed_array_prototype(value: &Value) -> bool {
    matches!(
        value,
        Value::Builtin(
            crate::ops::Builtin::Float64ArrayPrototype
                | crate::ops::Builtin::Float32ArrayPrototype
                | crate::ops::Builtin::Int8ArrayPrototype
                | crate::ops::Builtin::Int16ArrayPrototype
                | crate::ops::Builtin::Int32ArrayPrototype
                | crate::ops::Builtin::Uint8ArrayPrototype
                | crate::ops::Builtin::Uint16ArrayPrototype
                | crate::ops::Builtin::Uint32ArrayPrototype
                | crate::ops::Builtin::Uint8ClampedArrayPrototype
                | crate::ops::Builtin::BigInt64ArrayPrototype
                | crate::ops::Builtin::BigUint64ArrayPrototype
        )
    )
}

fn array_property_result(
    value: &Value,
    key: &str,
    receiver: &Value,
) -> Option<Result<Value, VmError>> {
    if let Some(getter) = array_accessor(value, key, "get") {
        return Some(match getter {
            Value::Undefined => Ok(Value::Undefined),
            getter => invoke_accessor(&getter, receiver),
        });
    }
    let Value::Array(values) = value else {
        return None;
    };
    if array_has_own_property(values, key) {
        return None;
    }
    crate::arrays::prototype_override_getter(key).map(|getter| match getter {
        Value::Undefined => Ok(Value::Undefined),
        getter => invoke_accessor(&getter, receiver),
    })
}

fn array_has_own_property(values: &crate::value::ArrayData, key: &str) -> bool {
    key == "length"
        || crate::arrays::array_index(key).is_some_and(|index| values.has_index(index as usize))
        || values.descriptor(key).is_some()
        || values.property(key).is_some()
}

fn descriptor_property_result(
    value: &Value,
    key: &str,
    receiver: &Value,
) -> Option<Result<Value, VmError>> {
    let Ok(descriptor) =
        crate::builtins::object::descriptor(Some(value), Some(&Value::String(key.to_string())))
    else {
        return None;
    };
    if matches!(descriptor, Value::Undefined) {
        return None;
    }
    if let Value::Object(descriptor) = descriptor {
        if let Some(getter) = descriptor
            .iter()
            .rev()
            .find_map(|(name, value)| (name == "get").then_some(value))
        {
            return Some(match getter {
                Value::Undefined => Ok(Value::Undefined),
                getter => invoke_accessor(getter, receiver),
            });
        }
    }
    Some(Ok(receiver_property(value, key, receiver)))
}

/// Invoke a getter using the receiver as `this`. The getter's own
/// `OrdinaryCallEvaluate` semantics handle ToObject coercion for sloppy
/// functions; strict functions keep the receiver as-is.
fn invoke_accessor(getter: &Value, receiver: &Value) -> Result<Value, VmError> {
    match getter {
        Value::Function(_) | Value::BoundFunction(_) => {
            crate::functions::execute_target(getter, receiver, &[])
        }
        Value::Builtin(builtin) => {
            crate::vm::execute_builtin_with_receiver(*builtin, &[], Some(receiver))
        }
        _ => Err(crate::vm::not_callable()),
    }
}

fn receiver_property(value: &Value, key: &str, receiver: &Value) -> Value {
    let property = get_property(value, key);
    if should_preserve_receiver_property(value, key, &property, receiver)
        || same_property_receiver(value, receiver)
    {
        return property;
    }
    bind_receiver_property(property, receiver)
}

fn should_preserve_receiver_property(
    value: &Value,
    key: &str,
    property: &Value,
    receiver: &Value,
) -> bool {
    if object_has_property(value, key) {
        return true;
    }
    if plural_rules_instance(receiver) {
        return true;
    }
    matches!(value, Value::Builtin(_))
        || matches!(value, Value::Object(_)) && crate::vm::is_global_object(value)
        || is_intl_number_format_property(property)
        || is_boxed_primitive(receiver) && matches!(property, Value::Builtin(_))
        || matches!(key, "constructor" | "prototype")
}

fn plural_rules_instance(value: &Value) -> bool {
    let Value::Object(properties) = value else {
        return false;
    };
    properties.iter().any(|(name, prototype)| {
        name == "\0prototype"
            && match prototype {
                Value::Builtin(Builtin::IntlPluralRulesPrototype) => true,
                Value::BoundFunction(bound) => {
                    bound.target == Value::Builtin(Builtin::IntlPluralRulesPrototype)
                }
                _ => false,
            }
    })
}
fn is_boxed_primitive(value: &Value) -> bool {
    matches!(
        value,
        Value::Object(properties)
            if properties.iter().any(|(name, value)|
                name == "_value"
                    && matches!(
                        value,
                        Value::Number(_) | Value::Boolean(_) | Value::String(_) | Value::BigInt(_)
                    ))
    )
}

fn object_has_property(value: &Value, key: &str) -> bool {
    matches!(value, Value::Object(properties) if properties.iter().rev().any(|(name, _)| name == key))
}

fn is_intl_number_format_property(property: &Value) -> bool {
    matches!(
        property,
        Value::Builtin(
            Builtin::IntlNumberFormatFormatToParts
                | Builtin::IntlNumberFormatFormatRange
                | Builtin::IntlNumberFormatFormatRangeToParts
        )
    )
}

pub(crate) fn bind_receiver_property(property: Value, receiver: &Value) -> Value {
    match property {
        Value::Builtin(builtin)
            if !is_accessor_builtin(builtin)
                && !is_iterator_next_builtin(builtin)
                && crate::intl::tolocale::symbol::name(builtin).is_none() =>
        {
            bind_method(receiver, Value::Builtin(builtin))
        }
        other => other,
    }
}

fn is_iterator_next_builtin(builtin: Builtin) -> bool {
    matches!(
        builtin,
        Builtin::IteratorNext
            | Builtin::RegExpStringIteratorNext
            | Builtin::StringIteratorNext
            | Builtin::SetIteratorNext
            | Builtin::MapIteratorNext
    )
}

/// Accessor getters/setters carry their `this` at invocation time; binding
/// them to the object they were read from (e.g. a property descriptor's
/// `.get`) would call them with the wrong receiver.
fn is_accessor_builtin(builtin: Builtin) -> bool {
    if builtin == Builtin::IntlNumberFormatFormat {
        return false;
    }
    let name = crate::builtins::builtin_name(builtin);
    name.starts_with("get ") || name.starts_with("set ")
}

