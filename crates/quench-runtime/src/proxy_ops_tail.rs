pub(crate) fn proxy_apply(
    target: &Value,
    this_arg: &Value,
    arguments: &[Value],
) -> Result<Value, VmError> {
    if let Value::Proxy(proxy) = target {
        check_revoked(proxy)?;
        if let Some(trap) = get_handler_trap(proxy, "apply") {
            let args_array = Value::array(arguments.to_vec());
            return call_trap(
                &trap,
                &[proxy.target.clone(), this_arg.clone(), args_array],
                Some(&proxy.handler),
            );
        }
        return proxy_apply(&proxy.target, this_arg, arguments);
    }
    match target {
        Value::Function(_) | Value::Builtin(_) | Value::BoundFunction(_) => {
            crate::functions::execute_target(target, this_arg, arguments)
        }
        _ => Err(VmError::NotCallable),
    }
}

pub(crate) fn proxy_construct(
    target: &Value,
    arguments: &[Value],
    new_target: Option<&Value>,
) -> Result<Value, VmError> {
    if let Some(new_target) = new_target {
        if !is_constructible(new_target) {
            return Err(VmError::Thrown(crate::builtins::error(
                Builtin::TypeError,
                &[Value::String("Target is not a constructor".to_string())],
            )));
        }
    }
    if let Value::Proxy(proxy) = target {
        check_revoked(proxy)?;
        if let Some(trap) = get_handler_trap(proxy, "construct") {
            let args_array = Value::array(arguments.to_vec());
            let new_target = new_target.unwrap_or(target);
            let result = call_trap(
                &trap,
                &[proxy.target.clone(), args_array, new_target.clone()],
                Some(&proxy.handler),
            )?;
            if !crate::value::is_object(&result) {
                return Err(crate::value::error::throw_type_error(
                    "Proxy construct trap must return an object",
                ));
            }
            return Ok(result);
        }
    }
    let new_target = new_target.unwrap_or(target);
    crate::construct::construct_value_with_new_target(target, new_target, arguments)
}

fn is_constructible(value: &Value) -> bool {
    match value {
        Value::Function(function) => {
            !function.is_async && matches!(function.kind, FunctionKind::Ordinary)
        }
        Value::BoundFunction(bound) => is_constructible(&bound.target),
        // A proxy is constructible exactly when its target is constructible.
        // The proxy itself does not acquire a [[Construct]] slot merely by
        // being a proxy.
        Value::Proxy(proxy) => is_constructible(&proxy.target),
        Value::Builtin(builtin) => crate::builtin_meta::constructor_name(*builtin).is_some(),
        _ => false,
    }
}

pub(crate) fn proxy_get_prototype_of(target: &Value) -> Result<Value, VmError> {
    if let Value::Proxy(proxy) = target {
        check_revoked(proxy)?;
        if let Some(trap) = get_handler_trap(proxy, "getPrototypeOf") {
            let result = call_trap(&trap, slice::from_ref(&proxy.target), Some(&proxy.handler))?;
            if !matches!(result, Value::Null) && !crate::value::is_object(&result) {
                return Err(crate::value::error::throw_type_error(
                    "Proxy getPrototypeOf trap must return an object or null",
                ));
            }
            if !crate::properties::object_is_extensible(&proxy.target) {
                let target_proto = crate::builtins::object::get_prototype_of(Some(&proxy.target))?;
                if !crate::builtins::same_value(Some(&result), Some(&target_proto)) {
                    return Err(crate::value::error::throw_type_error(
                        "Proxy getPrototypeOf invariant violated",
                    ));
                }
            }
            return Ok(result);
        }
        return crate::builtins::object::get_prototype_of(Some(&proxy.target));
    }
    crate::builtins::object::get_prototype_of(Some(target))
}

