use crate::{
    execute::VmError,
    ops::{Builtin, FunctionKind},
    value::{ProxyValue, Value},
};
use std::rc::Rc;
use std::slice;
include!("proxy_set.rs");
pub(crate) fn proxy_new(arguments: &[Value]) -> Result<Value, VmError> {
    let target = arguments.first().ok_or(VmError::NotCallable)?;
    let handler = arguments.get(1).ok_or(VmError::NotCallable)?;
    validate_proxy_arguments(target, handler)?;
    let revoked = Rc::new(std::cell::RefCell::new(false));
    Ok(Value::Proxy(Rc::new(ProxyValue {
        target: target.clone(),
        handler: handler.clone(),
        revoked,
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
    let value = crate::execute::get_property(&proxy.handler, trap);
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
    match trap {
        Value::Function(func) => crate::functions::execute(
            func,
            receiver.unwrap_or(&crate::value::Value::Undefined),
            arguments,
        ),
        Value::BoundFunction(bound) => crate::functions::execute_bound(bound, arguments),
        Value::Proxy(_) => proxy_apply(
            trap,
            receiver.unwrap_or(&crate::value::Value::Undefined),
            arguments,
        ),
        _ => Err(crate::vm::not_callable()),
    }
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
            return call_trap(
                &trap,
                &[
                    proxy.target.clone(),
                    Value::String(prop.to_string()),
                    receiver.clone(),
                ],
                Some(&proxy.handler),
            );
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
                target: Value::Builtin(builtin),
                receiver: receiver.clone(),
                arguments: Vec::new(),
                properties: std::cell::RefCell::new(Vec::new()),
            },
        )));
    }
    crate::vm::get_property_with_receiver(&proxy.target, prop, receiver)
}

pub(crate) fn proxy_has(target: &Value, prop: &str) -> Result<Value, VmError> {
    if let Value::Proxy(proxy) = target {
        check_revoked(proxy)?;
        if let Some(trap) = get_handler_trap(proxy, "has") {
            return call_trap(
                &trap,
                &[proxy.target.clone(), Value::String(prop.to_string())],
                Some(&proxy.handler),
            );
        }
    }
    let target = match target {
        Value::Proxy(proxy) => &proxy.target,
        target => target,
    };
    Ok(Value::Boolean(crate::with_scope::has_property(
        target, prop,
    )?))
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
            return Ok(Value::Boolean(crate::execute::is_truthy(&result)));
        }
    }
    let (updated, deleted) = crate::builtins::delete_property(target.clone(), prop);
    crate::locals::replace_value(target, &updated);
    Ok(Value::Boolean(deleted))
}

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
        Value::Function(func) => crate::functions::execute(func, this_arg, arguments),
        Value::Builtin(builtin) => {
            crate::execute::execute_builtin_with_receiver(*builtin, arguments, Some(this_arg))
        }
        Value::BoundFunction(bound) => crate::functions::execute_bound(bound, arguments),
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
        return proxy_construct(&proxy.target, arguments, new_target);
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
        Value::Proxy(proxy) => is_constructible(&proxy.target),
        Value::Builtin(builtin) => crate::builtin_meta::constructor_name(*builtin).is_some(),
        _ => false,
    }
}

pub(crate) fn proxy_get_prototype_of(target: &Value) -> Result<Value, VmError> {
    if let Value::Proxy(proxy) = target {
        check_revoked(proxy)?;
        if let Some(trap) = get_handler_trap(proxy, "getPrototypeOf") {
            return call_trap(&trap, slice::from_ref(&proxy.target), Some(&proxy.handler));
        }
        return proxy_get_prototype_of(&proxy.target);
    }
    crate::builtins::object::get_prototype_of(Some(target))
}

