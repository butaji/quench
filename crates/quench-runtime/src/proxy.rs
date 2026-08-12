//! Proxy and Reflect builtins for JavaScript Proxy and Reflect API.
use crate::{
    execute::VmError,
    ops::{Builtin, FunctionKind},
    value::{ProxyValue, Value},
};
use std::rc::Rc;
use std::slice;
pub(crate) fn proxy_new(arguments: &[Value]) -> Result<Value, VmError> {
    let target = arguments.first().ok_or(VmError::NotCallable)?;
    let handler = arguments.get(1).ok_or(VmError::NotCallable)?;
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

fn create_revoke_function(proxy: Value) -> Value {
    Value::BoundFunction(Rc::new(crate::value::BoundFunctionValue {
        target: Value::Builtin(Builtin::ProxyRevoke),
        receiver: proxy,
        arguments: Vec::new(),
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
        Err(VmError::EvalError(
            "Cannot perform operation on revoked proxy".to_string(),
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
                    target.clone(),
                    Value::String(prop.to_string()),
                    receiver.clone(),
                ],
                None,
            );
        }
        return proxy_target_property(proxy, prop, receiver.unwrap_or(target));
    }
    Ok(crate::execute::get_property(target, prop))
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
            },
        )));
    }
    Ok(crate::execute::get_property(&proxy.target, prop))
}

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
            call_trap(
                &trap,
                &[
                    target.clone(),
                    Value::String(prop.to_string()),
                    value.clone(),
                    receiver.clone(),
                ],
                None,
            )?;
            return Ok(Value::Boolean(true));
        }
    }
    Ok(crate::builtins::set_property(
        target.clone(),
        prop,
        value.clone(),
    ))
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
    Ok(Value::Boolean(has_own_property(target, prop)))
}

pub(crate) fn proxy_delete(target: &Value, prop: &str) -> Result<Value, VmError> {
    if let Value::Proxy(proxy) = target {
        check_revoked(proxy)?;
        if let Some(trap) = get_handler_trap(proxy, "deleteProperty") {
            return call_trap(
                &trap,
                &[target.clone(), Value::String(prop.to_string())],
                None,
            );
        }
    }
    let (_, deleted) = crate::builtins::delete_property(target.clone(), prop);
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
            return call_trap(&trap, &[target.clone(), this_arg.clone(), args_array], None);
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
            return call_trap(
                &trap,
                &[target.clone(), args_array, new_target.clone()],
                None,
            );
        }
    }
    crate::construct::construct_value(target, arguments)
}

fn is_constructible(value: &Value) -> bool {
    match value {
        Value::Function(function) => {
            !function.is_async && matches!(function.kind, FunctionKind::Ordinary)
        }
        Value::BoundFunction(bound) => is_constructible(&bound.target),
        Value::Proxy(_) => true,
        Value::Builtin(builtin) => crate::builtin_meta::constructor_name(*builtin).is_some(),
        _ => false,
    }
}

pub(crate) fn proxy_get_prototype_of(target: &Value) -> Result<Value, VmError> {
    if let Value::Proxy(proxy) = target {
        check_revoked(proxy)?;
        if let Some(trap) = get_handler_trap(proxy, "getPrototypeOf") {
            return call_trap(&trap, slice::from_ref(target), None);
        }
    }
    Ok(get_prototype_of(target))
}

pub(crate) fn proxy_set_prototype_of(target: &Value, prototype: &Value) -> Result<Value, VmError> {
    if let Value::Proxy(proxy) = target {
        check_revoked(proxy)?;
        if let Some(trap) = get_handler_trap(proxy, "setPrototypeOf") {
            return call_trap(&trap, &[target.clone(), prototype.clone()], None);
        }
    }
    Ok(prototype.clone())
}

pub(crate) fn proxy_is_extensible(target: &Value) -> Result<Value, VmError> {
    if let Value::Proxy(proxy) = target {
        check_revoked(proxy)?;
        if let Some(trap) = get_handler_trap(proxy, "isExtensible") {
            return call_trap(&trap, slice::from_ref(&proxy.target), None);
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
            return call_trap(&trap, slice::from_ref(&proxy.target), None);
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
            return call_trap(
                &trap,
                &[
                    target.clone(),
                    Value::String(prop.to_string()),
                    descriptor.clone(),
                ],
                None,
            );
        }
    }
    Ok(Value::Boolean(true))
}