pub(crate) fn proxy_set_prototype_of(target: &Value, prototype: &Value) -> Result<Value, VmError> {
    if let Value::Proxy(proxy) = target {
        check_revoked(proxy)?;
        if let Some(trap) = get_handler_trap(proxy, "setPrototypeOf") {
            let result = call_trap(
                &trap,
                &[proxy.target.clone(), prototype.clone()],
                Some(&proxy.handler),
            )?;
            let success = crate::execute::is_truthy(&result);
            if success && !crate::properties::object_is_extensible(&proxy.target) {
                let current = crate::builtins::object::get_prototype_of(Some(&proxy.target))?;
                if !crate::builtins::same_value(Some(&current), Some(prototype)) {
                    return Err(crate::value::error::throw_type_error(
                        "Proxy setPrototypeOf invariant violated",
                    ));
                }
            }
            return Ok(Value::Boolean(success));
        }
    }
    if prototype_matches(target, prototype)? {
        return Ok(Value::Boolean(true));
    }
    if !crate::properties::object_is_extensible(target) || prototype_contains(prototype, target)? {
        return Ok(Value::Boolean(false));
    }
    let updated = crate::builtins::object::set_prototype_of(&[target.clone(), prototype.clone()])?;
    crate::locals::replace_value(target, &updated);
    Ok(Value::Boolean(true))
}

fn prototype_matches(target: &Value, prototype: &Value) -> Result<bool, VmError> {
    let current = crate::builtins::object::get_prototype_of(Some(target))?;
    Ok(crate::builtins::same_value(Some(&current), Some(prototype)))
}

fn prototype_contains(prototype: &Value, target: &Value) -> Result<bool, VmError> {
    let mut current = prototype.clone();
    while !matches!(current, Value::Null) {
        if crate::builtins::same_value(Some(&current), Some(target)) {
            return Ok(true);
        }
        current = crate::builtins::object::get_prototype_of(Some(&current))?;
    }
    Ok(false)
}

pub(crate) fn proxy_is_extensible(target: &Value) -> Result<Value, VmError> {
    if let Value::Proxy(proxy) = target {
        check_revoked(proxy)?;
        if let Some(trap) = get_handler_trap(proxy, "isExtensible") {
            let result = call_trap(&trap, slice::from_ref(&proxy.target), Some(&proxy.handler))?;
            let reported = crate::execute::is_truthy(&result);
            if reported != crate::properties::object_is_extensible(&proxy.target) {
                return Err(crate::value::error::throw_type_error(
                    "Proxy isExtensible invariant violated",
                ));
            }
            return Ok(Value::Boolean(reported));
        }
    }
    require_reflect_object(target)?;
    Ok(Value::Boolean(crate::properties::object_is_extensible(
        target,
    )))
}

pub(crate) fn proxy_prevent_extensions(target: &Value) -> Result<Value, VmError> {
    if let Value::Proxy(proxy) = target {
        check_revoked(proxy)?;
        if let Some(trap) = get_handler_trap(proxy, "preventExtensions") {
            let result = call_trap(&trap, slice::from_ref(&proxy.target), Some(&proxy.handler))?;
            let success = crate::execute::is_truthy(&result);
            if success && crate::properties::object_is_extensible(&proxy.target) {
                return Err(crate::value::error::throw_type_error(
                    "Proxy preventExtensions invariant violated",
                ));
            }
            return Ok(Value::Boolean(success));
        }
        crate::properties::prevent_extensions(Some(&proxy.target))?;
        return Ok(Value::Boolean(true));
    }
    require_reflect_object(target)?;
    let _ = crate::properties::prevent_extensions(Some(target))?;
    Ok(Value::Boolean(true))
}

fn require_reflect_object(target: &Value) -> Result<(), VmError> {
    if crate::value::is_object(target) {
        return Ok(());
    }
    Err(crate::value::error::throw_type_error(
        "Reflect target must be an object",
    ))
}

