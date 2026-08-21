pub(crate) fn proxy_new(arguments: &[Value]) -> Result<Value, VmError> {
    let target = arguments.first().ok_or(VmError::NotCallable)?;
    let handler = arguments.get(1).ok_or(VmError::NotCallable)?;
    validate_proxy_arguments(target, handler)?;
    let revoked = Rc::new(std::cell::RefCell::new(false));
    Ok(Value::Proxy(Rc::new(ProxyValue {
        target: target.clone(),
        handler: handler.clone(),
        revoked,
        private_slots: Rc::new(std::cell::RefCell::new(Vec::new())),
    })))
}
pub(crate) fn proxy_revocable(arguments: &[Value]) -> Result<Value, VmError> {
    let target = arguments.first().ok_or(VmError::NotCallable)?;
    let handler = arguments.get(1).ok_or(VmError::NotCallable)?;
    validate_proxy_arguments(target, handler)?;
    let revoked = Rc::new(std::cell::RefCell::new(false));
    let proxy = Value::Proxy(Rc::new(ProxyValue {
        target: target.clone(),
        handler: handler.clone(),
        revoked: revoked.clone(),
        private_slots: Rc::new(std::cell::RefCell::new(Vec::new())),
    }));
    let revoke = create_revoke_function(proxy.clone());
    Ok(Value::Object(Rc::new(crate::value::ObjectData::new(vec![
        ("proxy".to_string(), proxy),
        ("revoke".to_string(), revoke),
    ]))))
}

fn validate_proxy_arguments(target: &Value, handler: &Value) -> Result<(), VmError> {
    if !crate::value::is_object(target) || !crate::value::is_object(handler) {
        return Err(crate::value::error::throw_type_error(
            "Proxy target and handler must be objects",
        ));
    }
    Ok(())
}

fn create_revoke_function(proxy: Value) -> Value {
    Value::BoundFunction(Rc::new(crate::value::BoundFunctionValue {
        realm: crate::vm::current_context_or_default().realm(),
        target: Value::Builtin(Builtin::ProxyRevoke),
        receiver: proxy,
        arguments: Vec::new(),
        properties: std::cell::RefCell::new(Vec::new()),
    }))
}

pub(crate) fn revoke(receiver: Option<&Value>) -> Result<Value, VmError> {
    let Some(Value::Proxy(proxy)) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "Proxy revoke called on incompatible receiver",
        ));
    };
    *proxy.revoked.borrow_mut() = true;
    Ok(Value::Undefined)
}

pub(crate) fn is_revoked(proxy: &ProxyValue) -> bool {
    *proxy.revoked.borrow()
}

fn check_revoked(proxy: &ProxyValue) -> Result<(), VmError> {
    if is_revoked(proxy) {
        Err(crate::value::error::throw_type_error(
            "Cannot perform operation on revoked proxy",
        ))
    } else {
        Ok(())
    }
}

pub(crate) fn get_handler_trap(proxy: &ProxyValue, trap: &str) -> Option<Value> {
    let value =
        crate::execute::get_property_result(&proxy.handler, trap).unwrap_or(Value::Undefined);
    if matches!(value, Value::Undefined | Value::Null) {
        None
    } else {
        Some(value)
    }
}

