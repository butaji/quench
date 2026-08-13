fn realm_default_prototype(target: &Value, new_target: &Value) -> Option<Value> {
    if let Value::BoundFunction(bound) = new_target {
        if let Value::HostCapability(_) = &bound.receiver {
            let builtin = builtin_default_name(target)?;
            return Some(Value::Builtin(builtin));
        }
    }
    if let Value::Function(function) = new_target {
        if let Some(Value::HostCapability(_)) = function
            .properties
            .borrow()
            .iter()
            .rev()
            .find_map(|(name, value)| (name == "\0realm").then(|| value.clone()))
        {
            let builtin = builtin_default_name(target)?;
            return Some(Value::Builtin(builtin));
        }
    }
    builtin_default_prototype(target)
}

fn construct_bound_in_realm(
    bound: &crate::value::BoundFunctionValue,
    builtin: crate::ops::Builtin,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    if let Value::HostCapability(capability) = &bound.receiver {
        return crate::vm::with_realm(capability.realm(), || construct_builtin(builtin, arguments))
            .unwrap_or_else(|| Err(crate::vm::not_callable()));
    }
    construct_builtin(builtin, arguments)
}

fn builtin_default_name(target: &Value) -> Option<crate::ops::Builtin> {
    match target {
        Value::Builtin(crate::ops::Builtin::Boolean) => Some(crate::ops::Builtin::BooleanPrototype),
        Value::Builtin(crate::ops::Builtin::Number) => Some(crate::ops::Builtin::NumberPrototype),
        Value::Builtin(crate::ops::Builtin::String) => Some(crate::ops::Builtin::StringPrototype),
        Value::Builtin(crate::ops::Builtin::Object) => Some(crate::ops::Builtin::ObjectPrototype),
        Value::Builtin(crate::ops::Builtin::Map) => Some(crate::ops::Builtin::MapPrototype),
        Value::Builtin(crate::ops::Builtin::Set) => Some(crate::ops::Builtin::SetPrototype),
        Value::Builtin(crate::ops::Builtin::WeakMap) => Some(crate::ops::Builtin::WeakMapPrototype),
        Value::Builtin(crate::ops::Builtin::WeakSet) => Some(crate::ops::Builtin::WeakSetPrototype),
        _ => None,
    }
}
