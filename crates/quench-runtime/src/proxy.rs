use crate::{
    execute::VmError,
    ops::{Builtin, FunctionKind},
    value::{ProxyValue, Value},
};
use std::rc::Rc;
use std::slice;
include!("proxy_set.rs");
pub(crate) fn proxy_new(arguments: &[Value]) -> Result<Value, VmError> {
    let target =
        crate::locals::resolved_replacement(arguments.first().ok_or(VmError::NotCallable)?.clone());
    let handler =
        crate::locals::resolved_replacement(arguments.get(1).ok_or(VmError::NotCallable)?.clone());
    validate_proxy_arguments(&target, &handler)?;
    let revoked = Rc::new(std::cell::RefCell::new(false));
    Ok(Value::Proxy(Rc::new(ProxyValue {
        target,
        handler,
        revoked,
        private_slots: Rc::new(std::cell::RefCell::new(Vec::new())),
    })))
}
pub(crate) fn proxy_revocable(arguments: &[Value]) -> Result<Value, VmError> {
    let target =
        crate::locals::resolved_replacement(arguments.first().ok_or(VmError::NotCallable)?.clone());
    let handler =
        crate::locals::resolved_replacement(arguments.get(1).ok_or(VmError::NotCallable)?.clone());
    validate_proxy_arguments(&target, &handler)?;
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
    let revoke = crate::vm::bind_method(&proxy, Value::Builtin(Builtin::ProxyRevoke));
    if let Value::BoundFunction(bound) = &revoke {
        let mut properties = bound.properties.borrow_mut();
        for (name, value) in properties.iter_mut() {
            if name == "name" {
                *value = Value::String(String::new());
            } else if name == &crate::builtins::descriptor_key("name") {
                if let Value::Object(fields) = &*value {
                    let mut entries = fields.properties.clone();
                    if let Some((_, mut value)) = entries.iter_mut().find(|(name, _)| name == "value") {
                        *value = Value::String(String::new());
                    }
                    *value = Value::Object(Rc::new(crate::value::ObjectData::new(
                        entries
                            .iter()
                            .map(|(name, value)| (name.as_str().to_owned(), value.clone()))
                            .collect(),
                    )));
                }
            }
        }
    }
    revoke
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
        let realm = crate::construct::constructor_realm(&proxy.target);
        crate::vm::with_realm(realm, || {
            Err(crate::value::error::throw_type_error(
                "Cannot perform operation on revoked proxy",
            ))
        })
        .unwrap_or_else(|| {
            Err(crate::value::error::throw_type_error(
                "Cannot perform operation on revoked proxy",
            ))
        })
    } else {
        Ok(())
    }
}