pub(crate) fn proxy_set_prototype_of(target: &Value, prototype: &Value) -> Result<Value, VmError> {
    if let Value::Proxy(proxy) = target {
        check_revoked(proxy)?;
        if let Some(trap) = get_handler_trap(proxy, "setPrototypeOf") {
            let result = call_trap(&trap, &[proxy.target.clone(), prototype.clone()], None)?;
            return Ok(Value::Boolean(crate::execute::is_truthy(&result)));
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
            let result = call_trap(&trap, slice::from_ref(&proxy.target), None)?;
            return Ok(Value::Boolean(crate::execute::is_truthy(&result)));
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
            let result = call_trap(&trap, slice::from_ref(&proxy.target), None)?;
            return Ok(Value::Boolean(crate::execute::is_truthy(&result)));
        }
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
            return call_trap(
                &trap,
                &[proxy.target.clone(), Value::String(prop.to_string())],
                Some(&proxy.handler),
            );
        }
    }
    if let Value::Proxy(proxy) = target {
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
            let current = crate::builtins::object::descriptor(
                Some(&proxy.target),
                Some(&Value::String(prop.to_string())),
            )?;
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
                validate_define_property_invariant(&proxy.target, &current, descriptor)?;
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

fn validate_define_property_invariant(
    target: &Value,
    current: &Value,
    descriptor: &Value,
) -> Result<(), VmError> {
    if matches!(current, Value::Undefined) {
        if !crate::properties::object_is_extensible(target)
            || matches!(
                crate::execute::get_property_result(descriptor, "configurable")?,
                Value::Boolean(false)
            )
        {
            return Err(crate::value::error::throw_type_error(
                "Proxy defineProperty trap violates target invariants",
            ));
        }
        return Ok(());
    }
    validate_configurable_invariant(current, descriptor)?;
    validate_writable_invariant(current, descriptor)?;
    validate_value_invariant(current, descriptor)?;
    Ok(())
}

fn validate_configurable_invariant(current: &Value, descriptor: &Value) -> Result<(), VmError> {
    let current = crate::execute::get_property_result(current, "configurable")?;
    let requested = crate::execute::get_property_result(descriptor, "configurable")?;
    if matches!(current, Value::Boolean(false)) && matches!(requested, Value::Boolean(true)) {
        return Err(crate::value::error::throw_type_error(
            "Proxy defineProperty trap violates target invariants",
        ));
    }
    Ok(())
}

fn validate_writable_invariant(current: &Value, descriptor: &Value) -> Result<(), VmError> {
    let current_configurable = crate::execute::get_property_result(current, "configurable")?;
    let current_writable = crate::execute::get_property_result(current, "writable")?;
    let requested_writable = crate::execute::get_property_result(descriptor, "writable")?;
    if matches!(current_configurable, Value::Boolean(false))
        && matches!(current_writable, Value::Boolean(false))
        && matches!(requested_writable, Value::Boolean(true))
    {
        return Err(crate::value::error::throw_type_error(
            "Proxy defineProperty trap violates target invariants",
        ));
    }
    Ok(())
}

fn validate_value_invariant(current: &Value, descriptor: &Value) -> Result<(), VmError> {
    let current_configurable = crate::execute::get_property_result(current, "configurable")?;
    let current_writable = crate::execute::get_property_result(current, "writable")?;
    let requested = crate::execute::get_property_result(descriptor, "value")?;
    if matches!(current_configurable, Value::Boolean(false))
        && matches!(current_writable, Value::Boolean(false))
        && !matches!(requested, Value::Undefined)
    {
        let current = crate::execute::get_property_result(current, "value")?;
        if !crate::builtins::same_value(Some(&current), Some(&requested)) {
            return Err(crate::value::error::throw_type_error(
                "Proxy defineProperty trap violates target invariants",
            ));
        }
    }
    Ok(())
}

pub(crate) fn proxy_own_keys(target: &Value) -> Result<Value, VmError> {
    if let Value::Proxy(proxy) = target {
        check_revoked(proxy)?;
        if let Some(trap) = get_handler_trap(proxy, "ownKeys") {
            return call_trap(&trap, slice::from_ref(&proxy.target), None);
        }
    }
    let target = match target {
        Value::Proxy(proxy) => &proxy.target,
        target => target,
    };
    crate::own_keys::all(target)
}

pub fn builtin(builtin: Builtin, arguments: &[Value]) -> Result<Value, VmError> {
    match builtin {
        Builtin::Proxy => proxy_new(arguments),
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

fn reflect_get(arguments: &[Value]) -> Result<Value, VmError> {
    let target = reflect_target(arguments)?;
    let prop = reflect_property(arguments)?;
    let receiver = arguments.get(2);
    proxy_get(target, &prop, receiver)
}

fn reflect_set(arguments: &[Value]) -> Result<Value, VmError> {
    let target = reflect_target(arguments)?;
    let prop = reflect_property(arguments)?;
    let value = arguments.get(2).cloned().unwrap_or(Value::Undefined);
    let receiver = arguments.get(3);
    proxy_set(target, &prop, &value, receiver)
}

fn reflect_has(arguments: &[Value]) -> Result<Value, VmError> {
    let target = reflect_target(arguments)?;
    let prop = reflect_property(arguments)?;
    proxy_has(target, &prop)
}

fn reflect_delete_property(arguments: &[Value]) -> Result<Value, VmError> {
    let target = reflect_target(arguments)?;
    let prop = reflect_property(arguments)?;
    proxy_delete(target, &prop)
}

fn reflect_get_prototype_of(arguments: &[Value]) -> Result<Value, VmError> {
    let target = reflect_target(arguments)?;
    proxy_get_prototype_of(target)
}

fn reflect_set_prototype_of(arguments: &[Value]) -> Result<Value, VmError> {
    let target = reflect_target(arguments)?;
    let prototype = arguments.get(1).cloned().unwrap_or(Value::Null);
    proxy_set_prototype_of(target, &prototype)
}

fn reflect_is_extensible(arguments: &[Value]) -> Result<Value, VmError> {
    let target = reflect_target(arguments)?;
    proxy_is_extensible(target)
}

fn reflect_prevent_extensions(arguments: &[Value]) -> Result<Value, VmError> {
    let target = reflect_target(arguments)?;
    proxy_prevent_extensions(target)
}

fn reflect_get_own_property_descriptor(arguments: &[Value]) -> Result<Value, VmError> {
    let target = reflect_target(arguments)?;
    let prop = reflect_property(arguments)?;
    proxy_get_own_property_descriptor(target, &prop)
}

fn reflect_target(arguments: &[Value]) -> Result<&Value, VmError> {
    let target = arguments.first().ok_or(VmError::NotCallable)?;
    require_reflect_object(target)?;
    Ok(target)
}

fn reflect_property(arguments: &[Value]) -> Result<String, VmError> {
    crate::conversion::to_property_key(arguments.get(1).unwrap_or(&Value::Undefined))
}

include!("proxy_reflect.rs");