pub(crate) fn proxy_own_keys(target: &Value) -> Result<Value, VmError> {
    if let Value::Proxy(proxy) = target {
        check_revoked(proxy)?;
        if let Some(trap) = get_handler_trap(proxy, "ownKeys") {
            return call_trap(&trap, slice::from_ref(target), None);
        }
    }
    if let Value::Proxy(proxy) = target {
        return Ok(crate::builtins::keys(Some(&proxy.target)));
    }
    Ok(crate::builtins::keys(Some(target)))
}

fn has_own_property(target: &Value, prop: &str) -> bool {
    match target {
        Value::Object(properties) => properties.iter().any(|(name, _)| name == prop),
        Value::Array(values) => {
            prop == "length" || prop.parse::<usize>().is_ok_and(|i| i < values.len())
        }
        Value::String(value) => {
            prop == "length"
                || prop
                    .parse::<usize>()
                    .is_ok_and(|i| i < value.chars().count())
        }
        Value::Builtin(builtin) => {
            crate::builtins::object::has_own_property(
                Some(&Value::Builtin(*builtin)),
                Some(&Value::String(prop.to_string())),
            ) == Value::Boolean(true)
        }
        _ => false,
    }
}

fn get_prototype_of(target: &Value) -> Value {
    match target {
        Value::Object(_) | Value::Array(_) | Value::Function(_) | Value::Proxy(_) => Value::Null,
        Value::Builtin(builtin) => {
            crate::builtin_meta::prototype(*builtin).map_or(Value::Null, Value::Builtin)
        }
        _ => Value::Null,
    }
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
    let target = arguments.first().ok_or(VmError::NotCallable)?;
    let prop = arguments.get(1).map(value_to_string).unwrap_or_default();
    let receiver = arguments.get(2);
    proxy_get(target, &prop, receiver)
}

fn reflect_set(arguments: &[Value]) -> Result<Value, VmError> {
    let target = arguments.first().ok_or(VmError::NotCallable)?;
    let prop = arguments.get(1).map(value_to_string).unwrap_or_default();
    let value = arguments.get(2).cloned().unwrap_or(Value::Undefined);
    let receiver = arguments.get(3);
    proxy_set(target, &prop, &value, receiver)?;
    Ok(Value::Boolean(true))
}

fn reflect_has(arguments: &[Value]) -> Result<Value, VmError> {
    let target = arguments.first().ok_or(VmError::NotCallable)?;
    let prop = arguments.get(1).map(value_to_string).unwrap_or_default();
    proxy_has(target, &prop)
}

fn reflect_delete_property(arguments: &[Value]) -> Result<Value, VmError> {
    let target = arguments.first().ok_or(VmError::NotCallable)?;
    let prop = arguments.get(1).map(value_to_string).unwrap_or_default();
    proxy_delete(target, &prop)
}

fn reflect_get_prototype_of(arguments: &[Value]) -> Result<Value, VmError> {
    let target = arguments.first().ok_or(VmError::NotCallable)?;
    proxy_get_prototype_of(target)
}

fn reflect_set_prototype_of(arguments: &[Value]) -> Result<Value, VmError> {
    let target = arguments.first().ok_or(VmError::NotCallable)?;
    let prototype = arguments.get(1).cloned().unwrap_or(Value::Null);
    proxy_set_prototype_of(target, &prototype)
}

fn reflect_is_extensible(arguments: &[Value]) -> Result<Value, VmError> {
    let target = arguments.first().ok_or(VmError::NotCallable)?;
    proxy_is_extensible(target)
}

fn reflect_prevent_extensions(arguments: &[Value]) -> Result<Value, VmError> {
    let target = arguments.first().ok_or(VmError::NotCallable)?;
    proxy_prevent_extensions(target)
}

fn reflect_get_own_property_descriptor(arguments: &[Value]) -> Result<Value, VmError> {
    let target = arguments.first().ok_or(VmError::NotCallable)?;
    let prop = arguments.get(1).map(value_to_string).unwrap_or_default();
    proxy_get_own_property_descriptor(target, &prop)
}

include!("proxy_reflect.rs");