pub(crate) fn get_handler_trap(proxy: &ProxyValue, trap: &str) -> Result<Option<Value>, VmError> {
    let value = crate::execute::get_property_result(&proxy.handler, trap)?;
    if matches!(value, Value::Undefined | Value::Null) {
        Ok(None)
    } else {
        Ok(Some(value))
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
    .map_err(|error| match error {
        VmError::NotCallable => crate::value::error::throw_type_error("Proxy trap is not callable"),
        error => error,
    })
}

pub(crate) fn proxy_get(
    target: &Value,
    prop: &str,
    receiver: Option<&Value>,
) -> Result<Value, VmError> {
    if let Value::Proxy(proxy) = target {
        check_revoked(proxy)?;
        if let Some(trap) = get_handler_trap(proxy, "get")? {
            let receiver = receiver.unwrap_or(target);
            let proxy_target = crate::locals::resolved_replacement(proxy.target.clone());
            let result = call_trap(
                &trap,
                &[
                    proxy_target.clone(),
                    crate::conversion::well_known_symbol(prop)
                        .map(Value::Builtin)
                        .unwrap_or_else(|| Value::String(prop.to_string())),
                    receiver.clone(),
                ],
                Some(&proxy.handler),
            )?;
            let descriptor = crate::builtins::object::descriptor(
                Some(&proxy_target),
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
                            .is_some_and(|v| !crate::builtins::same_value(Some(&v), Some(&result)))
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
        if let Some(trap) = get_handler_trap(proxy, "has")? {
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
                if is_non_configurable_descriptor(&descriptor)
                    || (!crate::properties::object_is_extensible(&proxy.target)
                        && !matches!(descriptor, Value::Undefined | Value::Null))
                {
                    return Err(crate::value::error::throw_type_error(
                        "Proxy has invariant violated",
                    ));
                }
            }
            return Ok(Value::Boolean(crate::execute::is_truthy(&result)));
        }
        if matches!(proxy.target, Value::Proxy(_)) {
            return proxy_has(&proxy.target, prop);
        }
        return Ok(Value::Boolean(crate::with_scope::has_property(
            &proxy.target, prop,
        )?));
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
        if let Some(trap) = get_handler_trap(proxy, "deleteProperty")? {
            let result = call_trap(
                &trap,
                &[proxy.target.clone(), Value::String(prop.to_string())],
                Some(&proxy.handler),
            )?;
            let success = crate::execute::is_truthy(&result);
            if success {
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
        return proxy_delete(&proxy.target, prop);
    }
    let target = crate::locals::resolved_replacement(target.clone());
    let (updated, deleted) = crate::builtins::delete_property(target.clone(), prop);
    crate::locals::replace_value(&target, &updated);
    Ok(Value::Boolean(deleted))
}

pub(crate) fn proxy_apply(
    target: &Value,
    this_arg: &Value,
    arguments: &[Value],
) -> Result<Value, VmError> {
    if let Value::Proxy(proxy) = target {
        check_revoked(proxy)?;
        if let Some(trap) = get_handler_trap(proxy, "apply")? {
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
        if let Some(trap) = get_handler_trap(proxy, "construct")? {
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
    if let Value::Proxy(proxy) = target {
        // A proxy without a construct trap forwards [[Construct]] to its
        // target; recursing with the proxy itself would never reach the
        // target's constructor (and can overflow the host stack).
        let result =
            crate::construct::construct_value_with_new_target(&proxy.target, new_target, arguments);
        if is_revoked(proxy) {
            return Err(crate::vm::not_callable());
        }
        return result;
    }
    crate::construct::construct_value_with_new_target(target, new_target, arguments)
}

fn is_constructible(value: &Value) -> bool {
    match value {
        Value::Function(function) => {
            !function.is_async
                && matches!(
                    function.kind,
                    FunctionKind::Ordinary | FunctionKind::ClassConstructor
                )
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
        if let Some(trap) = get_handler_trap(proxy, "getPrototypeOf")? {
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
    if matches!(target, Value::Builtin(Builtin::ObjectPrototype)) {
        let current = crate::builtins::object::get_prototype_of(Some(target))?;
        return Ok(Value::Boolean(crate::builtins::same_value(
            Some(&current),
            Some(prototype),
        )));
    }
    if let Value::Proxy(proxy) = target {
        check_revoked(proxy)?;
        if let Some(trap) = get_handler_trap(proxy, "setPrototypeOf")? {
            let proxy_target = crate::locals::resolved_replacement(proxy.target.clone());
            let result = call_trap(
                &trap,
                &[proxy_target.clone(), prototype.clone()],
                Some(&proxy.handler),
            );
            let result = result?;
            let success = crate::execute::is_truthy(&result);
            let current_target = crate::locals::resolved_replacement(proxy.target.clone());
            if success && !proxy_target_is_extensible(&current_target)? {
                let current = crate::builtins::object::get_prototype_of(Some(&current_target))?;
                if !crate::builtins::same_value(Some(&current), Some(prototype)) {
                    return Err(crate::value::error::throw_type_error(
                        "Proxy setPrototypeOf invariant violated",
                    ));
                }
            }
            return Ok(Value::Boolean(success));
        }
        return proxy_set_prototype_of(&proxy.target, prototype);
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
    let current = crate::locals::resolved_replacement(current);
    let prototype = crate::locals::resolved_replacement(prototype.clone());
    Ok(crate::builtins::same_value(Some(&current), Some(&prototype)))
}

fn proxy_target_is_extensible(target: &Value) -> Result<bool, VmError> {
    if matches!(target, Value::Proxy(_)) {
        return Ok(crate::execute::is_truthy(&proxy_is_extensible(target)?));
    }
    Ok(crate::properties::object_is_extensible(target))
}

fn prototype_contains(prototype: &Value, target: &Value) -> Result<bool, VmError> {
    let target = crate::locals::resolved_replacement(target.clone());
    let mut current = crate::locals::resolved_replacement(prototype.clone());
    while !matches!(current, Value::Null) {
        if crate::builtins::same_value(Some(&current), Some(&target)) {
            return Ok(true);
        }
        current = crate::locals::resolved_replacement(
            crate::builtins::object::get_prototype_of(Some(&current))?,
        );
    }
    Ok(false)
}

pub(crate) fn proxy_is_extensible(target: &Value) -> Result<Value, VmError> {
    if let Value::Proxy(proxy) = target {
        check_revoked(proxy)?;
        let proxy_target = crate::locals::resolved_replacement(proxy.target.clone());
        if let Some(trap) = get_handler_trap(proxy, "isExtensible")? {
            let result = call_trap(&trap, slice::from_ref(&proxy_target), Some(&proxy.handler))?;
            let reported = crate::execute::is_truthy(&result);
            if reported != crate::properties::object_is_extensible(&proxy_target) {
                return Err(crate::value::error::throw_type_error(
                    "Proxy isExtensible invariant violated",
                ));
            }
            return Ok(Value::Boolean(reported));
        }
        let extensible = match proxy_target {
            Value::Proxy(_) => crate::execute::is_truthy(&proxy_is_extensible(&proxy_target)?),
            target => crate::properties::object_is_extensible(&target),
        };
        return Ok(Value::Boolean(extensible));
    }
    require_reflect_object(target)?;
    let target = crate::locals::resolved_replacement(target.clone());
    Ok(Value::Boolean(crate::properties::object_is_extensible(
        &target,
    )))
}

pub(crate) fn proxy_prevent_extensions(target: &Value) -> Result<Value, VmError> {
    if let Value::Proxy(proxy) = target {
        check_revoked(proxy)?;
        if let Some(trap) = get_handler_trap(proxy, "preventExtensions")? {
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
        if let Some(trap) = get_handler_trap(proxy, "getOwnPropertyDescriptor")? {
            let result = call_trap(
                &trap,
                &[proxy.target.clone(), Value::String(prop.to_string())],
                Some(&proxy.handler),
            )?;
            if !matches!(result, Value::Undefined | Value::Object(_)) {
                return Err(crate::value::error::throw_type_error(
                    "Proxy getOwnPropertyDescriptor trap must return an object or undefined",
                ));
            }
            let target_desc = crate::builtins::object::descriptor(
                Some(&proxy.target),
                Some(&Value::String(prop.to_string())),
            )?;
            if matches!(result, Value::Null | Value::Undefined) {
                if !matches!(target_desc, Value::Undefined | Value::Null)
                    && (!crate::properties::object_is_extensible(&proxy.target)
                        || is_non_configurable_descriptor(&target_desc))
                {
                    return Err(crate::value::error::throw_type_error(
                        "Proxy getOwnPropertyDescriptor invariant violated",
                    ));
                }
            } else {
                validate_get_own_property_descriptor_result(&target_desc, &result, &proxy.target)?;
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

fn validate_get_own_property_descriptor_result(
    target_desc: &Value,
    result: &Value,
    target: &Value,
) -> Result<(), VmError> {
    let Value::Object(target_fields) = target_desc else {
        if !crate::properties::object_is_extensible(target) {
            return Err(crate::value::error::throw_type_error(
                "Proxy getOwnPropertyDescriptor invariant violated",
            ));
        }
        if let Value::Object(result_fields) = result {
            if result_fields.iter().any(|(name, value)| {
                name == "configurable" && matches!(value, Value::Boolean(false))
            }) {
                return Err(crate::value::error::throw_type_error(
                    "Proxy getOwnPropertyDescriptor invariant violated",
                ));
            }
        }
        return Ok(());
    };
    let Value::Object(result_fields) = result else {
        return Ok(());
    };
    let result_configurable = result_fields
        .iter()
        .rev()
        .find_map(|(n, v)| (n == "configurable").then_some(v));
    let target_configurable = target_fields
        .iter()
        .rev()
        .find_map(|(n, v)| (n == "configurable").then_some(v));
    if matches!(result_configurable, Some(Value::Boolean(false)))
        && matches!(target_configurable, Some(Value::Boolean(true)))
    {
        return Err(crate::value::error::throw_type_error(
            "Proxy getOwnPropertyDescriptor invariant violated",
        ));
    }
    if matches!(target_configurable, Some(Value::Boolean(false))) {
        let result_configurable = result_fields
            .iter()
            .find_map(|(n, v)| (n == "configurable").then_some(v));
        if !matches!(result_configurable, Some(Value::Boolean(false))) {
            return Err(crate::value::error::throw_type_error(
                "Proxy getOwnPropertyDescriptor invariant violated",
            ));
        }
        for field in [
            "value",
            "writable",
            "get",
            "set",
            "enumerable",
            "configurable",
        ] {
            let Some(expected) = target_fields
                .iter()
                .find_map(|(n, v)| (n == field).then_some(v))
            else {
                continue;
            };
            if let Some(actual) = result_fields
                .iter()
                .find_map(|(n, v)| (n == field).then_some(v))
            {
                if !crate::builtins::same_value(Some(&expected), Some(&actual)) {
                    return Err(crate::value::error::throw_type_error(
                        "Proxy getOwnPropertyDescriptor invariant violated",
                    ));
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn proxy_define_property(
    target: &Value,
    prop: &str,
    descriptor: &Value,
) -> Result<Value, VmError> {
    if let Value::Proxy(proxy) = target {
        check_revoked(proxy)?;
        if let Some(trap) = get_handler_trap(proxy, "defineProperty")? {
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
        return proxy_define_property(&proxy.target, prop, descriptor);
    }
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
    let field = |name: &str| {
        fields
            .iter()
            .rev()
            .find_map(|(candidate, value)| (candidate == name).then_some(value))
    };
    if matches!(&current, Value::Undefined | Value::Null) {
        if !crate::properties::object_is_extensible(target)
            || matches!(field("configurable"), Some(Value::Boolean(false)))
        {
            return Err(crate::value::error::throw_type_error(
                "Proxy defineProperty invariant violated",
            ));
        }
        return Ok(());
    }
    let Value::Object(current_fields) = &current else {
        return Ok(());
    };
    let current_field = |name: &str| {
        current_fields
            .iter()
            .rev()
            .find_map(|(candidate, value)| (candidate == name).then_some(value))
    };
    let non_configurable = matches!(
        &current,
        Value::Object(properties)
            if properties.iter().any(|(name, value)| {
                name == "configurable" && matches!(value, Value::Boolean(false))
            })
    );
    let setting_config_false = matches!(field("configurable"), Some(Value::Boolean(false)));
    let current_config_true = matches!(current_field("configurable"), Some(Value::Boolean(true)));
    if non_configurable
        && fields
            .iter()
            .any(|(name, value)| name == "configurable" && matches!(value, Value::Boolean(true)))
    {
        return Err(crate::value::error::throw_type_error(
            "Proxy defineProperty invariant violated",
        ));
    }
    if setting_config_false && current_config_true {
        return Err(crate::value::error::throw_type_error(
            "Proxy defineProperty invariant violated",
        ));
    }
    if non_configurable {
        if let (Some(current_enum), Some(requested_enum)) =
            (current_field("enumerable"), field("enumerable"))
        {
            if !crate::builtins::same_value(Some(&current_enum), Some(&requested_enum)) {
                return Err(crate::value::error::throw_type_error(
                    "Proxy defineProperty invariant violated",
                ));
            }
        }
    }
    let non_writable = matches!(current_field("writable"), Some(Value::Boolean(false)));
    let current_writable = matches!(current_field("writable"), Some(Value::Boolean(true)));
    if non_configurable
        && current_writable
        && matches!(field("writable"), Some(Value::Boolean(false)))
    {
        return Err(crate::value::error::throw_type_error(
            "Proxy defineProperty invariant violated",
        ));
    }
    if non_configurable && non_writable {
        let current_value = descriptor_value(&current, "value");
        let requested_value = fields
            .iter()
            .rev()
            .find_map(|(name, value)| (name == "value").then_some(value));
        if let Some(current_value) = current_value {
            if let Some(requested_value) = requested_value {
                if !crate::builtins::same_value(Some(&current_value), Some(requested_value)) {
                    return Err(crate::value::error::throw_type_error(
                        "Proxy defineProperty invariant violated",
                    ));
                }
            }
        }
    }
    Ok(())
}

pub fn builtin(builtin: Builtin, arguments: &[Value]) -> Result<Value, VmError> {
    match builtin {
        // Proxy is a constructor only; invoking it without `new` must throw.
        Builtin::Proxy => Err(crate::value::error::throw_type_error(
            "Proxy constructor must be called with new",
        )),
        Builtin::ProxyRevocable => proxy_revocable(arguments),
        Builtin::ReflectGet => reflect_get(arguments),
        Builtin::ReflectSet => reflect_set(arguments),
        Builtin::ReflectHas => reflect_has(arguments),
        Builtin::ReflectDeleteProperty => reflect_delete_property(arguments),
        Builtin::ReflectGetPrototypeOf => reflect_get_prototype_of(arguments),
        Builtin::ReflectSetPrototypeOf => reflect_set_prototype_of(arguments),
        Builtin::ReflectIsExtensible => reflect_is_extensible(arguments),
        Builtin::ReflectPreventExtensions => reflect_prevent_extensions(arguments),
        Builtin::ReflectGetOwnPropertyDescriptor => reflect_get_own_property_descriptor(arguments),
        Builtin::ReflectDefineProperty => reflect_define_property(arguments),
        Builtin::ReflectOwnKeys => reflect_own_keys(arguments),
        Builtin::ReflectApply => reflect_apply(arguments),
        Builtin::ReflectConstruct => reflect_construct(arguments),
        _ => Err(VmError::NotCallable),
    }
}

include!("proxy_reflect.rs");