pub(crate) fn call_trap(
    trap: &Value,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Result<Value, VmError> {
    crate::functions::execute_target(
        trap,
        receiver.unwrap_or(&crate::value::Value::Undefined),
        arguments,
    )
}

pub(crate) fn proxy_get(
    target: &Value,
    prop: &str,
    receiver: Option<&Value>,
) -> Result<Value, VmError> {
    if let Value::Proxy(proxy) = target {
        check_revoked(proxy)?;
        if let Some(trap) = get_handler_trap(proxy, "get") {
            let receiver = receiver.unwrap_or(target);
            let result = call_trap(
                &trap,
                &[
                    proxy.target.clone(),
                    crate::conversion::property_key_value(prop),
                    receiver.clone(),
                ],
                Some(&proxy.handler),
            )?;
            let descriptor = crate::builtins::object::descriptor(
                Some(&proxy.target),
                Some(&Value::String(prop.to_string())),
            )?;
            if let Value::Object(properties) = &descriptor {
                let non_configurable = properties
                    .iter()
                    .any(|(n, v)| n == "configurable" && matches!(v, Value::Boolean(false)));
                if non_configurable {
                    let value_desc = properties
                        .iter()
                        .find_map(|(n, v)| (n == "value").then_some(v));
                    let writable = properties
                        .iter()
                        .find_map(|(n, v)| (n == "writable").then_some(v));
                    if matches!(writable, Some(Value::Boolean(false)))
                        && value_desc
                            .is_some_and(|v| !crate::builtins::same_value(Some(v), Some(&result)))
                    {
                        return Err(crate::value::error::throw_type_error(
                            "Proxy get invariant violated",
                        ));
                    }
                    if properties
                        .iter()
                        .any(|(n, v)| n == "get" && matches!(v, Value::Undefined))
                        && !matches!(result, Value::Undefined)
                    {
                        return Err(crate::value::error::throw_type_error(
                            "Proxy get invariant violated",
                        ));
                    }
                }
            }
            return Ok(result);
        }
        return proxy_target_property(proxy, prop, receiver.unwrap_or(target));
    }
    crate::vm::get_property_with_receiver(target, prop, receiver.unwrap_or(target))
}

fn proxy_target_property(
    proxy: &ProxyValue,
    prop: &str,
    receiver: &Value,
) -> Result<Value, VmError> {
    if matches!(prop, "apply" | "call" | "bind") && crate::conversion::is_callable(&proxy.target) {
        let builtin = match prop {
            "apply" => Builtin::FunctionApply,
            "call" => Builtin::FunctionCall,
            "bind" => Builtin::FunctionBind,
            _ => return Err(VmError::NotCallable),
        };
        return Ok(Value::BoundFunction(Rc::new(
            crate::value::BoundFunctionValue {
                realm: crate::vm::current_context_or_default().realm(),
                target: Value::Builtin(builtin),
                receiver: receiver.clone(),
                arguments: Vec::new(),
                properties: std::cell::RefCell::new(Vec::new()),
            },
        )));
    }
    crate::vm::get_property_with_receiver(
        &crate::locals::resolved_replacement(proxy.target.clone()),
        prop,
        receiver,
    )
}

pub(crate) fn proxy_has(target: &Value, prop: &str) -> Result<Value, VmError> {
    if let Value::Proxy(proxy) = target {
        check_revoked(proxy)?;
        if let Some(trap) = get_handler_trap(proxy, "has") {
            let result = call_trap(
                &trap,
                &[proxy.target.clone(), Value::String(prop.to_string())],
                Some(&proxy.handler),
            )?;
            if !crate::execute::is_truthy(&result) {
                let descriptor = crate::builtins::object::descriptor(
                    Some(&proxy.target),
                    Some(&Value::String(prop.to_string())),
                )?;
                if is_non_configurable_descriptor(&descriptor) {
                    return Err(crate::value::error::throw_type_error(
                        "Proxy has invariant violated",
                    ));
                }
            }
            return Ok(Value::Boolean(crate::execute::is_truthy(&result)));
        }
    }
    Ok(Value::Boolean(crate::with_scope::has_property(
        target, prop,
    )?))
}

fn is_non_configurable_descriptor(descriptor: &Value) -> bool {
    let Value::Object(properties) = descriptor else {
        return false;
    };
    properties
        .iter()
        .any(|(name, value)| name == "configurable" && matches!(value, Value::Boolean(false)))
}

pub(crate) fn proxy_delete(target: &Value, prop: &str) -> Result<Value, VmError> {
    if let Value::Proxy(proxy) = target {
        check_revoked(proxy)?;
        if let Some(trap) = get_handler_trap(proxy, "deleteProperty") {
            let result = call_trap(
                &trap,
                &[proxy.target.clone(), Value::String(prop.to_string())],
                Some(&proxy.handler),
            )?;
            let success = crate::execute::is_truthy(&result);
            if !success {
                let descriptor = crate::builtins::object::descriptor(
                    Some(&proxy.target),
                    Some(&Value::String(prop.to_string())),
                )?;
                if is_non_configurable_descriptor(&descriptor)
                    || (!crate::properties::object_is_extensible(&proxy.target)
                        && !matches!(descriptor, Value::Undefined | Value::Null))
                {
                    return Err(crate::value::error::throw_type_error(
                        "Proxy delete invariant violated",
                    ));
                }
            }
            return Ok(Value::Boolean(success));
        }
    }
    let (updated, deleted) = crate::builtins::delete_property(target.clone(), prop);
    crate::locals::replace_value(target, &updated);
    Ok(Value::Boolean(deleted))
}
include!("proxy_ops_tail.rs");
