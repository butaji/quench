//! Proxy and Reflect builtins for JavaScript Proxy and Reflect API.

use std::rc::Rc;
use std::slice;

use crate::{
    execute::VmError,
    ops::Builtin,
    value::{ProxyValue, Value},
};

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
    let revoke = create_revoke_function(revoked);
    Ok(Value::Object(Rc::new(vec![
        ("proxy".to_string(), proxy),
        ("revoke".to_string(), revoke),
    ])))
}

fn create_revoke_function(revoked: Rc<std::cell::RefCell<bool>>) -> Value {
    let _revoked_clone = revoked;
    Value::Object(Rc::new(vec![(
        "call".to_string(),
        Value::Builtin(Builtin::FunctionCall),
    )]))
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
    if value == Value::Undefined {
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
        Value::Function(func) => crate::functions::execute(func, arguments),
        Value::Builtin(builtin) => {
            crate::execute::execute_builtin_with_receiver(*builtin, arguments, receiver)
        }
        Value::BoundFunction(bound) => crate::functions::execute_bound(bound, arguments),
        _ => Err(VmError::NotCallable),
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
            call_trap(
                &trap,
                &[
                    target.clone(),
                    Value::String(prop.to_string()),
                    receiver.clone(),
                ],
                None,
            )?;
        }
    }
    Ok(crate::execute::get_property(target, prop))
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
                &[target.clone(), Value::String(prop.to_string())],
                None,
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
    Ok(crate::builtins::delete_property(target.clone(), prop))
}

pub(crate) fn proxy_apply(
    target: &Value,
    this_arg: &Value,
    arguments: &[Value],
) -> Result<Value, VmError> {
    if let Value::Proxy(proxy) = target {
        check_revoked(proxy)?;
        if let Some(trap) = get_handler_trap(proxy, "apply") {
            let args_array = Value::Array(Rc::new(arguments.to_vec()));
            return call_trap(&trap, &[target.clone(), this_arg.clone(), args_array], None);
        }
    }
    match target {
        Value::Function(func) => crate::functions::execute(func, arguments),
        Value::Builtin(builtin) => {
            crate::execute::execute_builtin_with_receiver(*builtin, arguments, Some(this_arg))
        }
        Value::BoundFunction(bound) => {
            let mut combined = bound.arguments.clone();
            combined.extend_from_slice(arguments);
            crate::functions::execute_bound(bound, &combined)
        }
        _ => Err(VmError::NotCallable),
    }
}

pub(crate) fn proxy_construct(
    target: &Value,
    arguments: &[Value],
    new_target: Option<&Value>,
) -> Result<Value, VmError> {
    if let Value::Proxy(proxy) = target {
        check_revoked(proxy)?;
        if let Some(trap) = get_handler_trap(proxy, "construct") {
            let args_array = Value::Array(Rc::new(arguments.to_vec()));
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
            return call_trap(&trap, slice::from_ref(target), None);
        }
    }
    Ok(Value::Boolean(true))
}

pub(crate) fn proxy_prevent_extensions(target: &Value) -> Result<Value, VmError> {
    if let Value::Proxy(proxy) = target {
        check_revoked(proxy)?;
        if let Some(trap) = get_handler_trap(proxy, "preventExtensions") {
            return call_trap(&trap, slice::from_ref(target), None);
        }
    }
    Ok(Value::Boolean(true))
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
                &[target.clone(), Value::String(prop.to_string())],
                None,
            );
        }
    }
    Ok(crate::builtins::descriptor(
        Some(target),
        Some(&Value::String(prop.to_string())),
    ))
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
            crate::builtins::has_own_property(
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

fn reflect_define_property(arguments: &[Value]) -> Result<Value, VmError> {
    let target = arguments.first().ok_or(VmError::NotCallable)?;
    let prop = arguments.get(1).map(value_to_string).unwrap_or_default();
    let descriptor = arguments.get(2).cloned().unwrap_or(Value::Undefined);
    proxy_define_property(target, &prop, &descriptor)?;
    Ok(Value::Boolean(true))
}

fn reflect_own_keys(arguments: &[Value]) -> Result<Value, VmError> {
    let target = arguments.first().ok_or(VmError::NotCallable)?;
    proxy_own_keys(target)
}

fn extract_array_arg(arguments: &[Value], index: usize) -> Vec<Value> {
    arguments
        .get(index)
        .and_then(|v| {
            if let Value::Array(arr) = v {
                Some(arr.as_ref().clone())
            } else {
                None
            }
        })
        .unwrap_or_default()
}

fn reflect_apply(arguments: &[Value]) -> Result<Value, VmError> {
    let target = arguments.first().ok_or(VmError::NotCallable)?;
    let this_arg = arguments.get(1).cloned().unwrap_or(Value::Undefined);
    let args = extract_array_arg(arguments, 2);
    proxy_apply(target, &this_arg, &args)
}

fn reflect_construct(arguments: &[Value]) -> Result<Value, VmError> {
    let target = arguments.first().ok_or(VmError::NotCallable)?;
    let args = extract_array_arg(arguments, 1);
    let new_target = arguments.get(2);
    proxy_construct(target, &args, new_target)
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Boolean(b) => b.to_string(),
        Value::Null => "null".to_string(),
        Value::Undefined => "undefined".to_string(),
        _ => "[object Object]".to_string(),
    }
}