pub(crate) fn proxy_get_own_property_descriptor(
    target: &Value,
    prop: &str,
) -> Result<Value, VmError> {
    if let Value::Proxy(proxy) = target {
        check_revoked(proxy)?;
        if let Some(trap) = get_handler_trap(proxy, "getOwnPropertyDescriptor") {
            let result = call_trap(
                &trap,
                &[proxy.target.clone(), Value::String(prop.to_string())],
                Some(&proxy.handler),
            )?;
            if !matches!(result, Value::Null | Value::Undefined | Value::Object(_)) {
                return Err(crate::value::error::throw_type_error(
                    "Proxy getOwnPropertyDescriptor trap must return an object or null",
                ));
            }
            let target_desc = crate::builtins::object::descriptor(
                Some(&proxy.target),
                Some(&Value::String(prop.to_string())),
            )?;
            if matches!(result, Value::Null | Value::Undefined)
                && !matches!(target_desc, Value::Undefined | Value::Null)
                && (!crate::properties::object_is_extensible(&proxy.target)
                    || is_non_configurable_descriptor(&target_desc))
            {
                return Err(crate::value::error::throw_type_error(
                    "Proxy getOwnPropertyDescriptor invariant violated",
                ));
            }
            return Ok(result);
        }
        return crate::builtins::object::descriptor(
            Some(&proxy.target),
            Some(&Value::String(prop.to_string())),
        );
    }
    crate::builtins::object::descriptor(Some(target), Some(&Value::String(prop.to_string())))
}

pub(crate) fn proxy_define_property(
    target: &Value,
    prop: &str,
    descriptor: &Value,
) -> Result<Value, VmError> {
    if let Value::Proxy(proxy) = target {
        check_revoked(proxy)?;
        if let Some(trap) = get_handler_trap(proxy, "defineProperty") {
            let result = call_trap(
                &trap,
                &[
                    proxy.target.clone(),
                    Value::String(prop.to_string()),
                    descriptor.clone(),
                ],
                Some(&proxy.handler),
            )?;
            if crate::execute::is_truthy(&result) {
                validate_define_invariant(&proxy.target, prop, descriptor)?;
            }
            return Ok(result);
        }
    }
    let target = match target {
        Value::Proxy(proxy) => &proxy.target,
        target => target,
    };
    let updated = crate::builtins::define_property(&[
        target.clone(),
        Value::String(prop.to_string()),
        descriptor.clone(),
    ])?;
    crate::locals::replace_value(target, &updated);
    Ok(Value::Boolean(true))
}

fn validate_define_invariant(
    target: &Value,
    prop: &str,
    descriptor: &Value,
) -> Result<(), VmError> {
    let current =
        crate::builtins::object::descriptor(Some(target), Some(&Value::String(prop.to_string())))?;
    let fields = crate::builtins::descriptor_fields(descriptor)?;
    let non_configurable = matches!(
        &current,
        Value::Object(properties)
            if properties.iter().any(|(name, value)| {
                name == "configurable" && matches!(value, Value::Boolean(false))
            })
    );
    if non_configurable
        && fields
            .iter()
            .any(|(name, value)| name == "configurable" && matches!(value, Value::Boolean(true)))
    {
        return Err(crate::value::error::throw_type_error(
            "Proxy defineProperty invariant violated",
        ));
    }
    let non_writable = matches!(
        &current,
        Value::Object(properties)
            if properties.iter().any(|(name, value)| {
                name == "writable" && matches!(value, Value::Boolean(false))
            })
    );
    if non_configurable && non_writable {
        let current_value = descriptor_value(&current, "value");
        let requested_value = fields
            .iter()
            .rev()
            .find_map(|(name, value)| (name == "value").then_some(value));
        if let Some(current_value) = current_value {
            if let Some(requested_value) = requested_value {
                if !crate::builtins::same_value(Some(current_value), Some(requested_value)) {
                    return Err(crate::value::error::throw_type_error(
                        "Proxy defineProperty invariant violated",
                    ));
                }
            }
        }
    }
    Ok(())
}