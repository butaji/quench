pub(crate) fn proxy_set(
    target: &Value,
    prop: &str,
    value: &Value,
    receiver: Option<&Value>,
) -> Result<Value, VmError> {
    if let Value::Proxy(proxy) = target {
        check_revoked(proxy)?;
        if let Some(trap) = get_handler_trap(proxy, "set") {
            let receiver = receiver.unwrap_or(target);
            let result = call_trap(
                &trap,
                &[
                    proxy.target.clone(),
                    Value::String(prop.to_string()),
                    value.clone(),
                    receiver.clone(),
                ],
                Some(&proxy.handler),
            )?;
            let success = crate::execute::is_truthy(&result);
            if success {
                let descriptor = crate::builtins::object::descriptor(
                    Some(&proxy.target),
                    Some(&Value::String(prop.to_string())),
                )?;
                if let Value::Object(properties) = &descriptor {
                    let non_configurable = properties.iter().any(|(name, v)| {
                        name == "configurable" && matches!(v, Value::Boolean(false))
                    });
                    if non_configurable {
                        let writable = properties.iter().find_map(|(name, v)| {
                            (name == "writable").then_some(v)
                        });
                        if matches!(writable, Some(Value::Boolean(false))) {
                            let current = properties.iter().find_map(|(name, v)| {
                                (name == "value").then_some(v)
                            });
                            if current.is_some_and(|current| {
                                !crate::builtins::same_value(Some(current), Some(value))
                            }) {
                                return Err(crate::value::error::throw_type_error(
                                    "Proxy set invariant violated",
                                ));
                            }
                        } else {
                            let getter_undefined = properties.iter().any(|(name, v)| {
                                name == "get" && matches!(v, Value::Undefined)
                            });
                            if getter_undefined && !matches!(value, Value::Undefined) {
                                return Err(crate::value::error::throw_type_error(
                                    "Proxy set invariant violated",
                                ));
                            }
                        }
                    }
                }
            }
            return Ok(Value::Boolean(success));
        }
        return proxy_set(&proxy.target, prop, value, Some(&proxy.target));
    }
    let receiver = receiver.unwrap_or(target);
    crate::properties::set_with_receiver(target, prop, value, receiver).map(Value::Boolean)
}
